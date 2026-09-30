//! Windows Graphics Capture (the OS screen-capture API, no injection into the game) plus
//! GPU color conversion and scaling (BGRA -> NV12) with the D3D11 video processor.
//! Frames never leave the GPU until the hardware encoder reads them.

use super::d3d::Gpu;
use super::video_enc::EncIn;
use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{Sender, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use windows::core::{Interface, IInspectable};
use windows::Foundation::{TimeSpan, TypedEventHandler};
use windows::Graphics::Capture::{Direct3D11CaptureFramePool, GraphicsCaptureAccess, GraphicsCaptureAccessKind, GraphicsCaptureItem, GraphicsCaptureSession};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Graphics::SizeInt32;
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_NV12, DXGI_RATIONAL};
use windows::Win32::Graphics::Gdi::HMONITOR;
use windows::Win32::System::WinRT::Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;

pub const RING: usize = 8;
const MAX_IN_FLIGHT: usize = 5;

#[derive(Clone, Copy)]
pub enum Target {
    Window(HWND),
    Monitor(HMONITOR),
}
unsafe impl Send for Target {}

pub fn create_item(target: Target) -> Result<GraphicsCaptureItem> {
    let interop = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
    unsafe {
        Ok(match target {
            Target::Window(h) => interop.CreateForWindow(h).context("can't capture the game window")?,
            Target::Monitor(m) => interop.CreateForMonitor(m).context("can't capture the screen")?,
        })
    }
}

/// Output size for the encoder: `height` (0 = source), same aspect, even numbers, no upscaling.
pub fn output_size(src_w: u32, src_h: u32, height: u32) -> (u32, u32) {
    let h = if height == 0 || height >= src_h { src_h } else { height };
    let w = (src_w as u64 * h as u64 / src_h.max(1) as u64) as u32;
    ((w & !1).max(64), (h & !1).max(64))
}

struct Converter {
    vp: ID3D11VideoProcessor,
    vctx: ID3D11VideoContext,
    in_view: ID3D11VideoProcessorInputView,
    out_views: Vec<ID3D11VideoProcessorOutputView>,
    in_w: u32,
    in_h: u32,
}

impl Converter {
    unsafe fn new(gpu: &Gpu, input: &ID3D11Texture2D, in_w: u32, in_h: u32, outputs: &[ID3D11Texture2D], out_w: u32, out_h: u32, fps: u32) -> Result<Converter> {
        unsafe {
            let vdev: ID3D11VideoDevice = gpu.device.cast()?;
            let vctx: ID3D11VideoContext = gpu.context.cast()?;
            let rate = DXGI_RATIONAL { Numerator: fps, Denominator: 1 };
            let desc = D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
                InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
                InputFrameRate: rate,
                InputWidth: in_w,
                InputHeight: in_h,
                OutputFrameRate: rate,
                OutputWidth: out_w,
                OutputHeight: out_h,
                Usage: D3D11_VIDEO_USAGE_PLAYBACK_NORMAL,
            };
            let en = vdev.CreateVideoProcessorEnumerator(&desc)?;
            let vp = vdev.CreateVideoProcessor(&en, 0)?;
            let mut ivd = D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC { FourCC: 0, ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D, ..Default::default() };
            ivd.Anonymous.Texture2D = D3D11_TEX2D_VPIV { MipSlice: 0, ArraySlice: 0 };
            let mut in_view = None;
            vdev.CreateVideoProcessorInputView(input, &en, &ivd, Some(&mut in_view))?;
            let mut out_views = Vec::new();
            for t in outputs {
                let mut ovd = D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC { ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D, ..Default::default() };
                ovd.Anonymous.Texture2D = D3D11_TEX2D_VPOV { MipSlice: 0 };
                let mut v = None;
                vdev.CreateVideoProcessorOutputView(t, &en, &ovd, Some(&mut v))?;
                out_views.push(v.unwrap());
            }
            // Full-range RGB in, BT.709 limited-range YUV out (what players expect for HD video).
            vctx.VideoProcessorSetStreamColorSpace(&vp, 0, &D3D11_VIDEO_PROCESSOR_COLOR_SPACE { _bitfield: 0 });
            vctx.VideoProcessorSetOutputColorSpace(&vp, &D3D11_VIDEO_PROCESSOR_COLOR_SPACE { _bitfield: (1 << 2) | (1 << 4) });
            vctx.VideoProcessorSetStreamFrameFormat(&vp, 0, D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE);
            vctx.VideoProcessorSetStreamAutoProcessingMode(&vp, 0, false);
            // Letterbox if the game's aspect ratio differs from the recording's.
            let scale = (out_w as f64 / in_w as f64).min(out_h as f64 / in_h as f64);
            let (dw, dh) = ((in_w as f64 * scale) as i32 & !1, (in_h as f64 * scale) as i32 & !1);
            let (dx, dy) = (((out_w as i32 - dw) / 2) & !1, ((out_h as i32 - dh) / 2) & !1);
            let src = RECT { left: 0, top: 0, right: in_w as i32, bottom: in_h as i32 };
            let dst = RECT { left: dx, top: dy, right: dx + dw, bottom: dy + dh };
            vctx.VideoProcessorSetStreamSourceRect(&vp, 0, true, Some(&src));
            vctx.VideoProcessorSetStreamDestRect(&vp, 0, true, Some(&dst));
            let black = D3D11_VIDEO_COLOR { Anonymous: D3D11_VIDEO_COLOR_0 { YCbCr: D3D11_VIDEO_COLOR_YCbCrA { Y: 0.0625, Cb: 0.5, Cr: 0.5, A: 1.0 } } };
            vctx.VideoProcessorSetOutputBackgroundColor(&vp, true, &black);
            Ok(Converter { vp, vctx, in_view: in_view.unwrap(), out_views, in_w, in_h })
        }
    }

    unsafe fn convert(&self, out_index: usize) -> Result<()> {
        unsafe {
            let stream = D3D11_VIDEO_PROCESSOR_STREAM {
                Enable: true.into(),
                pInputSurface: std::mem::ManuallyDrop::new(Some(self.in_view.clone())),
                ..Default::default()
            };
            let streams = [stream];
            let r = self.vctx.VideoProcessorBlt(&self.vp, &self.out_views[out_index], 0, &streams);
            let [mut s] = streams;
            std::mem::ManuallyDrop::drop(&mut s.pInputSurface);
            Ok(r?)
        }
    }
}

pub struct Screenshot {
    pub path: PathBuf,
    pub width: u32,
    pub reply: Sender<Result<()>>,
}

struct State {
    gpu: Arc<Gpu>,
    d3d: IDirect3DDevice,
    pool_size: SizeInt32,
    copy: Option<(ID3D11Texture2D, u32, u32)>,
    conv: Option<Converter>,
    nv12: Vec<ID3D11Texture2D>,
    next: usize,
    out_w: u32,
    out_h: u32,
    fps: u32,
    interval: i64,
    /// When the next frame is due. Frames are picked on a fixed grid, so a 141 fps game
    /// gives 60 fps (not the 47 fps that "at least one interval since the last frame" gives).
    next_due: i64,
    rec_start: i64,
    enc: SyncSender<EncIn>,
    in_flight: Arc<AtomicUsize>,
    screenshot: Option<Screenshot>,
}
unsafe impl Send for State {}

#[derive(Default)]
pub struct Stats {
    pub frames: AtomicU64,
    pub dropped: AtomicU64,
    pub last_frame_hns: AtomicU64,
}

pub struct Capture {
    session: GraphicsCaptureSession,
    pool: Direct3D11CaptureFramePool,
    state: Arc<Mutex<State>>,
    pub stats: Arc<Stats>,
    pub out_w: u32,
    pub out_h: u32,
}

impl Capture {
    pub fn request_screenshot(&self, s: Screenshot) {
        self.state.lock().unwrap().screenshot = Some(s);
    }

    pub fn stop(self) {
        let _ = self.session.Close();
        let _ = self.pool.Close();
    }
}

impl State {
    unsafe fn on_frame(&mut self, frame: &windows::Graphics::Capture::Direct3D11CaptureFrame, pool: &Direct3D11CaptureFramePool, stats: &Stats) -> Result<()> {
        unsafe {
            let size = frame.ContentSize()?;
            if size.Width != self.pool_size.Width || size.Height != self.pool_size.Height {
                // The game changed resolution: recreate the pool at the new size.
                self.pool_size = size;
                pool.Recreate(&self.d3d, DirectXPixelFormat::B8G8R8A8UIntNormalized, 2, size)?;
                return Ok(());
            }
            let now = frame.SystemRelativeTime()?.Duration;
            let pts = now - self.rec_start;
            // Cap to the recording frame rate (the game may run at 240 fps).
            if pts < self.next_due - 10_000 {
                return Ok(()); // 1 ms tolerance for timing jitter
            }
            let (w, h) = (size.Width.max(2) as u32, size.Height.max(2) as u32);
            let access: IDirect3DDxgiInterfaceAccess = frame.Surface()?.cast()?;
            let src: ID3D11Texture2D = access.GetInterface()?;
            if self.copy.as_ref().is_none_or(|c| c.1 != w || c.2 != h) {
                let t = self.gpu.texture(w, h, DXGI_FORMAT_B8G8R8A8_UNORM, D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE, false)?;
                self.conv = None;
                self.copy = Some((t, w, h));
            }
            let (copy, _, _) = self.copy.as_ref().unwrap();
            let bx = D3D11_BOX { left: 0, top: 0, front: 0, right: w, bottom: h, back: 1 };
            self.gpu.context.CopySubresourceRegion(copy, 0, 0, 0, 0, &src, 0, Some(&bx));
            drop(src);

            if let Some(s) = self.screenshot.take() {
                let r = super::snapshot::save_jpeg(&self.gpu, copy, w, h, &s.path, s.width);
                let _ = s.reply.send(r);
            }

            if self.in_flight.load(Ordering::SeqCst) >= MAX_IN_FLIGHT {
                stats.dropped.fetch_add(1, Ordering::Relaxed);
                return Ok(());
            }
            if self.conv.as_ref().is_none_or(|c| c.in_w != w || c.in_h != h) {
                self.conv = Some(Converter::new(&self.gpu, copy, w, h, &self.nv12, self.out_w, self.out_h, self.fps)?);
            }
            let idx = self.next;
            self.conv.as_ref().unwrap().convert(idx)?;
            self.gpu.context.Flush();
            match self.enc.try_send(EncIn::Frame { tex: self.nv12[idx].clone(), pts }) {
                Ok(()) => {
                    self.next = (self.next + 1) % RING;
                    // Stay on the grid; after a stall, restart it from this frame.
                    self.next_due = if pts - self.next_due > self.interval { pts + self.interval } else { self.next_due + self.interval };
                    stats.frames.fetch_add(1, Ordering::Relaxed);
                    stats.last_frame_hns.store(now as u64, Ordering::Relaxed);
                }
                Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {
                    stats.dropped.fetch_add(1, Ordering::Relaxed);
                }
            }
            Ok(())
        }
    }
}

pub struct CaptureParams {
    pub target: Target,
    /// Output height (0 = same as the game).
    pub height: u32,
    pub fps: u32,
    pub rec_start: i64,
    pub cursor: bool,
}

/// Size of what will be captured (to size the encoder before starting).
pub fn target_size(target: Target) -> Result<(u32, u32)> {
    let item = create_item(target)?;
    let s = item.Size()?;
    Ok((s.Width.max(2) as u32, s.Height.max(2) as u32))
}

pub fn start(gpu: Arc<Gpu>, p: CaptureParams, out_w: u32, out_h: u32, enc: SyncSender<EncIn>, in_flight: Arc<AtomicUsize>) -> Result<Capture> {
    unsafe {
        let item = create_item(p.target)?;
        let insp: IInspectable = CreateDirect3D11DeviceFromDXGIDevice(&gpu.dxgi()?)?;
        let d3d: IDirect3DDevice = insp.cast()?;
        let size = item.Size()?;
        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(&d3d, DirectXPixelFormat::B8G8R8A8UIntNormalized, 2, size)?;
        let session = pool.CreateCaptureSession(&item)?;
        // No yellow border and the real cursor (Windows 11); ignored where unsupported.
        if let Ok(op) = GraphicsCaptureAccess::RequestAccessAsync(GraphicsCaptureAccessKind::Borderless) {
            let _ = op.get();
        }
        let _ = session.SetIsBorderRequired(false);
        let _ = session.SetIsCursorCaptureEnabled(p.cursor);
        let interval = 10_000_000i64 / p.fps.max(1) as i64;
        // Let Windows deliver up to twice the recording rate; the grid above picks the frames.
        // (A limit of one full interval here would alias a 141 fps game down to ~47 fps.)
        let _ = session.SetMinUpdateInterval(TimeSpan { Duration: interval / 2 });

        let mut nv12 = Vec::new();
        for _ in 0..RING {
            nv12.push(gpu.texture(out_w, out_h, DXGI_FORMAT_NV12, D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE, false)?);
        }
        let state = Arc::new(Mutex::new(State {
            gpu,
            d3d,
            pool_size: size,
            copy: None,
            conv: None,
            nv12,
            next: 0,
            out_w,
            out_h,
            fps: p.fps,
            interval,
            next_due: i64::MIN / 2,
            rec_start: p.rec_start,
            enc,
            in_flight,
            screenshot: None,
        }));
        let stats = Arc::new(Stats::default());
        let (st, stt) = (state.clone(), stats.clone());
        pool.FrameArrived(&TypedEventHandler::<Direct3D11CaptureFramePool, IInspectable>::new(move |pool, _| {
            let Some(pool) = pool.as_ref() else { return Ok(()) };
            let frame = pool.TryGetNextFrame()?;
            let mut s = st.lock().unwrap();
            if let Err(e) = s.on_frame(&frame, pool, &stt) {
                log::debug!("frame: {e:#}");
            }
            let _ = frame.Close();
            Ok(())
        }))?;
        session.StartCapture()?;
        Ok(Capture { session, pool, state, stats, out_w, out_h })
    }
}
