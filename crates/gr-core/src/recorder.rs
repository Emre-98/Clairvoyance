//! Recorder abstraction, implemented by the built-in recorder (`gr-capture`: Windows Graphics
//! Capture + hardware H.264). Keeping it behind a trait lets the engine be tested with a fake.

use crate::game::CaptureTarget;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RecorderStatus {
    pub connected: bool,
    pub recording: bool,
    pub replay_buffer: bool,
    /// e.g. "32.0.1"
    pub version: Option<String>,
    /// Encoder in use, e.g. "nvenc". "x264" means CPU encoding (a warning in the UI).
    pub encoder: Option<String>,
    pub hardware_encoder: bool,
    /// Last error to show the user.
    pub error: Option<String>,
}

/// Recording options derived from the settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordOptions {
    pub output_dir: PathBuf,
    /// "nvenc", "amd", "qsv", or "auto".
    pub encoder: String,
    /// "standard" or "high".
    pub quality: String,
    pub fps: u32,
    /// Output height, 0 = same as the screen.
    pub height: u32,
    pub replay_buffer_secs: u32,
    pub record_mic: bool,
    /// Force Display Capture instead of Game Capture.
    pub display_capture: bool,
}

#[async_trait]
pub trait Recorder: Send + Sync {
    /// Connects if needed (and may launch the recorder). Cheap when already connected.
    async fn ensure_connected(&self) -> anyhow::Result<()>;
    /// Sets up scene, capture source, encoder and output folder for a game.
    async fn prepare(&self, target: &CaptureTarget, opts: &RecordOptions) -> anyhow::Result<()>;
    async fn start_recording(&self) -> anyhow::Result<()>;
    /// Stops and returns the path of the saved video.
    async fn stop_recording(&self) -> anyhow::Result<PathBuf>;
    /// How long the current recording has run, according to the recorder itself.
    async fn record_elapsed(&self) -> anyhow::Result<Option<Duration>>;
    /// Saves the replay buffer and returns the saved file.
    async fn save_replay(&self) -> anyhow::Result<PathBuf>;
    /// Saves a still frame of the current output (used for thumbnails).
    async fn screenshot(&self, path: &Path, width: u32) -> anyhow::Result<()>;
    /// Called after a game so the recorder can restore the user's own setup.
    async fn finish(&self) -> anyhow::Result<()>;
    async fn status(&self) -> RecorderStatus;
}
