//! Audio capture (WASAPI) and AAC encoding (Media Foundation, tiny CPU cost).
//!
//! - Game audio: per-process loopback (Windows 10 build 20348+ / Windows 11). Only the game's
//!   sound is captured, not Discord or Spotify. Older Windows falls back to all desktop audio.
//! - Microphone: the default communications microphone, as its own track.
//!
//! Timing: audio is anchored to the QPC clock the video uses. When the game is silent WASAPI
//! delivers nothing, so gaps are filled with silence; small clock drift is corrected by
//! dropping or padding a few milliseconds.

use super::d3d::qpc_hns;
use crate::mp4::Packet;
use anyhow::{anyhow, bail, Context, Result};
use std::mem::ManuallyDrop;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use windows::core::{implement, Interface, HRESULT};
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Media::Audio::*;
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, BLOB, CLSCTX_ALL, COINIT_MULTITHREADED};
use windows::Win32::System::Threading::{CreateEventW, SetEvent, WaitForSingleObject};
use windows::Win32::System::Variant::VT_BLOB;

pub const RATE: u32 = 48_000;
pub const CHANNELS: u16 = 2;
const BYTES_PER_FRAME: usize = 4; // 16-bit stereo

#[derive(Debug, Clone)]
pub enum Source {
    /// Only these processes (and their children), e.g. the game.
    Process(u32),
    /// Everything you hear (fallback).
    Desktop,
    Microphone,
}

pub struct AudioCapture {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    pub description: String,
}

impl AudioCapture {
    pub fn stop(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn format() -> WAVEFORMATEX {
    WAVEFORMATEX {
        wFormatTag: 1, // WAVE_FORMAT_PCM
        nChannels: CHANNELS,
        nSamplesPerSec: RATE,
        nAvgBytesPerSec: RATE * BYTES_PER_FRAME as u32,
        nBlockAlign: BYTES_PER_FRAME as u16,
        wBitsPerSample: 16,
        cbSize: 0,
    }
}

#[implement(IActivateAudioInterfaceCompletionHandler, windows::Win32::System::Com::IAgileObject)]
struct Activated(HANDLE);

impl IActivateAudioInterfaceCompletionHandler_Impl for Activated_Impl {
    fn ActivateCompleted(&self, _op: windows::core::Ref<'_, IActivateAudioInterfaceAsyncOperation>) -> windows::core::Result<()> {
        unsafe { SetEvent(self.0) }
    }
}
impl windows::Win32::System::Com::IAgileObject_Impl for Activated_Impl {}

unsafe fn process_loopback_client(pid: u32) -> Result<IAudioClient> {
    unsafe {
        let params = AUDIOCLIENT_ACTIVATION_PARAMS {
            ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
            Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
                ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
                    TargetProcessId: pid,
                    ProcessLoopbackMode: PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE,
                },
            },
        };
        // The PROPVARIANT only *borrows* `params` (on our stack). It must never be dropped:
        // PROPVARIANT's Drop calls PropVariantClear, which would CoTaskMemFree the stack
        // pointer and corrupt the heap (this crashed the whole app on the first real test).
        let mut pv = ManuallyDrop::new(PROPVARIANT::default());
        {
            let inner = &mut *pv.Anonymous.Anonymous;
            inner.vt = VT_BLOB;
            inner.Anonymous.blob = BLOB { cbSize: std::mem::size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32, pBlobData: &params as *const _ as *mut u8 };
        }
        let event = CreateEventW(None, false, false, None)?;
        let handler: IActivateAudioInterfaceCompletionHandler = Activated(event).into();
        let op = ActivateAudioInterfaceAsync(VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK, &IAudioClient::IID, Some(&*pv), &handler);
        let op = match op {
            Ok(o) => o,
            Err(e) => {
                let _ = CloseHandle(event);
                return Err(anyhow!("process audio capture isn't available: {e}"));
            }
        };
        let w = WaitForSingleObject(event, 5000);
        let _ = CloseHandle(event);
        if w != WAIT_OBJECT_0 {
            bail!("process audio capture didn't start");
        }
        let mut hr = HRESULT(0);
        let mut unk = None;
        op.GetActivateResult(&mut hr, &mut unk)?;
        hr.ok()?;
        Ok(unk.context("no audio client")?.cast()?)
    }
}

unsafe fn endpoint_client(flow: EDataFlow, role: ERole) -> Result<IAudioClient> {
    unsafe {
        let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let dev = en.GetDefaultAudioEndpoint(flow, role)?;
        Ok(dev.Activate(CLSCTX_ALL, None)?)
    }
}

// ---------- AAC encoder ----------

pub struct Aac {
    mft: IMFTransform,
    out_size: u32,
}

impl Aac {
    pub unsafe fn new(bitrate_bytes: u32) -> Result<Aac> {
        unsafe {
            let input = MFT_REGISTER_TYPE_INFO { guidMajorType: MFMediaType_Audio, guidSubtype: MFAudioFormat_PCM };
            let output = MFT_REGISTER_TYPE_INFO { guidMajorType: MFMediaType_Audio, guidSubtype: MFAudioFormat_AAC };
            let mut ptr: *mut Option<IMFActivate> = std::ptr::null_mut();
            let mut count = 0u32;
            MFTEnumEx(MFT_CATEGORY_AUDIO_ENCODER, MFT_ENUM_FLAG_SYNCMFT | MFT_ENUM_FLAG_SORTANDFILTER, Some(&input), Some(&output), &mut ptr, &mut count)?;
            if ptr.is_null() || count == 0 {
                bail!("no AAC encoder on this PC");
            }
            let slice = std::slice::from_raw_parts_mut(ptr, count as usize);
            let act = slice[0].take().context("no AAC encoder")?;
            for a in slice.iter_mut() {
                drop(a.take());
            }
            CoTaskMemFree(Some(ptr as _));
            let mft: IMFTransform = act.ActivateObject()?;

            let it = MFCreateMediaType()?;
            it.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
            it.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM)?;
            it.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
            it.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, RATE)?;
            it.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, CHANNELS as u32)?;
            it.SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, BYTES_PER_FRAME as u32)?;
            it.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, RATE * BYTES_PER_FRAME as u32)?;
            let ot = MFCreateMediaType()?;
            ot.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
            ot.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_AAC)?;
            ot.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
            ot.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, RATE)?;
            ot.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, CHANNELS as u32)?;
            ot.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, bitrate_bytes)?;
            ot.SetUINT32(&MF_MT_AAC_PAYLOAD_TYPE, 0)?; // raw AAC frames
            if mft.SetInputType(0, &it, 0).is_ok() {
                mft.SetOutputType(0, &ot, 0).context("AAC output format")?;
            } else {
                mft.SetOutputType(0, &ot, 0).context("AAC output format")?;
                mft.SetInputType(0, &it, 0).context("AAC input format")?;
            }
            let info = mft.GetOutputStreamInfo(0)?;
            mft.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)?;
            mft.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)?;
            Ok(Aac { mft, out_size: info.cbSize.max(8192) })
        }
    }

    /// Feeds PCM (16-bit stereo) with the time of its first sample; returns encoded frames.
    pub unsafe fn encode(&mut self, pcm: &[u8], pts: i64) -> Result<Vec<(i64, Vec<u8>)>> {
        unsafe {
            let buf = MFCreateMemoryBuffer(pcm.len() as u32)?;
            let mut p = std::ptr::null_mut();
            buf.Lock(&mut p, None, None)?;
            std::ptr::copy_nonoverlapping(pcm.as_ptr(), p, pcm.len());
            buf.Unlock()?;
            buf.SetCurrentLength(pcm.len() as u32)?;
            let s = MFCreateSample()?;
            s.AddBuffer(&buf)?;
            s.SetSampleTime(pts)?;
            s.SetSampleDuration((pcm.len() / BYTES_PER_FRAME) as i64 * 10_000_000 / RATE as i64)?;
            self.mft.ProcessInput(0, &s, 0)?;
            self.drain_output()
        }
    }

    unsafe fn drain_output(&mut self) -> Result<Vec<(i64, Vec<u8>)>> {
        let mut out = Vec::new();
        unsafe {
            loop {
                let s = MFCreateSample()?;
                s.AddBuffer(&MFCreateMemoryBuffer(self.out_size)?)?;
                let mut db = [MFT_OUTPUT_DATA_BUFFER { dwStreamID: 0, pSample: ManuallyDrop::new(Some(s)), dwStatus: 0, pEvents: ManuallyDrop::new(None) }];
                let mut status = 0;
                let r = self.mft.ProcessOutput(0, &mut db, &mut status);
                let sample = ManuallyDrop::take(&mut db[0].pSample);
                drop(ManuallyDrop::take(&mut db[0].pEvents));
                match r {
                    Ok(()) => {
                        if let Some(s) = sample {
                            let t = s.GetSampleTime().unwrap_or(0);
                            let b = s.ConvertToContiguousBuffer()?;
                            let mut p = std::ptr::null_mut();
                            let mut len = 0u32;
                            b.Lock(&mut p, None, Some(&mut len))?;
                            if len > 0 {
                                out.push((t, std::slice::from_raw_parts(p, len as usize).to_vec()));
                            }
                            b.Unlock()?;
                        }
                    }
                    Err(e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => break,
                    Err(e) => return Err(anyhow!("AAC encode: {e}")),
                }
            }
        }
        Ok(out)
    }

    pub unsafe fn flush(&mut self) -> Result<Vec<(i64, Vec<u8>)>> {
        unsafe {
            let _ = self.mft.ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0);
            let _ = self.mft.ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0);
            self.drain_output()
        }
    }
}

// ---------- capture thread ----------

struct Clock {
    rec_start: i64,
    t0: i64,       // timeline position of the first sample (100 ns)
    frames: i64,   // frames sent to the encoder so far
    out_frames: i64, // AAC frames received from the encoder
}

impl Clock {
    fn expected_frames(&self) -> i64 {
        (qpc_hns() - self.rec_start - self.t0) * RATE as i64 / 10_000_000
    }
    fn pts(&self) -> i64 {
        self.t0 + self.frames * 10_000_000 / RATE as i64
    }
}

/// Starts capturing `source` into audio track `track`. Returns a description of what's captured.
pub fn start(source: Source, track: usize, rec_start: i64, bitrate_bytes: u32, out: Sender<super::mux::MuxMsg>) -> Result<AudioCapture> {
    let stop = Arc::new(AtomicBool::new(false));
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<String>>();
    let st = stop.clone();
    let thread = std::thread::Builder::new().name(format!("audio-{track}")).spawn(move || unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let _ = MFStartup(MF_VERSION, MFSTARTUP_FULL);
        let (client, loopback, desc) = match &source {
            Source::Process(pid) => match process_loopback_client(*pid) {
                Ok(c) => (c, true, "game audio only".to_string()),
                Err(e) => {
                    log::warn!("{e:#}; falling back to desktop audio");
                    match endpoint_client(eRender, eConsole) {
                        Ok(c) => (c, true, "all desktop audio (this Windows version can't capture one app)".to_string()),
                        Err(e) => {
                            let _ = ready_tx.send(Err(e));
                            return;
                        }
                    }
                }
            },
            Source::Desktop => match endpoint_client(eRender, eConsole) {
                Ok(c) => (c, true, "all desktop audio".to_string()),
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                    return;
                }
            },
            Source::Microphone => match endpoint_client(eCapture, eCommunications) {
                Ok(c) => (c, false, "microphone".to_string()),
                Err(e) => {
                    let _ = ready_tx.send(Err(e.context("no microphone found")));
                    return;
                }
            },
        };
        let r = (|| -> Result<(IAudioCaptureClient, HANDLE)> {
            let mut flags = AUDCLNT_STREAMFLAGS_EVENTCALLBACK | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY;
            if loopback {
                flags |= AUDCLNT_STREAMFLAGS_LOOPBACK;
            }
            let fmt = format();
            client.Initialize(AUDCLNT_SHAREMODE_SHARED, flags, 200_000, 0, &fmt, None)?;
            let ev = CreateEventW(None, false, false, None)?;
            client.SetEventHandle(ev)?;
            let cap: IAudioCaptureClient = client.GetService()?;
            client.Start()?;
            Ok((cap, ev))
        })();
        let (cap, ev) = match r {
            Ok(x) => x,
            Err(e) => {
                let _ = ready_tx.send(Err(e.context("starting audio capture")));
                return;
            }
        };
        let mut aac = match Aac::new(bitrate_bytes) {
            Ok(a) => a,
            Err(e) => {
                let _ = ready_tx.send(Err(e));
                return;
            }
        };
        let _ = ready_tx.send(Ok(desc));

        let mut clock = Clock { rec_start, t0: qpc_hns() - rec_start, frames: 0, out_frames: 0 };
        let mut pcm: Vec<u8> = Vec::with_capacity(RATE as usize);
        let chunk = 1024 * BYTES_PER_FRAME * 4; // ~85 ms per encoder call
        let send = |aac: &mut Aac, clock: &mut Clock, pcm: &mut Vec<u8>, all: bool| {
            while pcm.len() >= chunk || (all && !pcm.is_empty()) {
                let n = if pcm.len() >= chunk { chunk } else { pcm.len() };
                let part: Vec<u8> = pcm.drain(..n).collect();
                let pts = clock.pts();
                clock.frames += (n / BYTES_PER_FRAME) as i64;
                match aac.encode(&part, pts) {
                    Ok(frames) => {
                        // Timestamps from our own count: each AAC frame is 1024 samples.
                        for (_, data) in frames {
                            let t = clock.t0 + clock.out_frames * 1024 * 10_000_000 / RATE as i64;
                            clock.out_frames += 1;
                            let _ = out.send(super::mux::MuxMsg::Packet(Packet { track, pts: t, data, key: true }));
                        }
                    }
                    Err(e) => log::warn!("{e:#}"),
                }
            }
        };
        while !st.load(Ordering::SeqCst) {
            let _ = WaitForSingleObject(ev, 50);
            loop {
                let n = match cap.GetNextPacketSize() {
                    Ok(n) => n,
                    Err(_) => 0,
                };
                if n == 0 {
                    break;
                }
                let mut data = std::ptr::null_mut();
                let mut frames = 0u32;
                let mut flags = 0u32;
                if cap.GetBuffer(&mut data, &mut frames, &mut flags, None, None).is_err() {
                    break;
                }
                let bytes = frames as usize * BYTES_PER_FRAME;
                if flags & (AUDCLNT_BUFFERFLAGS_SILENT.0 as u32) != 0 || data.is_null() {
                    pcm.resize(pcm.len() + bytes, 0);
                } else {
                    pcm.extend_from_slice(std::slice::from_raw_parts(data, bytes));
                }
                let _ = cap.ReleaseBuffer(frames);
            }
            // Keep audio on the video clock: pad silence when nothing arrives, trim if ahead.
            let have = clock.frames + (pcm.len() / BYTES_PER_FRAME) as i64;
            let expected = clock.expected_frames() - (RATE as i64 / 25); // allow 40 ms of buffering
            if have < expected {
                pcm.resize(pcm.len() + ((expected - have) as usize) * BYTES_PER_FRAME, 0);
            } else if have > expected + RATE as i64 / 5 && pcm.len() > RATE as usize / 100 * BYTES_PER_FRAME {
                let drop_bytes = (RATE as usize / 100) * BYTES_PER_FRAME; // drop 10 ms
                pcm.drain(..drop_bytes);
            }
            send(&mut aac, &mut clock, &mut pcm, false);
        }
        send(&mut aac, &mut clock, &mut pcm, true);
        if let Ok(frames) = aac.flush() {
            for (_, data) in frames {
                let t = clock.t0 + clock.out_frames * 1024 * 10_000_000 / RATE as i64;
                clock.out_frames += 1;
                let _ = out.send(super::mux::MuxMsg::Packet(Packet { track, pts: t, data, key: true }));
            }
        }
        let _ = client.Stop();
        let _ = CloseHandle(ev);
    })?;
    let description = ready_rx.recv().map_err(|_| anyhow!("audio thread died"))??;
    Ok(AudioCapture { stop, thread: Some(thread), description })
}
