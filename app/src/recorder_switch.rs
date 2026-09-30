//! Chooses between the built-in recorder and OBS (backup) per game, from the settings.
//! Both implement the same `Recorder` trait, so the engine doesn't know which one runs.

use async_trait::async_trait;
use gr_core::game::CaptureTarget;
use gr_core::recorder::{RecordOptions, Recorder, RecorderStatus};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::Duration;

pub struct RecorderSwitch {
    pub builtin: Arc<gr_capture::NativeRecorder>,
    pub obs: Arc<gr_obs::ObsRecorder>,
    /// What the settings say ("builtin" / "obs").
    mode: RwLock<String>,
    /// The recorder used by the current game (fixed from `prepare` until `finish`).
    current: RwLock<Option<String>>,
}

impl RecorderSwitch {
    pub fn new(builtin: Arc<gr_capture::NativeRecorder>, obs: Arc<gr_obs::ObsRecorder>, mode: &str) -> Self {
        Self { builtin, obs, mode: RwLock::new(mode.to_string()), current: RwLock::new(None) }
    }

    pub fn set_mode(&self, mode: &str) {
        *self.mode.write().unwrap() = mode.to_string();
    }

    pub fn mode(&self) -> String {
        self.mode.read().unwrap().clone()
    }

    fn active(&self) -> Arc<dyn Recorder> {
        let m = self.current.read().unwrap().clone().unwrap_or_else(|| self.mode());
        if m == "obs" {
            self.obs.clone()
        } else {
            self.builtin.clone()
        }
    }
}

#[async_trait]
impl Recorder for RecorderSwitch {
    async fn ensure_connected(&self) -> anyhow::Result<()> {
        *self.current.write().unwrap() = Some(self.mode());
        self.active().ensure_connected().await
    }
    async fn prepare(&self, target: &CaptureTarget, opts: &RecordOptions) -> anyhow::Result<()> {
        self.active().prepare(target, opts).await
    }
    async fn start_recording(&self) -> anyhow::Result<()> {
        self.active().start_recording().await
    }
    async fn stop_recording(&self) -> anyhow::Result<PathBuf> {
        self.active().stop_recording().await
    }
    async fn record_elapsed(&self) -> anyhow::Result<Option<Duration>> {
        self.active().record_elapsed().await
    }
    async fn save_replay(&self) -> anyhow::Result<PathBuf> {
        self.active().save_replay().await
    }
    async fn screenshot(&self, path: &Path, width: u32) -> anyhow::Result<()> {
        self.active().screenshot(path, width).await
    }
    async fn finish(&self) -> anyhow::Result<()> {
        let r = self.active().finish().await;
        *self.current.write().unwrap() = None;
        r
    }
    async fn status(&self) -> RecorderStatus {
        let mut s = self.active().status().await;
        if self.current.read().unwrap().is_none() && self.mode() != "obs" {
            // The built-in recorder is always ready.
            s.connected = true;
        }
        s
    }
}
