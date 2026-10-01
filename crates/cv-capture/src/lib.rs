//! Built-in game recorder: Windows Graphics Capture + hardware H.264 (NVENC / AMF / Quick Sync
//! through Media Foundation) + per-process WASAPI loopback audio, written as crash-safe
//! fragmented MP4, with an in-memory replay buffer. Implements `cv_core::Recorder`.
//!
//! `mp4` and `replay` are portable (tested anywhere); everything touching Windows lives in `win`.

pub mod mp4;
pub mod nv12;
pub mod remux;
pub mod replay;

#[cfg(windows)]
pub mod win;
#[cfg(windows)]
pub use win::NativeRecorder;

/// Stand-in on other platforms (development builds only): recording is Windows-only.
#[cfg(not(windows))]
pub struct NativeRecorder;

#[cfg(not(windows))]
impl NativeRecorder {
    pub fn new() -> Self {
        NativeRecorder
    }
    pub fn available_encoders() -> anyhow::Result<(String, Vec<String>)> {
        anyhow::bail!("the built-in recorder only works on Windows")
    }
}

#[cfg(not(windows))]
impl Default for NativeRecorder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(not(windows))]
#[async_trait::async_trait]
impl cv_core::Recorder for NativeRecorder {
    async fn ensure_connected(&self) -> anyhow::Result<()> {
        anyhow::bail!("the built-in recorder only works on Windows")
    }
    async fn prepare(&self, _: &cv_core::game::CaptureTarget, _: &cv_core::recorder::RecordOptions) -> anyhow::Result<()> {
        Ok(())
    }
    async fn start_recording(&self) -> anyhow::Result<()> {
        anyhow::bail!("the built-in recorder only works on Windows")
    }
    async fn stop_recording(&self) -> anyhow::Result<std::path::PathBuf> {
        anyhow::bail!("not recording")
    }
    async fn record_elapsed(&self) -> anyhow::Result<Option<std::time::Duration>> {
        Ok(None)
    }
    async fn save_replay(&self, _secs: Option<u32>) -> anyhow::Result<std::path::PathBuf> {
        anyhow::bail!("not recording")
    }
    async fn finish(&self) -> anyhow::Result<()> {
        Ok(())
    }
    async fn status(&self) -> cv_core::recorder::RecorderStatus {
        cv_core::recorder::RecorderStatus::default()
    }
}
