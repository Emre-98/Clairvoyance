//! Reads small regions of frames from a finished recording, for post-game analysis (the ult
//! check). Media Foundation decodes on the GPU (DXVA: NVDEC / VCN / Quick Sync) into Direct3D
//! textures; only the requested region is copied back to the CPU and converted to RGB.
//!
//! Runs only in the maintenance pass after a game, on a background-priority thread, with the GPU
//! work at the lowest GPU thread priority. Falls back to Media Foundation's software decoder when
//! the hardware path can't be set up.

use crate::nv12;
use anyhow::{Context, Result};
use cv_core::game::{FrameSource, Region, Rgb};
use std::mem::ManuallyDrop;
use std::path::Path;
use windows::core::{Interface, HSTRING};
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_11_1};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_NV12, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
use windows::Win32::System::Variant::VT_I8;

const FIRST_VIDEO: u32 = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
const HNS: f64 = 10_000_000.0;

struct Gpu {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    _manager: IMFDXGIDeviceManager,
    staging: Option<(u32, u32, ID3D11Texture2D)>,
}

pub struct VideoFrames {
    reader: IMFSourceReader,
    gpu: Option<Gpu>,
    size: (u32, u32),
    duration: f64,
    keyframes: Vec<f64>,
    /// Decoder output plane height (1080p decodes as 1088 lines) for the software path.
    plane_h: u32,
    /// Time of the last frame read (for continuing without a seek).
    last: Option<f64>,
    pub hardware: bool,
    pub frames_decoded: u64,
    pub seeks: u64,
}

unsafe impl Send for VideoFrames {}

fn create_gpu() -> Result<(ID3D11Device, ID3D11DeviceContext, IMFDXGIDeviceManager)> {
    let (adapter, _) = super::d3d::adapters()?.into_iter().next().context("no graphics card")?;
    unsafe {
        let mut device = None;
        let mut context = None;
        D3D11CreateDevice(
            &adapter,
            D3D_DRIVER_TYPE_UNKNOWN,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
            Some(&[D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0]),
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )?;
        let device: ID3D11Device = device.context("no device")?;
        let context = context.context("no context")?;
        let mt: ID3D11Multithread = device.cast()?;
        let _ = mt.SetMultithreadProtected(true);
        // The game always wins the GPU.
        if let Ok(dxgi) = device.cast::<IDXGIDevice>() {
            let _ = dxgi.SetGPUThreadPriority(-7);
        }
        let mut token = 0u32;
        let mut manager = None;
        MFCreateDXGIDeviceManager(&mut token, &mut manager)?;
        let manager = manager.context("no device manager")?;
        manager.ResetDevice(&device, token)?;
        Ok((device, context, manager))
    }
}

unsafe fn create_reader(video: &Path, manager: Option<&IMFDXGIDeviceManager>) -> Result<IMFSourceReader> {
    unsafe {
        let mut attrs: Option<IMFAttributes> = None;
        MFCreateAttributes(&mut attrs, 4)?;
        let attrs = attrs.context("no attributes")?;
        if let Some(m) = manager {
            attrs.SetUnknown(&MF_SOURCE_READER_D3D_MANAGER, m)?;
            attrs.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1)?;
        }
        // A frame comes out as soon as it is decoded (no reorder buffering after each seek).
        attrs.SetUINT32(&MF_LOW_LATENCY, 1)?;
        let reader = MFCreateSourceReaderFromURL(&HSTRING::from(video.as_os_str()), &attrs)?;
        reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false)?;
        reader.SetStreamSelection(FIRST_VIDEO, true)?;
        let mt = MFCreateMediaType()?;
        mt.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        mt.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)?;
        reader.SetCurrentMediaType(FIRST_VIDEO, None, &mt)?;
        Ok(reader)
    }
}

impl VideoFrames {
    pub fn open(video: &Path) -> Result<VideoFrames> {
        let info = crate::remux::info(video).with_context(|| format!("reading {}", video.display()))?;
        let keyframes = crate::remux::keyframes(video)?;
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            MFStartup(MF_VERSION, MFSTARTUP_FULL)?;
            let hw = create_gpu().and_then(|(device, context, manager)| {
                let reader = create_reader(video, Some(&manager))?;
                Ok((reader, Gpu { device, context, _manager: manager, staging: None }))
            });
            let (reader, gpu) = match hw {
                Ok((r, g)) => (r, Some(g)),
                Err(e) => {
                    log::info!("hardware decoding unavailable ({e:#}); using the software decoder");
                    (create_reader(video, None)?, None)
                }
            };
            let mut f = VideoFrames {
                reader,
                hardware: gpu.is_some(),
                gpu,
                size: (info.width, info.height),
                duration: info.duration_secs,
                keyframes,
                plane_h: info.height,
                last: None,
                frames_decoded: 0,
                seeks: 0,
            };
            f.read_plane_height();
            Ok(f)
        }
    }

    fn read_plane_height(&mut self) {
        unsafe {
            if let Ok(cur) = self.reader.GetCurrentMediaType(FIRST_VIDEO) {
                if let Ok(size) = cur.GetUINT64(&MF_MT_FRAME_SIZE) {
                    self.plane_h = (size & 0xFFFF_FFFF) as u32;
                }
            }
        }
    }

    fn seek(&mut self, t: f64) -> Result<()> {
        self.seeks += 1;
        self.last = None;
        unsafe {
            let mut pv = ManuallyDrop::new(PROPVARIANT::default());
            {
                let inner = &mut *pv.Anonymous.Anonymous;
                inner.vt = VT_I8;
                inner.Anonymous.hVal = (t.max(0.0) * HNS) as i64;
            }
            self.reader.SetCurrentPosition(&windows::core::GUID::zeroed(), &*pv)?;
        }
        Ok(())
    }

    /// The next decoded frame (time, sample), or None at the end.
    fn next(&mut self) -> Result<Option<(f64, IMFSample)>> {
        unsafe {
            for _ in 0..1000 {
                let mut flags = 0u32;
                let mut s: Option<IMFSample> = None;
                self.reader.ReadSample(FIRST_VIDEO, 0, None, Some(&mut flags), None, Some(&mut s))?;
                if flags & MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED.0 as u32 != 0 {
                    self.read_plane_height();
                }
                if let Some(s) = s {
                    let t = s.GetSampleTime()? as f64 / HNS;
                    self.frames_decoded += 1;
                    self.last = Some(t);
                    return Ok(Some((t, s)));
                }
                if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                    return Ok(None);
                }
            }
            Ok(None)
        }
    }

    /// Copies `r` out of a decoded frame and converts it to RGB.
    fn region(&mut self, s: &IMFSample, r: Region) -> Result<Rgb> {
        let (w, h) = self.size;
        let b = nv12::aligned_box(r, w, h);
        unsafe {
            let buf = s.GetBufferByIndex(0)?;
            if let (Some(gpu), Ok(dx)) = (self.gpu.as_mut(), buf.cast::<IMFDXGIBuffer>()) {
                let mut p: *mut core::ffi::c_void = std::ptr::null_mut();
                dx.GetResource(&ID3D11Texture2D::IID, &mut p)?;
                let tex = ID3D11Texture2D::from_raw(p);
                let sub = dx.GetSubresourceIndex()?;
                let staging = match &gpu.staging {
                    Some((sw, sh, t)) if *sw == b.w && *sh == b.h => t.clone(),
                    _ => {
                        let desc = D3D11_TEXTURE2D_DESC {
                            Width: b.w,
                            Height: b.h,
                            MipLevels: 1,
                            ArraySize: 1,
                            Format: DXGI_FORMAT_NV12,
                            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                            Usage: D3D11_USAGE_STAGING,
                            BindFlags: 0,
                            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                            MiscFlags: 0,
                        };
                        let mut t = None;
                        gpu.device.CreateTexture2D(&desc, None, Some(&mut t))?;
                        let t = t.context("no staging texture")?;
                        gpu.staging = Some((b.w, b.h, t.clone()));
                        t
                    }
                };
                let bx = D3D11_BOX { left: b.x, top: b.y, front: 0, right: b.x + b.w, bottom: b.y + b.h, back: 1 };
                gpu.context.CopySubresourceRegion(&staging, 0, 0, 0, 0, &tex, sub, Some(&bx));
                let mut m = D3D11_MAPPED_SUBRESOURCE::default();
                gpu.context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut m))?;
                let pitch = m.RowPitch as usize;
                let base = m.pData as *const u8;
                let y = std::slice::from_raw_parts(base, pitch * b.h as usize);
                let uv = std::slice::from_raw_parts(base.add(pitch * b.h as usize), pitch * (b.h as usize / 2));
                let out = nv12::to_rgb(y, pitch, uv, pitch, b, r);
                gpu.context.Unmap(&staging, 0);
                return Ok(out);
            }
            // Software decoder: NV12 in system memory.
            let b2: IMF2DBuffer = buf.cast().context("decoded frame isn't a 2D buffer")?;
            let mut scan0: *mut u8 = std::ptr::null_mut();
            let mut pitch: i32 = 0;
            b2.Lock2D(&mut scan0, &mut pitch)?;
            let pitch_u = pitch.unsigned_abs() as usize;
            let y = std::slice::from_raw_parts(scan0.add(b.y as usize * pitch_u), pitch_u * b.h as usize);
            let uv0 = scan0.add(pitch_u * self.plane_h as usize + (b.y as usize / 2) * pitch_u);
            let uv = std::slice::from_raw_parts(uv0, pitch_u * (b.h as usize / 2));
            // Rows start at the box's x: shift by passing a box at x 0 of the full-width rows.
            let full_rows = Region { x: 0, y: b.y, w: b.x + b.w, h: b.h };
            let out = nv12::to_rgb(y, pitch_u, uv, pitch_u, full_rows, r);
            let _ = b2.Unlock2D();
            Ok(out)
        }
    }
}

impl Drop for VideoFrames {
    fn drop(&mut self) {
        unsafe {
            let _ = MFShutdown();
        }
    }
}

impl FrameSource for VideoFrames {
    fn size(&self) -> (u32, u32) {
        self.size
    }
    fn duration(&self) -> f64 {
        self.duration
    }
    fn keyframes(&self) -> &[f64] {
        &self.keyframes
    }

    /// The frame at `t` (meant for keyframe times: a seek lands on them directly).
    fn frame_at(&mut self, t: f64, region: Region) -> Result<Option<(f64, Rgb)>> {
        self.seek(t)?;
        while let Some((pt, s)) = self.next()? {
            if pt >= t - 0.25 {
                let img = self.region(&s, region)?;
                return Ok(Some((pt, img)));
            }
        }
        Ok(None)
    }

    fn frames(&mut self, t0: f64, t1: f64, region: Region, f: &mut dyn FnMut(f64, &Rgb) -> bool) -> Result<()> {
        // Keep decoding forward when we're just before t0; otherwise seek (to the keyframe
        // before t0) and decode from there.
        let near = self.last.is_some_and(|l| l <= t0 && t0 - l < 1.0);
        if !near {
            self.seek(t0)?;
        }
        while let Some((pt, s)) = self.next()? {
            if pt > t1 {
                break;
            }
            if pt >= t0 - 0.001 {
                let img = self.region(&s, region)?;
                if !f(pt, &img) {
                    break;
                }
            }
        }
        Ok(())
    }
}
