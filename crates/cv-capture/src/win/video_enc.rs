//! Hardware H.264 encoding through Media Foundation. The GPU vendors ship their encoders as
//! Media Foundation transforms: NVIDIA NVENC, AMD AMF and Intel Quick Sync all appear here,
//! so one code path covers all three. CPU (software) encoders are never used.

use super::d3d::{vendor_name, Gpu};
use anyhow::{anyhow, bail, Context, Result};
use std::mem::ManuallyDrop;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, Sender, SyncSender};
use std::sync::Arc;
use windows::core::{Interface, GUID, PWSTR};
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{CoInitializeEx, CoTaskMemFree, COINIT_MULTITHREADED};
use windows::Win32::System::Variant::{VARIANT, VT_BOOL, VT_UI4};

pub enum EncIn {
    Frame { tex: ID3D11Texture2D, pts: i64 },
    Stop,
}

// Textures are only touched under the device's multithread protection.
unsafe impl Send for EncIn {}

pub struct EncodedFrame {
    pub pts: i64,
    pub annexb: Vec<u8>,
    pub key: bool,
}

#[derive(Debug, Clone)]
pub struct EncoderDesc {
    pub name: String,
    pub vendor: String,
}

pub struct VideoEncoder {
    pub tx: SyncSender<EncIn>,
    pub in_flight: Arc<AtomicUsize>,
    pub desc: EncoderDesc,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl VideoEncoder {
    /// Frames the encoder hasn't returned yet (used to avoid overwriting textures in use).
    pub fn in_flight(&self) -> usize {
        self.in_flight.load(Ordering::SeqCst)
    }

    /// Drains the last frames. Never blocks for more than a few seconds, even if the
    /// driver's encoder misbehaves.
    pub fn stop(mut self) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        loop {
            match self.tx.try_send(EncIn::Stop) {
                Ok(()) | Err(std::sync::mpsc::TrySendError::Disconnected(_)) => break,
                Err(std::sync::mpsc::TrySendError::Full(_)) if std::time::Instant::now() < deadline => std::thread::sleep(std::time::Duration::from_millis(10)),
                Err(_) => break,
            }
        }
        if let Some(t) = self.thread.take() {
            let (done_tx, done_rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let _ = t.join();
                let _ = done_tx.send(());
            });
            if done_rx.recv_timeout(std::time::Duration::from_secs(5)).is_err() {
                log::warn!("video encoder didn't finish draining; continuing");
            }
        }
    }
}

pub struct EncodeParams {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate: u32,
    /// "nvenc", "amd", "qsv" or "auto".
    pub prefer: String,
}

fn vendor_code(prefer: &str) -> Option<&'static str> {
    match prefer {
        "nvenc" => Some("VEN_10DE"),
        "amd" => Some("VEN_1002"),
        "qsv" => Some("VEN_8086"),
        _ => None,
    }
}

fn var_u32(v: u32) -> VARIANT {
    let mut var = VARIANT::default();
    unsafe {
        let inner = &mut *var.Anonymous.Anonymous;
        inner.vt = VT_UI4;
        inner.Anonymous.ulVal = v;
    }
    var
}

fn var_bool(v: bool) -> VARIANT {
    let mut var = VARIANT::default();
    unsafe {
        let inner = &mut *var.Anonymous.Anonymous;
        inner.vt = VT_BOOL;
        inner.Anonymous.boolVal = windows::Win32::Foundation::VARIANT_BOOL(if v { -1 } else { 0 });
    }
    var
}

unsafe fn get_string(a: &IMFAttributes, key: &GUID) -> String {
    let mut p = PWSTR::null();
    let mut len = 0u32;
    if unsafe { a.GetAllocatedString(key, &mut p, &mut len) }.is_ok() && !p.is_null() {
        let s = unsafe { p.to_string().unwrap_or_default() };
        unsafe { CoTaskMemFree(Some(p.0 as _)) };
        s
    } else {
        String::new()
    }
}

/// Hardware H.264 encoders, best match first: same GPU, then the preferred vendor.
pub unsafe fn list_encoders(gpu_luid: i64, gpu_vendor: u32, prefer: &str) -> Result<Vec<(IMFActivate, EncoderDesc)>> {
    let input = MFT_REGISTER_TYPE_INFO { guidMajorType: MFMediaType_Video, guidSubtype: MFVideoFormat_NV12 };
    let output = MFT_REGISTER_TYPE_INFO { guidMajorType: MFMediaType_Video, guidSubtype: MFVideoFormat_H264 };
    let mut ptr: *mut Option<IMFActivate> = std::ptr::null_mut();
    let mut count = 0u32;
    unsafe {
        MFTEnumEx(
            MFT_CATEGORY_VIDEO_ENCODER,
            MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_SORTANDFILTER,
            Some(&input),
            Some(&output),
            &mut ptr,
            &mut count,
        )?;
    }
    let mut out: Vec<(IMFActivate, EncoderDesc, i32)> = Vec::new();
    if !ptr.is_null() {
        let slice = unsafe { std::slice::from_raw_parts_mut(ptr, count as usize) };
        let want_vendor = vendor_code(prefer).map(str::to_string).unwrap_or_else(|| format!("VEN_{gpu_vendor:04X}"));
        for a in slice.iter_mut() {
            if let Some(act) = a.take() {
                let attrs: IMFAttributes = act.cast()?;
                let name = unsafe { get_string(&attrs, &MFT_FRIENDLY_NAME_Attribute) };
                let ven = unsafe { get_string(&attrs, &MFT_ENUM_HARDWARE_VENDOR_ID_Attribute) };
                let luid = unsafe { attrs.GetUINT64(&MFT_ENUM_ADAPTER_LUID) }.ok().map(|v| v as i64);
                let mut score = 0;
                if luid == Some(gpu_luid) {
                    score += 2;
                }
                if ven.eq_ignore_ascii_case(&want_vendor) {
                    score += 1;
                }
                let vid = u32::from_str_radix(ven.trim_start_matches("VEN_"), 16).unwrap_or(0);
                out.push((act, EncoderDesc { name, vendor: vendor_name(vid).to_string() }, score));
            }
        }
        unsafe { CoTaskMemFree(Some(ptr as _)) };
    }
    out.sort_by(|a, b| b.2.cmp(&a.2));
    Ok(out.into_iter().map(|(a, d, _)| (a, d)).collect())
}

unsafe fn codec_settings(mft: &IMFTransform, p: &EncodeParams) {
    if let Ok(codec) = mft.cast::<ICodecAPI>() {
        unsafe {
            let _ = codec.SetValue(&CODECAPI_AVEncCommonRateControlMode, &var_u32(eAVEncCommonRateControlMode_UnconstrainedVBR.0 as u32));
            let _ = codec.SetValue(&CODECAPI_AVEncCommonMeanBitRate, &var_u32(p.bitrate));
            // A keyframe every second: seeking (timeline markers) only decodes <1 s of video.
            let _ = codec.SetValue(&CODECAPI_AVEncMPVGOPSize, &var_u32(p.fps.max(1)));
            let _ = codec.SetValue(&CODECAPI_AVEncMPVDefaultBPictureCount, &var_u32(0));
            let _ = codec.SetValue(&CODECAPI_AVLowLatencyMode, &var_bool(true));
        }
    }
}

unsafe fn configure(mft: &IMFTransform, gpu: &Gpu, p: &EncodeParams) -> Result<IMFDXGIDeviceManager> {
    unsafe {
        let attrs = mft.GetAttributes()?;
        if attrs.GetUINT32(&MF_TRANSFORM_ASYNC).unwrap_or(0) != 1 {
            bail!("encoder isn't asynchronous (not a hardware encoder)");
        }
        attrs.SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1)?;
        let _ = attrs.SetUINT32(&MF_LOW_LATENCY, 1);

        let mut token = 0u32;
        let mut mgr = None;
        MFCreateDXGIDeviceManager(&mut token, &mut mgr)?;
        let mgr = mgr.context("no DXGI device manager")?;
        mgr.ResetDevice(&gpu.device, token)?;
        mft.ProcessMessage(MFT_MESSAGE_SET_D3D_MANAGER, mgr.as_raw() as usize).context("encoder refused the GPU device")?;
        // Some encoders only accept rate-control settings before the formats are set,
        // others only after: set them both times (errors are harmless).
        codec_settings(mft, p);

        let size = ((p.width as u64) << 32) | p.height as u64;
        let rate = ((p.fps as u64) << 32) | 1;
        let ot = MFCreateMediaType()?;
        ot.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        ot.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)?;
        ot.SetUINT32(&MF_MT_AVG_BITRATE, p.bitrate)?;
        ot.SetUINT64(&MF_MT_FRAME_SIZE, size)?;
        ot.SetUINT64(&MF_MT_FRAME_RATE, rate)?;
        ot.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, (1u64 << 32) | 1)?;
        ot.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
        ot.SetUINT32(&MF_MT_MPEG2_PROFILE, eAVEncH264VProfile_High.0 as u32)?;
        mft.SetOutputType(0, &ot, 0).context("encoder rejected the H.264 output format")?;

        let mut set = false;
        for i in 0..32 {
            let Ok(t) = mft.GetInputAvailableType(0, i) else { break };
            if t.GetGUID(&MF_MT_SUBTYPE).ok() == Some(MFVideoFormat_NV12) {
                t.SetUINT64(&MF_MT_FRAME_SIZE, size)?;
                t.SetUINT64(&MF_MT_FRAME_RATE, rate)?;
                mft.SetInputType(0, &t, 0).context("encoder rejected NV12 input")?;
                set = true;
                break;
            }
        }
        if !set {
            bail!("encoder doesn't accept NV12 input");
        }

        codec_settings(mft, p);
        mft.ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0)?;
        mft.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)?;
        mft.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)?;
        Ok(mgr)
    }
}

unsafe fn read_sample(s: &IMFSample) -> Result<(i64, Vec<u8>, bool)> {
    unsafe {
        let pts = s.GetSampleTime().unwrap_or(0);
        let key = s.GetUINT32(&MFSampleExtension_CleanPoint).unwrap_or(0) == 1;
        let b = s.ConvertToContiguousBuffer()?;
        let mut ptr = std::ptr::null_mut();
        let mut len = 0u32;
        b.Lock(&mut ptr, None, Some(&mut len))?;
        let data = std::slice::from_raw_parts(ptr, len as usize).to_vec();
        b.Unlock()?;
        Ok((pts, data, key))
    }
}

/// SPS/PPS from the current output format (Annex B), if the encoder publishes them there.
unsafe fn sequence_header(mft: &IMFTransform) -> Option<Vec<u8>> {
    unsafe {
        let t = mft.GetOutputCurrentType(0).ok()?;
        let len = t.GetBlobSize(&MF_MT_MPEG_SEQUENCE_HEADER).ok()?;
        if len == 0 {
            return None;
        }
        let mut buf = vec![0u8; len as usize];
        t.GetBlob(&MF_MT_MPEG_SEQUENCE_HEADER, &mut buf, None).ok()?;
        Some(buf)
    }
}

unsafe fn pull_output(mft: &IMFTransform, provides: bool, out_size: u32, tx: &Sender<super::mux::MuxMsg>, in_flight: &AtomicUsize, fed: &mut std::collections::VecDeque<i64>, last_key: &mut i64) -> Result<()> {
    unsafe {
        let own = if provides {
            None
        } else {
            let s = MFCreateSample()?;
            s.AddBuffer(&MFCreateMemoryBuffer(out_size.max(4 * 1024 * 1024))?)?;
            Some(s)
        };
        let mut buf = [MFT_OUTPUT_DATA_BUFFER { dwStreamID: 0, pSample: ManuallyDrop::new(own), dwStatus: 0, pEvents: ManuallyDrop::new(None) }];
        let mut status = 0u32;
        let r = mft.ProcessOutput(0, &mut buf, &mut status);
        let sample = ManuallyDrop::take(&mut buf[0].pSample);
        drop(ManuallyDrop::take(&mut buf[0].pEvents));
        match r {
            Ok(()) => {
                if let Some(s) = sample {
                    let (pts, mut data, key) = read_sample(&s)?;
                    // Some encoders only put SPS/PPS in the output format, not in the stream:
                    // add them in front of keyframes so the file can start.
                    let has_sps = crate::mp4::split_annexb(&data).iter().any(|n| n.first().is_some_and(|b| b & 0x1f == 7));
                    if !has_sps && (key || fed.len() <= 1) {
                        if let Some(h) = sequence_header(mft) {
                            let mut v = h;
                            v.extend_from_slice(&data);
                            data = v;
                        }
                    }
                    // Every input up to this timestamp is finished with its texture.
                    while fed.front().is_some_and(|t| *t <= pts) {
                        fed.pop_front();
                    }
                    if fed.len() > 8 {
                        fed.pop_front();
                    }
                    in_flight.store(fed.len(), Ordering::SeqCst);
                    if key {
                        *last_key = (*last_key).max(pts);
                    }
                    let _ = tx.send(super::mux::MuxMsg::Video(EncodedFrame { pts, annexb: data, key }));
                }
                Ok(())
            }
            Err(e) if e.code() == MF_E_TRANSFORM_STREAM_CHANGE => {
                let t = mft.GetOutputAvailableType(0, 0)?;
                mft.SetOutputType(0, &t, 0)?;
                Ok(())
            }
            Err(e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => Ok(()),
            Err(e) => Err(anyhow!("encoder output failed: {e}")),
        }
    }
}

/// Longest time between keyframes (100 ns units).
const KEYFRAME_EVERY: i64 = 10_000_000;

fn run(mft: IMFTransform, _mgr: IMFDXGIDeviceManager, fps: u32, rx: Receiver<EncIn>, tx: Sender<super::mux::MuxMsg>, in_flight: Arc<AtomicUsize>) {
    unsafe {
        let info = mft.GetOutputStreamInfo(0).unwrap_or_default();
        let provides = info.dwFlags & (MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32 | MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES.0 as u32) != 0;
        let Ok(gen) = mft.cast::<IMFMediaEventGenerator>() else { return };
        let dur = 10_000_000i64 / fps.max(1) as i64;
        let mut draining = false;
        let mut fed: std::collections::VecDeque<i64> = std::collections::VecDeque::new();
        // Keyframes at least every second of *time*: the GOP is counted in frames, and games don't
        // always deliver the full frame rate (menus, loading, heavy fights), which stretched the
        // gap between keyframes to 5 s in real recordings. Seeking decodes from the keyframe
        // before the target, so this keeps every jump short.
        let codec = mft.cast::<ICodecAPI>().ok();
        let mut last_key = i64::MIN / 2;
        loop {
            let ev = match gen.GetEvent(MF_EVENT_FLAG_NONE) {
                Ok(e) => e,
                Err(e) => {
                    log::warn!("encoder event: {e}");
                    break;
                }
            };
            let ty = ev.GetType().unwrap_or(0) as i32;
            if ty == METransformNeedInput.0 {
                if draining {
                    continue;
                }
                match rx.recv() {
                    Ok(EncIn::Frame { tex, pts }) => {
                        let r = (|| -> Result<()> {
                            let buf = MFCreateDXGISurfaceBuffer(&ID3D11Texture2D::IID, &tex, 0, false)?;
                            if let Ok(b2) = buf.cast::<IMF2DBuffer>() {
                                buf.SetCurrentLength(b2.GetContiguousLength()?)?;
                            }
                            let s = MFCreateSample()?;
                            s.AddBuffer(&buf)?;
                            s.SetSampleTime(pts)?;
                            s.SetSampleDuration(dur)?;
                            if pts - last_key >= KEYFRAME_EVERY - dur / 2 {
                                if let Some(c) = &codec {
                                    let _ = c.SetValue(&CODECAPI_AVEncVideoForceKeyFrame, &var_u32(1));
                                }
                                last_key = pts;
                            }
                            mft.ProcessInput(0, &s, 0)?;
                            fed.push_back(pts);
                            in_flight.store(fed.len(), Ordering::SeqCst);
                            Ok(())
                        })();
                        if let Err(e) = r {
                            log::warn!("encoder input: {e:#}");
                        }
                    }
                    Ok(EncIn::Stop) | Err(_) => {
                        draining = true;
                        let _ = mft.ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0);
                        if mft.ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0).is_err() {
                            break;
                        }
                    }
                }
            } else if ty == METransformHaveOutput.0 {
                if let Err(e) = pull_output(&mft, provides, info.cbSize, &tx, &in_flight, &mut fed, &mut last_key) {
                    log::warn!("{e:#}");
                }
            } else if ty == METransformDrainComplete.0 {
                break;
            }
        }
        let _ = mft.ProcessMessage(MFT_MESSAGE_NOTIFY_END_STREAMING, 0);
        let _ = mft.ProcessMessage(MFT_MESSAGE_SET_D3D_MANAGER, 0);
    }
}

/// Starts the encoder thread. Fails if no hardware H.264 encoder works on this PC.
pub fn start(gpu: Arc<Gpu>, p: EncodeParams, out: Sender<super::mux::MuxMsg>) -> Result<VideoEncoder> {
    let (tx, rx) = sync_channel::<EncIn>(2);
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<EncoderDesc>>();
    let in_flight = Arc::new(AtomicUsize::new(0));
    let inf = in_flight.clone();
    let fps = p.fps;
    let thread = std::thread::Builder::new()
        .name("video-encoder".into())
        .spawn(move || unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let _ = MFStartup(MF_VERSION, MFSTARTUP_FULL);
            let list = match list_encoders(gpu.info.luid, gpu.info.vendor_id, &p.prefer) {
                Ok(l) => l,
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                    return;
                }
            };
            let mut last_err = anyhow!("no hardware H.264 encoder found (update your graphics driver)");
            for (act, desc) in list {
                let mft: IMFTransform = match act.ActivateObject() {
                    Ok(m) => m,
                    Err(e) => {
                        last_err = anyhow!("{}: {e}", desc.name);
                        continue;
                    }
                };
                match configure(&mft, &gpu, &p) {
                    Ok(mgr) => {
                        log::info!("video encoder: {} ({})", desc.name, desc.vendor);
                        let _ = ready_tx.send(Ok(desc));
                        run(mft, mgr, fps, rx, out, inf);
                        let _ = act.ShutdownObject();
                        return;
                    }
                    Err(e) => {
                        log::warn!("encoder {} unusable: {e:#}", desc.name);
                        last_err = e.context(desc.name.clone());
                        let _ = act.ShutdownObject();
                    }
                }
            }
            let _ = ready_tx.send(Err(last_err));
        })?;
    let desc = ready_rx.recv().map_err(|_| anyhow!("encoder thread died"))??;
    Ok(VideoEncoder { tx, in_flight, desc, thread: Some(thread) })
}
