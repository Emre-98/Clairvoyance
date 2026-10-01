//! Recorder abstraction, implemented by the built-in recorder (`cv-capture`: Windows Graphics
//! Capture + hardware H.264). Keeping it behind a trait lets the engine be tested with a fake.

use crate::game::CaptureTarget;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
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
    /// Capture the whole monitor instead of the game window.
    pub display_capture: bool,
    /// false = "clips only": keep the replay buffer (hotkey + event clips), no full video file.
    #[serde(default = "yes")]
    pub full_video: bool,
}

fn yes() -> bool {
    true
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
    /// Saves the last `secs` seconds of the replay buffer (all of it if `None`) and returns
    /// the saved file.
    async fn save_replay(&self, secs: Option<u32>) -> anyhow::Result<PathBuf>;
    /// Called after a game so the recorder can restore the user's own setup.
    async fn finish(&self) -> anyhow::Result<()>;
    async fn status(&self) -> RecorderStatus;
    /// The recording's video clock: QPC (100 ns units) at video time 0, the base its frame
    /// timestamps use. Input recorded with the same clock lines up with the frames.
    fn clock_base_hns(&self) -> Option<i64> {
        None
    }
}
