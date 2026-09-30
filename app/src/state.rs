use crate::ffmpeg::Ffmpeg;
use crate::platform::WinPlatform;
use gr_core::engine::{EngineCommand, LiveStatus};
use gr_core::Settings;
use gr_obs::ObsRecorder;
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
    /// OBS (backup recorder).
    pub recorder: Arc<ObsRecorder>,
    /// What the engine records with: built-in or OBS, per the settings.
    pub switch: Arc<crate::recorder_switch::RecorderSwitch>,
    pub ffmpeg: Arc<Ffmpeg>,
    pub platform: Arc<WinPlatform>,
    pub gpu: Option<GpuInfo>,
    pub engine_task: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    pub perf: Mutex<crate::perftest::PerfTestStatus>,
    pub perf_cancel: std::sync::atomic::AtomicBool,
    /// The window is shown (possibly minimized). While hidden, the UI is paused and gets
    /// no live events; it catches up ("resync") when shown again.
    pub ui_visible: std::sync::atomic::AtomicBool,
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
