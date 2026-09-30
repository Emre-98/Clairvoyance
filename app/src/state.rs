use crate::ffmpeg::Ffmpeg;
use crate::platform::WinPlatform;
use cv_core::engine::{EngineCommand, LiveStatus};
use cv_core::Settings;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use tokio::sync::mpsc::UnboundedSender;

#[derive(Debug, Clone)]
pub struct Paths {
    pub config_file: PathBuf,
    pub data_dir: PathBuf,
    pub log_file: PathBuf,
    pub default_save_dir: PathBuf,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct GpuInfo {
    pub vendor: String,
    pub name: String,
    pub encoder: String,
}

pub struct AppState {
    pub paths: Paths,
    pub settings: Arc<RwLock<Settings>>,
    pub cmd: UnboundedSender<EngineCommand>,
    pub live: Mutex<LiveStatus>,
    /// The built-in recorder (the engine records with it; the performance test drives it too).
    pub recorder: Arc<cv_capture::NativeRecorder>,
    pub ffmpeg: Arc<Ffmpeg>,
    pub platform: Arc<WinPlatform>,
    pub gpu: Option<GpuInfo>,
    pub engine_task: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    pub perf: Mutex<crate::perftest::PerfTestStatus>,
    pub perf_cancel: std::sync::atomic::AtomicBool,
    /// The window is shown (possibly minimized). While hidden, the UI is paused and gets
    /// no live events; it catches up ("resync") when shown again.
    pub ui_visible: std::sync::atomic::AtomicBool,
    /// Summaries of every game (cached on disk, refreshed incrementally).
    pub library: Arc<cv_core::library::LibraryIndex>,
    /// Something on disk changed while it wasn't a good moment to re-read it (in game, window
    /// hidden): refresh when the window is shown or the game ends.
    pub library_dirty: std::sync::atomic::AtomicBool,
    /// The background refresh after start-up has been started.
    pub library_first_refresh: std::sync::atomic::AtomicBool,
    pub maintenance: crate::maintenance::Maintenance,
}

impl AppState {
    pub fn settings(&self) -> Settings {
        self.settings.read().unwrap().clone()
    }

    pub fn save_dir(&self) -> PathBuf {
        self.settings.read().unwrap().save_dir_or(&self.paths.default_save_dir)
    }

    /// Settings as the engine should see them ("auto" encoder resolved to the GPU's).
    pub fn engine_settings(&self, s: &Settings) -> Settings {
        resolve_encoder(s, self.gpu.as_ref())
    }
}

pub fn resolve_encoder(s: &Settings, gpu: Option<&GpuInfo>) -> Settings {
    let mut s = s.clone();
    if s.video.encoder == "auto" || s.video.encoder.is_empty() {
        s.video.encoder = gpu.map(|g| g.encoder.clone()).unwrap_or_else(|| "nvenc".into());
    }
    s
}
