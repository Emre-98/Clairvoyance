//! The Windows recorder pipeline:
//!
//! ```text
//! Windows Graphics Capture ─► copy + BGRA→NV12/scale (GPU) ─► hardware H.264 (MF: NVENC/AMF/QSV) ─┐
//! WASAPI process loopback (game only) ─► AAC ─────────────────────────────────────────────────────┼─► mux thread ─► fragmented MP4
//! WASAPI microphone (optional) ─► AAC ────────────────────────────────────────────────────────────┘        └─► replay buffer (RAM)
//! ```

pub mod audio;
pub mod capture;
pub mod clipboard;
pub mod d3d;
pub mod frames;
pub mod input;
pub mod mux;
pub mod perf;
pub mod thumb;
pub mod video_enc;
pub mod window;

use crate::mp4::AudioConfig;
use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use capture::{Capture, CaptureParams, Target};
use cv_core::game::CaptureTarget;
use cv_core::recorder::{RecordOptions, Recorder, RecorderStatus};
use d3d::{qpc_hns, Gpu};
use mux::{MuxMsg, MuxParams};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

/// Bitrate for the chosen quality, scaled from 1080p60 (12 / 20 Mbps).
pub fn bitrate(quality: &str, w: u32, h: u32, fps: u32) -> u32 {
    let base = if quality == "high" { 20_000_000.0 } else { 12_000_000.0 };
    let scale = (w as f64 * h as f64 * fps as f64) / (1920.0 * 1080.0 * 60.0);
    (base * scale.powf(0.75)).clamp(3_000_000.0, 60_000_000.0) as u32
}

pub use crate::encopts::{codec_factor, pick_codec, quality_value};

struct Active {
    rec_start: i64,
    capture: Arc<Mutex<Option<Capture>>>,
    encoder: Option<video_enc::VideoEncoder>,
    audio: Vec<audio::AudioCapture>,
    mux_tx: Sender<MuxMsg>,
    mux_thread: Option<std::thread::JoinHandle<()>>,
    output_dir: PathBuf,
    watchdog_stop: Arc<std::sync::atomic::AtomicBool>,
}

struct Inner {
    gpu: Mutex<Option<Arc<Gpu>>>,
    prepared: Mutex<Option<(CaptureTarget, RecordOptions)>>,
    active: Mutex<Option<Active>>,
    status: Mutex<RecorderStatus>,
}

pub struct NativeRecorder {
    inner: Arc<Inner>,
}

impl Default for NativeRecorder {
    fn default() -> Self {
        Self::new()
    }
}

fn com() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}

/// Writes panics and native crashes (access violations etc.) to the log before the process
/// dies, so a crash in the field leaves a trace. Call once at startup, after the logger.
pub fn install_crash_logging() {
    use windows::Win32::System::Diagnostics::Debug::{SetUnhandledExceptionFilter, EXCEPTION_POINTERS};
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let bt = std::backtrace::Backtrace::force_capture();
        let thread = std::thread::current().name().unwrap_or("?").to_string();
        log::error!("PANIC in thread '{thread}': {info}\n{bt}");
        log::logger().flush();
        default(info);
    }));
    unsafe extern "system" fn filter(p: *const EXCEPTION_POINTERS) -> i32 {
        unsafe {
            if let Some(rec) = p.as_ref().and_then(|p| p.ExceptionRecord.as_ref()) {
                let thread = std::thread::current().name().unwrap_or("?").to_string();
                log::error!(
                    "CRASH in thread '{thread}': exception 0x{:08X} at {:?} (info {:?})",
                    rec.ExceptionCode.0 as u32,
                    rec.ExceptionAddress,
                    &rec.ExceptionInformation[..(rec.NumberParameters as usize).min(4)]
                );
                log::logger().flush();
            }
        }
        0 // EXCEPTION_CONTINUE_SEARCH: let Windows end the process as usual
    }
    unsafe {
        SetUnhandledExceptionFilter(Some(filter));
    }
}

fn stamp() -> String {
    chrono::Local::now().format("%Y-%m-%d_%H-%M-%S").to_string()
}

impl NativeRecorder {
    pub fn new() -> Self {
        let status = RecorderStatus { version: Some("built-in".into()), ..Default::default() };
        Self { inner: Arc::new(Inner { gpu: Mutex::new(None), prepared: Mutex::new(None), active: Mutex::new(None), status: Mutex::new(status) }) }
    }

    /// The hardware encoders available (for Settings and the self-test).
    pub fn available_encoders() -> Result<(String, Vec<String>)> {
        com();
        let gpu = Gpu::create()?;
        unsafe {
            let _ = windows::Win32::Media::MediaFoundation::MFStartup(windows::Win32::Media::MediaFoundation::MF_VERSION, 0);
            let list = video_enc::list_encoders(gpu.info.luid, gpu.info.vendor_id, "auto")?;
            Ok((gpu.info.name.clone(), list.into_iter().map(|(_, d)| format!("{} ({})", d.name, d.vendor)).collect()))
        }
    }

    fn gpu(&self) -> Result<Arc<Gpu>> {
        let mut g = self.inner.gpu.lock().unwrap();
        if g.is_none() {
            com();
            *g = Some(Arc::new(Gpu::create()?));
        }
        Ok(g.as_ref().unwrap().clone())
    }

    fn set_status(&self, f: impl FnOnce(&mut RecorderStatus)) {
        f(&mut self.inner.status.lock().unwrap());
    }

    fn start_blocking(&self) -> Result<()> {
        com();
        let (target, opts) = self.inner.prepared.lock().unwrap().clone().context("recorder not prepared")?;
        if self.inner.active.lock().unwrap().is_some() {
            bail!("already recording");
        }
        let gpu = self.gpu()?;
        std::fs::create_dir_all(&opts.output_dir)?;

        // Which window? It may take a moment to appear after the game starts.
        let mut hwnd = None;
        if !(opts.display_capture || target.display_capture_only) {
            for _ in 0..16 {
                hwnd = window::find_window(&target.exe);
                if hwnd.is_some() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(500));
            }
        }
        let cap_target = match hwnd {
            Some(h) if !(opts.display_capture || target.display_capture_only) => Target::Window(h),
            _ => Target::Monitor(window::monitor_for(window::find_window(&target.exe))),
        };
        let (src_w, src_h) = capture::target_size(cap_target)?;
        let (out_w, out_h) = capture::output_size(src_w, src_h, opts.height);
        let fps = opts.fps.clamp(10, 240);
        let h264_rate = bitrate(&opts.quality, out_w, out_h, fps);
        let (codec, note) = pick_codec(&opts.codec, &opts.playable, video_enc::decoder_available);
        if let Some(n) = &note {
            log::warn!("recorder: {n}");
        }
        let rate = (h264_rate as f64 * codec_factor(codec)) as u32;
        let rc = if opts.rate_control == "quality" { video_enc::RateControl::Quality(quality_value(&opts.quality)) } else { video_enc::RateControl::Bitrate };

        log::info!(
            "recorder: capturing {} ({src_w}x{src_h}) -> {out_w}x{out_h}@{fps}",
            match cap_target {
                Target::Window(_) => "window",
                Target::Monitor(_) => "monitor",
            }
        );
        let (mux_tx, mux_rx) = channel::<MuxMsg>();
        let encoder = video_enc::start(
            gpu.clone(),
            video_enc::EncodeParams { width: out_w, height: out_h, fps, bitrate: rate, prefer: opts.encoder.clone(), codec, rate: rc, h264_bitrate: h264_rate },
            mux_tx.clone(),
        )?;
        let rec_start = qpc_hns();

        // Audio: the game only (its process tree), plus the mic if enabled.
        let mut audio_caps = Vec::new();
        let mut audio_cfgs: Vec<AudioConfig> = Vec::new();
        let game_pid = hwnd.map(window::window_pid).or_else(|| window::pids_for_exe(&target.exe).first().copied());
        let game_src = match game_pid {
            Some(pid) => audio::Source::Process(pid),
            None => audio::Source::Desktop,
        };
        let mut audio_desc = Vec::new();
        log::info!("recorder: starting audio ({game_src:?})");
        match audio::start(game_src, audio_cfgs.len() + 1, rec_start, 20_000, mux_tx.clone()) {
            Ok(a) => {
                audio_desc.push(a.description.clone());
                audio_cfgs.push(AudioConfig::aac_lc(audio::RATE, audio::CHANNELS, "Game audio"));
                audio_caps.push(a);
            }
            Err(e) => log::warn!("game audio not recorded: {e:#}"),
        }
        if opts.record_mic {
            match audio::start(audio::Source::Microphone, audio_cfgs.len() + 1, rec_start, 16_000, mux_tx.clone()) {
                Ok(a) => {
                    audio_desc.push(a.description.clone());
                    audio_cfgs.push(AudioConfig::aac_lc(audio::RATE, audio::CHANNELS, "Microphone"));
                    audio_caps.push(a);
                }
                Err(e) => log::warn!("microphone not recorded: {e:#}"),
            }
        }

        let path = opts.output_dir.join(format!("{}.mp4", stamp()));
        let mp = MuxParams {
            path,
            width: out_w,
            height: out_h,
            fps,
            audio: audio_cfgs,
            replay_secs: opts.replay_buffer_secs,
            // Quality-based mode can run above the average in fights: room for twice H.264's rate.
            replay_max_bytes: (if matches!(rc, video_enc::RateControl::Quality(_)) { h264_rate as usize * 2 } else { rate as usize } / 8) * (opts.replay_buffer_secs as usize + 4) + 32 * 1024 * 1024,
            full_video: opts.full_video,
            codec: encoder.desc.codec,
        };
        if !opts.full_video {
            log::info!("recorder: clips only (replay buffer, no full video)");
        }
        log::info!("recorder: starting muxer and capture");
        let mux_thread = std::thread::Builder::new().name("mux".into()).spawn(move || mux::run(mp, mux_rx))?;

        let cap =
            match capture::start(gpu.clone(), CaptureParams { target: cap_target, height: opts.height, fps, rec_start, cursor: true }, out_w, out_h, encoder.tx.clone(), encoder.in_flight.clone()) {
                Ok(c) => c,
                Err(e) => {
                    // Clean up everything already started.
                    encoder.stop();
                    for a in audio_caps {
                        a.stop();
                    }
                    let (tx, rx) = channel();
                    let _ = mux_tx.send(MuxMsg::Stop { reply: tx });
                    let _ = rx.recv_timeout(Duration::from_secs(5));
                    let _ = mux_thread.join();
                    return Err(e);
                }
            };
        let capture = Arc::new(Mutex::new(Some(cap)));

        // Watchdog: if window capture delivers nothing (some exclusive-fullscreen games),
        // switch to capturing the screen.
        let watchdog_stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        if let Target::Window(h) = cap_target {
            let h_raw = h.0 as isize;
            let (c, g, ws, etx, inf) = (capture.clone(), gpu.clone(), watchdog_stop.clone(), encoder.tx.clone(), encoder.in_flight.clone());
            let height = opts.height;
            std::thread::spawn(move || {
                com();
                std::thread::sleep(Duration::from_secs(6));
                if ws.load(std::sync::atomic::Ordering::SeqCst) {
                    return;
                }
                let frames = c.lock().unwrap().as_ref().map(|c| c.stats.frames.load(std::sync::atomic::Ordering::Relaxed)).unwrap_or(1);
                if frames == 0 {
                    log::warn!("window capture delivered no frames; switching to screen capture");
                    let mon = Target::Monitor(window::monitor_for(Some(windows::Win32::Foundation::HWND(h_raw as *mut _))));
                    let mut guard = c.lock().unwrap();
                    if let Some(old) = guard.take() {
                        old.stop();
                    }
                    match capture::start(g, CaptureParams { target: mon, height, fps, rec_start, cursor: true }, out_w, out_h, etx, inf) {
                        Ok(n) => *guard = Some(n),
                        Err(e) => log::error!("screen capture failed too: {e:#}"),
                    }
                }
            });
        }

        let desc = encoder.desc.clone();
        *self.inner.active.lock().unwrap() =
            Some(Active { rec_start, capture, encoder: Some(encoder), audio: audio_caps, mux_tx, mux_thread: Some(mux_thread), output_dir: opts.output_dir.clone(), watchdog_stop });
        let what = match cap_target {
            Target::Window(_) => "game window",
            Target::Monitor(_) => "screen",
        };
        log::info!("recording {what} at {out_w}x{out_h}@{fps}, {} kbps, {}; audio: {}", rate / 1000, desc.name, audio_desc.join(", "));
        self.set_status(|s| {
            s.recording = true;
            s.replay_buffer = true;
            s.connected = true;
            s.encoder = Some(format!("{} ({}, {})", desc.name, desc.vendor, desc.codec.as_str().to_uppercase()));
            s.hardware_encoder = true;
            s.error = None;
        });
        Ok(())
    }

    /// Lets the GPU idle between games (on hybrid laptops it keeps the dGPU awake otherwise).
    /// The capture and encoder threads hold their own references until they stop.
    fn release_gpu(&self) {
        *self.inner.gpu.lock().unwrap() = None;
    }

    fn stop_blocking(&self) -> Result<PathBuf> {
        self.release_gpu();
        let mut a = self.inner.active.lock().unwrap().take().context("not recording")?;
        a.watchdog_stop.store(true, std::sync::atomic::Ordering::SeqCst);
        if let Some(c) = a.capture.lock().unwrap().take() {
            let s = &c.stats;
            log::info!("captured {} frames, dropped {}", s.frames.load(std::sync::atomic::Ordering::Relaxed), s.dropped.load(std::sync::atomic::Ordering::Relaxed));
            c.stop();
        }
        if let Some(e) = a.encoder.take() {
            e.stop(); // drains the last frames into the mux
        }
        for au in a.audio.drain(..) {
            au.stop();
        }
        let (tx, rx) = channel();
        let _ = a.mux_tx.send(MuxMsg::Stop { reply: tx });
        let r = rx.recv_timeout(Duration::from_secs(60)).map_err(|_| anyhow!("the recording didn't finish writing"))?;
        if let Some(t) = a.mux_thread.take() {
            let _ = t.join();
        }
        self.set_status(|s| {
            s.recording = false;
            s.replay_buffer = false;
        });
        let (path, dur) = r?;
        log::info!("recording saved: {} ({dur:.1} s)", path.display());
        Ok(path)
    }

    fn save_replay_blocking(&self, want: Option<u32>) -> Result<PathBuf> {
        let (tx, rx) = channel();
        let secs;
        {
            let a = self.inner.active.lock().unwrap();
            let a = a.as_ref().context("not recording")?;
            let buf = self.inner.prepared.lock().unwrap().as_ref().map(|p| p.1.replay_buffer_secs).unwrap_or(30);
            secs = want.map(|w| w.clamp(3, buf)).unwrap_or(buf);
            let path = a.output_dir.join(format!("Replay_{}.mp4", stamp()));
            a.mux_tx.send(MuxMsg::SaveReplay { path, secs, reply: tx }).map_err(|_| anyhow!("recorder stopped"))?;
        }
        rx.recv_timeout(Duration::from_secs(30)).map_err(|_| anyhow!("saving the clip timed out"))?
    }
}

#[async_trait]
impl Recorder for NativeRecorder {
    async fn ensure_connected(&self) -> Result<()> {
        let me = NativeRecorder { inner: self.inner.clone() };
        tokio::task::spawn_blocking(move || me.gpu().map(|_| ())).await??;
        self.set_status(|s| s.connected = true);
        Ok(())
    }

    async fn prepare(&self, target: &CaptureTarget, opts: &RecordOptions) -> Result<()> {
        *self.inner.prepared.lock().unwrap() = Some((target.clone(), opts.clone()));
        Ok(())
    }

    async fn start_recording(&self) -> Result<()> {
        let me = NativeRecorder { inner: self.inner.clone() };
        let r = tokio::task::spawn_blocking(move || me.start_blocking()).await?;
        if let Err(e) = &r {
            let msg = format!("{e:#}");
            self.set_status(|s| s.error = Some(msg));
            self.release_gpu();
        }
        r
    }

    async fn stop_recording(&self) -> Result<PathBuf> {
        let me = NativeRecorder { inner: self.inner.clone() };
        tokio::task::spawn_blocking(move || me.stop_blocking()).await?
    }

    async fn record_elapsed(&self) -> Result<Option<Duration>> {
        let a = self.inner.active.lock().unwrap();
        Ok(a.as_ref().map(|a| Duration::from_nanos(((qpc_hns() - a.rec_start).max(0) as u64) * 100)))
    }

    async fn save_replay(&self, secs: Option<u32>) -> Result<PathBuf> {
        let me = NativeRecorder { inner: self.inner.clone() };
        tokio::task::spawn_blocking(move || me.save_replay_blocking(secs)).await?
    }

    async fn finish(&self) -> Result<()> {
        Ok(())
    }

    async fn status(&self) -> RecorderStatus {
        self.inner.status.lock().unwrap().clone()
    }

    fn clock_base_hns(&self) -> Option<i64> {
        self.inner.active.lock().unwrap().as_ref().map(|a| a.rec_start)
    }
}

/// Records `secs` seconds of the screen (or a window) to `out_dir`, for the self-test.
/// Returns the file and the frame statistics.
/// Test tools: records `exe`'s window at its own size, 60 fps (see `examples/inputtest.rs`).
pub fn start_test_recording(out_dir: &Path, exe: &str) -> Result<NativeRecorder> {
    let rec = NativeRecorder::new();
    let target = CaptureTarget { exe: exe.to_string(), display_capture_only: false };
    let opts = RecordOptions {
        output_dir: out_dir.to_path_buf(),
        encoder: "auto".into(),
        quality: "high".into(),
        fps: 60,
        height: 0,
        replay_buffer_secs: 5,
        record_mic: false,
        display_capture: false,
        full_video: true,
        codec: "h264".into(),
        rate_control: "bitrate".into(),
        playable: Vec::new(),
    };
    *rec.inner.prepared.lock().unwrap() = Some((target, opts));
    rec.start_blocking()?;
    Ok(rec)
}

impl NativeRecorder {
    /// Test tools: stops a recording started with [`start_test_recording`].
    pub fn stop_test_recording(&self) -> Result<PathBuf> {
        self.stop_blocking()
    }
    pub fn test_clock_base(&self) -> Option<i64> {
        self.inner.active.lock().unwrap().as_ref().map(|a| a.rec_start)
    }
}

/// Video settings for the self-test (the in-app test uses the user's own).
#[derive(Debug, Clone, Default)]
pub struct TestVideo {
    pub codec: String,
    pub rate_control: String,
    pub playable: Vec<String>,
}

pub fn self_test(out_dir: &Path, secs: u64, exe: Option<&str>, mic: bool, video: TestVideo) -> Result<(PathBuf, u64, u64)> {
    let rec = NativeRecorder::new();
    let target = CaptureTarget { exe: exe.unwrap_or("explorer.exe").to_string(), display_capture_only: exe.is_none() };
    let opts = RecordOptions {
        output_dir: out_dir.to_path_buf(),
        encoder: "auto".into(),
        quality: "standard".into(),
        fps: 60,
        height: 1080,
        replay_buffer_secs: 10,
        record_mic: mic,
        display_capture: exe.is_none(),
        full_video: true,
        codec: video.codec,
        rate_control: video.rate_control,
        playable: video.playable,
    };
    *rec.inner.prepared.lock().unwrap() = Some((target, opts));
    rec.start_blocking()?;
    std::thread::sleep(Duration::from_secs(secs));
    let frames = rec.inner.active.lock().unwrap().as_ref().and_then(|a| {
        a.capture
            .lock()
            .unwrap()
            .as_ref()
            .map(|c| (c.stats.frames.load(std::sync::atomic::Ordering::Relaxed), c.stats.dropped.load(std::sync::atomic::Ordering::Relaxed)))
    });
    let _ = rec.save_replay_blocking(None);
    let path = rec.stop_blocking()?;
    // The same thumbnail path the app uses after a game (decode a frame from the file).
    if let Err(e) = thumb::video_thumbnail(&path, secs as f64 / 2.0, &out_dir.join("selftest.jpg"), 480) {
        log::warn!("self-test thumbnail: {e:#}");
    }
    match crate::remux::info(&path) {
        Ok(i) => log::info!(
            "self-test file: {:?}, {}, {} frames, keyframes every {:.2} s (max {:.2} s)",
            i.layout,
            i.codec.unwrap_or_default(),
            i.video_frames,
            i.keyframe_interval_avg,
            i.keyframe_interval_max
        ),
        Err(e) => log::warn!("self-test file info: {e}"),
    }
    let (f, d) = frames.unwrap_or((0, 0));
    Ok((path, f, d))
}
