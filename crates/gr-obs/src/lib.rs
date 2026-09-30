//! [`Recorder`] implementation that drives OBS Studio over obs-websocket v5.
//!
//! Setup it performs (all inside its own OBS profile and scene collection named
//! "GameRecorder", so the user's own OBS setup is untouched and restored afterwards):
//! - Simple output mode, hardware encoder (NVENC/AMF/QSV, never x264),
//!   hybrid MP4 (crash-safe and playable), replay buffer on.
//! - Scene "Game" with Game Capture of the game window (Display Capture as fallback).

pub mod client;

use anyhow::{anyhow, bail};
use async_trait::async_trait;
use client::ObsClient;
use gr_core::game::CaptureTarget;
use gr_core::recorder::{RecordOptions, Recorder, RecorderStatus};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

pub const PROFILE: &str = "GameRecorder";
pub const COLLECTION: &str = "GameRecorder";
pub const SCENE: &str = "Game";
pub const GAME_SOURCE: &str = "Game Capture";
pub const DISPLAY_SOURCE: &str = "Display Capture";

// Source kinds. (The Linux ones exist only so the OBS integration can be tested on Linux.)
#[cfg(windows)]
const GAME_KIND: &str = "game_capture";
#[cfg(windows)]
const DISPLAY_KIND: &str = "monitor_capture";
#[cfg(not(windows))]
const GAME_KIND: &str = "color_source_v3";
#[cfg(not(windows))]
const DISPLAY_KIND: &str = "xshm_input";

/// Testing only: allow CPU encoding (x264) when GR_ALLOW_CPU_ENCODER=1.
fn cpu_allowed() -> bool {
    std::env::var("GR_ALLOW_CPU_ENCODER").is_ok_and(|v| v == "1")
}

#[derive(Debug, Clone, Default)]
pub struct ObsConfig {
    pub host: String,
    pub port: u16,
    pub password: String,
}

/// Launches OBS if it isn't running. Returns true if it launched (or was running).
pub type Launcher = Arc<dyn Fn() -> bool + Send + Sync>;

#[derive(Default)]
struct Restore {
    profile: Option<String>,
    collection: Option<String>,
}

pub struct ObsRecorder {
    cfg: StdMutex<ObsConfig>,
    client: Mutex<Option<ObsClient>>,
    launcher: Option<Launcher>,
    status: StdMutex<RecorderStatus>,
    restore: StdMutex<Restore>,
    /// Prevent hammering OBS with connection attempts.
    last_attempt: StdMutex<Option<Instant>>,
}

fn is_hardware(encoder: &str) -> bool {
    !encoder.is_empty() && !encoder.starts_with("x264")
}

/// Parses "32.0.1" into a comparable tuple.
pub fn parse_version(v: &str) -> (u32, u32, u32) {
    let mut it = v.split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty()).map(|s| s.parse().unwrap_or(0));
    (it.next().unwrap_or(0), it.next().unwrap_or(0), it.next().unwrap_or(0))
}

/// Output size for a target height, keeping the canvas aspect ratio (even numbers for the encoder).
pub fn output_size(base_w: u32, base_h: u32, height: u32) -> (u32, u32) {
    if height == 0 || height >= base_h || base_h == 0 {
        return (base_w, base_h);
    }
    let w = (base_w as f64 * height as f64 / base_h as f64).round() as u32;
    (w & !1, height & !1)
}

impl ObsRecorder {
    pub fn new(cfg: ObsConfig, launcher: Option<Launcher>) -> Self {
        Self {
            cfg: StdMutex::new(cfg),
            client: Mutex::new(None),
            launcher,
            status: StdMutex::new(RecorderStatus::default()),
            restore: StdMutex::new(Restore::default()),
            last_attempt: StdMutex::new(None),
        }
    }

    /// New connection details (from Settings). Drops the current connection if they changed.
    pub async fn set_config(&self, cfg: ObsConfig) {
        let changed = {
            let mut c = self.cfg.lock().unwrap();
            let changed = c.host != cfg.host || c.port != cfg.port || c.password != cfg.password;
            *c = cfg;
            changed
        };
        if changed {
            if let Some(c) = self.client.lock().await.take() {
                c.close().await;
            }
            self.set_status(|s| s.connected = false);
            *self.last_attempt.lock().unwrap() = None;
        }
    }

    fn set_status(&self, f: impl FnOnce(&mut RecorderStatus)) {
        f(&mut self.status.lock().unwrap());
    }

    async fn req(&self, t: &str, d: Value) -> anyhow::Result<Value> {
        let guard = self.client.lock().await;
        let c = guard.as_ref().ok_or_else(|| anyhow!("not connected to OBS"))?;
        c.request(t, d).await
    }

    async fn connect_once(&self) -> anyhow::Result<()> {
        let cfg = self.cfg.lock().unwrap().clone();
        let client = ObsClient::connect(&cfg.host, cfg.port, &cfg.password).await?;
        let v = client.request("GetVersion", Value::Null).await?;
        let version = v["obsVersion"].as_str().unwrap_or("").to_string();
        if parse_version(&version).0 < 30 {
            self.set_status(|s| s.error = Some(format!("OBS {version} is too old. Please update OBS to version 30 or newer.")));
        }
        *self.client.lock().await = Some(client);
        self.set_status(|s| {
            s.connected = true;
            s.version = Some(version);
            s.error = None;
        });
        log::info!("connected to OBS");
        Ok(())
    }

    /// Test the connection with the current settings (used by the Settings page).
    pub async fn test_connection(&self) -> anyhow::Result<String> {
        *self.last_attempt.lock().unwrap() = None;
        if let Some(c) = self.client.lock().await.take() {
            c.close().await;
        }
        self.connect_once().await?;
        Ok(self.status.lock().unwrap().version.clone().unwrap_or_default())
    }

    async fn get_param(&self, cat: &str, name: &str) -> Option<String> {
        self.req("GetProfileParameter", json!({ "parameterCategory": cat, "parameterName": name }))
            .await
            .ok()
            .and_then(|v| v["parameterValue"].as_str().map(str::to_string))
    }

    async fn set_param(&self, cat: &str, name: &str, value: &str) -> anyhow::Result<bool> {
        if self.get_param(cat, name).await.as_deref() == Some(value) {
            return Ok(false);
        }
        self.req("SetProfileParameter", json!({ "parameterCategory": cat, "parameterName": name, "parameterValue": value }))
            .await?;
        Ok(true)
    }

    async fn ensure_profile(&self, opts: &RecordOptions, version: (u32, u32, u32)) -> anyhow::Result<()> {
        let list = self.req("GetProfileList", Value::Null).await?;
        let current = list["currentProfileName"].as_str().unwrap_or("").to_string();
        let profiles: Vec<String> =
            list["profiles"].as_array().map(|a| a.iter().filter_map(|p| p.as_str().map(str::to_string)).collect()).unwrap_or_default();
        {
            let mut r = self.restore.lock().unwrap();
            if current != PROFILE && r.profile.is_none() {
                r.profile = Some(current.clone());
            }
        }
        if !profiles.iter().any(|p| p == PROFILE) {
            self.req("CreateProfile", json!({ "profileName": PROFILE })).await?;
        } else if current != PROFILE {
            self.req("SetCurrentProfile", json!({ "profileName": PROFILE })).await?;
        }

        let encoder = match opts.encoder.as_str() {
            "nvenc" | "amd" | "qsv" => opts.encoder.clone(),
            "x264" if cpu_allowed() => "x264".into(),
            // "auto" is resolved by the app from the GPU vendor; keep a sane default.
            _ => "nvenc".into(),
        };
        let format = if version >= (30, 2, 0) { "hybrid_mp4" } else { "fragmented_mp4" };
        let quality = if opts.quality == "high" { "HQ" } else { "Small" };
        let rb_secs = opts.replay_buffer_secs.max(5).to_string();
        let dir = opts.output_dir.to_string_lossy().to_string();
        let mut changed = false;
        changed |= self.set_param("Output", "Mode", "Simple").await?;
        changed |= self.set_param("SimpleOutput", "RecEncoder", &encoder).await?;
        changed |= self.set_param("SimpleOutput", "RecQuality", quality).await?;
        changed |= self.set_param("SimpleOutput", "RecFormat2", format).await?;
        changed |= self.set_param("SimpleOutput", "RecRB", if opts.replay_buffer_secs > 0 { "true" } else { "false" }).await?;
        changed |= self.set_param("SimpleOutput", "RecRBTime", &rb_secs).await?;
        changed |= self.set_param("SimpleOutput", "RecRBSize", "2048").await?;
        self.set_param("SimpleOutput", "FileNameWithoutSpace", "true").await?;
        self.set_param("SimpleOutput", "FilePath", &dir).await?;
        let _ = self.req("SetRecordDirectory", json!({ "recordDirectory": dir })).await;

        // Video: keep the canvas (base) size, scale the output, set fps.
        let vs = self.req("GetVideoSettings", Value::Null).await?;
        let (bw, bh) = (vs["baseWidth"].as_u64().unwrap_or(1920) as u32, vs["baseHeight"].as_u64().unwrap_or(1080) as u32);
        let (ow, oh) = output_size(bw, bh, opts.height);
        let fps = opts.fps.clamp(10, 240);
        let cur_fps = vs["fpsNumerator"].as_u64().unwrap_or(0) as u32 / (vs["fpsDenominator"].as_u64().unwrap_or(1).max(1) as u32);
        if vs["outputWidth"].as_u64() != Some(ow as u64) || vs["outputHeight"].as_u64() != Some(oh as u64) || cur_fps != fps {
            self.req(
                "SetVideoSettings",
                json!({ "fpsNumerator": fps, "fpsDenominator": 1, "baseWidth": bw, "baseHeight": bh, "outputWidth": ow, "outputHeight": oh }),
            )
            .await?;
        }

        // OBS builds its outputs/encoders when a profile is activated, so switch away and
        // back to be sure the settings above are the ones used (also recovers from a
        // previously failed encoder).
        let _ = changed;
        {
            let other = self.restore.lock().unwrap().profile.clone().filter(|p| p != PROFILE).or_else(|| profiles.iter().find(|p| *p != PROFILE).cloned());
            if let Some(other) = other {
                self.req("SetCurrentProfile", json!({ "profileName": other })).await?;
                self.req("SetCurrentProfile", json!({ "profileName": PROFILE })).await?;
            }
        }

        let enc = self.get_param("SimpleOutput", "RecEncoder").await.unwrap_or_default();
        self.set_status(|s| {
            s.hardware_encoder = is_hardware(&enc);
            s.encoder = Some(enc.clone());
        });
        if !is_hardware(&enc) && !cpu_allowed() {
            bail!("OBS is set to CPU encoding ({enc}). Choose your GPU encoder in Settings > Video.");
        }
        Ok(())
    }

    async fn ensure_scene(&self, target: &CaptureTarget, opts: &RecordOptions) -> anyhow::Result<()> {
        let list = self.req("GetSceneCollectionList", Value::Null).await?;
        let current = list["currentSceneCollectionName"].as_str().unwrap_or("").to_string();
        let exists = list["sceneCollections"].as_array().is_some_and(|a| a.iter().any(|c| c.as_str() == Some(COLLECTION)));
        {
            let mut r = self.restore.lock().unwrap();
            if current != COLLECTION && r.collection.is_none() {
                r.collection = Some(current.clone());
            }
        }
        if !exists {
            self.req("CreateSceneCollection", json!({ "sceneCollectionName": COLLECTION })).await?;
        } else if current != COLLECTION {
            self.req("SetCurrentSceneCollection", json!({ "sceneCollectionName": COLLECTION })).await?;
        }

        let scenes = self.req("GetSceneList", Value::Null).await?;
        let has_scene = scenes["scenes"].as_array().is_some_and(|a| a.iter().any(|s| s["sceneName"] == SCENE));
        if !has_scene {
            self.req("CreateScene", json!({ "sceneName": SCENE })).await?;
        }
        self.req("SetCurrentProgramScene", json!({ "sceneName": SCENE })).await?;

        let vs = self.req("GetVideoSettings", Value::Null).await?;
        let (bw, bh) = (vs["baseWidth"].as_f64().unwrap_or(1920.0), vs["baseHeight"].as_f64().unwrap_or(1080.0));
        let use_display = opts.display_capture || target.display_capture_only;

        let game_settings = json!({
            "capture_mode": "window",
            "window": target.window,
            "priority": 2,
            "capture_cursor": true,
            "allow_transparency": false,
            "capture_overlays": false,
            "anti_cheat_hook": true,
        });
        self.ensure_input(GAME_SOURCE, GAME_KIND, game_settings, !use_display, bw, bh).await?;
        self.ensure_input(DISPLAY_SOURCE, DISPLAY_KIND, json!({ "capture_cursor": true }), use_display, bw, bh).await?;
        // Game capture on top.
        if let Ok(id) = self.item_id(GAME_SOURCE).await {
            let _ = self.req("SetSceneItemIndex", json!({ "sceneName": SCENE, "sceneItemId": id, "sceneItemIndex": 1 })).await;
        }

        // Audio: desktop audio on, mic per setting.
        if let Ok(sp) = self.req("GetSpecialInputs", Value::Null).await {
            if let Some(mic) = sp["mic1"].as_str() {
                let _ = self.req("SetInputMute", json!({ "inputName": mic, "inputMuted": !opts.record_mic })).await;
            }
            if let Some(desk) = sp["desktop1"].as_str() {
                let _ = self.req("SetInputMute", json!({ "inputName": desk, "inputMuted": false })).await;
            }
        }
        Ok(())
    }

    async fn item_id(&self, source: &str) -> anyhow::Result<i64> {
        let v = self.req("GetSceneItemId", json!({ "sceneName": SCENE, "sourceName": source })).await?;
        v["sceneItemId"].as_i64().ok_or_else(|| anyhow!("no scene item id"))
    }

    async fn ensure_input(&self, name: &str, kind: &str, settings: Value, enabled: bool, bw: f64, bh: f64) -> anyhow::Result<()> {
        let id = match self.item_id(name).await {
            Ok(id) => {
                self.req("SetInputSettings", json!({ "inputName": name, "inputSettings": settings, "overlay": true })).await?;
                id
            }
            Err(_) => {
                let v = self
                    .req("CreateInput", json!({ "sceneName": SCENE, "inputName": name, "inputKind": kind, "inputSettings": settings, "sceneItemEnabled": enabled }))
                    .await?;
                v["sceneItemId"].as_i64().unwrap_or_default()
            }
        };
        self.req("SetSceneItemEnabled", json!({ "sceneName": SCENE, "sceneItemId": id, "sceneItemEnabled": enabled })).await?;
        // Fit to the canvas whatever the game resolution.
        let _ = self
            .req(
                "SetSceneItemTransform",
                json!({ "sceneName": SCENE, "sceneItemId": id, "sceneItemTransform": {
                    "boundsType": "OBS_BOUNDS_SCALE_INNER", "boundsWidth": bw, "boundsHeight": bh,
                    "boundsAlignment": 0, "alignment": 5, "positionX": 0.0, "positionY": 0.0 } }),
            )
            .await;
        Ok(())
    }

    async fn wait_event(&self, rx: &mut tokio::sync::broadcast::Receiver<client::ObsEvent>, pred: impl Fn(&client::ObsEvent) -> bool, timeout: Duration) -> Option<client::ObsEvent> {
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return None;
            }
            match tokio::time::timeout(left, rx.recv()).await {
                Ok(Ok(e)) if pred(&e) => return Some(e),
                Ok(Ok(_)) => continue,
                Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => continue,
                _ => return None,
            }
        }
    }

    async fn events(&self) -> anyhow::Result<tokio::sync::broadcast::Receiver<client::ObsEvent>> {
        let g = self.client.lock().await;
        Ok(g.as_ref().ok_or_else(|| anyhow!("not connected to OBS"))?.subscribe())
    }
}

#[async_trait]
impl Recorder for ObsRecorder {
    async fn ensure_connected(&self) -> anyhow::Result<()> {
        if self.client.lock().await.as_ref().is_some_and(|c| c.is_alive()) {
            return Ok(());
        }
        self.set_status(|s| s.connected = false);
        match self.connect_once().await {
            Ok(()) => return Ok(()),
            Err(e) => {
                let Some(launch) = &self.launcher else { return Err(e) };
                // Not running? Launch OBS minimized and wait for its WebSocket server.
                {
                    let mut last = self.last_attempt.lock().unwrap();
                    if last.is_some_and(|t| t.elapsed() < Duration::from_secs(20)) {
                        return Err(e);
                    }
                    *last = Some(Instant::now());
                }
                if !launch() {
                    self.set_status(|s| s.error = Some(format!("{e:#}")));
                    return Err(e);
                }
                log::info!("launched OBS, waiting for it");
                let mut last_err = e;
                for _ in 0..25 {
                    tokio::time::sleep(Duration::from_millis(800)).await;
                    match self.connect_once().await {
                        Ok(()) => return Ok(()),
                        Err(e) => last_err = e,
                    }
                }
                self.set_status(|s| s.error = Some(format!("{last_err:#}")));
                Err(last_err)
            }
        }
    }

    async fn prepare(&self, target: &CaptureTarget, opts: &RecordOptions) -> anyhow::Result<()> {
        let st = self.req("GetRecordStatus", Value::Null).await?;
        if st["outputActive"].as_bool() == Some(true) {
            bail!("OBS is already recording something else, so GameRecorder left it alone.");
        }
        let stream = self.req("GetStreamStatus", Value::Null).await?;
        if stream["outputActive"].as_bool() == Some(true) {
            bail!("OBS is streaming, so GameRecorder didn't change its setup.");
        }
        // Replay buffer must be stopped to change settings.
        let _ = self.req("StopReplayBuffer", Value::Null).await;
        let version = parse_version(self.status.lock().unwrap().version.as_deref().unwrap_or("0"));
        self.ensure_profile(opts, version).await?;
        self.ensure_scene(target, opts).await?;
        std::fs::create_dir_all(&opts.output_dir)?;
        Ok(())
    }

    async fn start_recording(&self) -> anyhow::Result<()> {
        let mut rx = self.events().await?;
        self.req("StartRecord", Value::Null).await?;
        let started = self
            .wait_event(&mut rx, |e| e.event_type == "RecordStateChanged" && e.data["outputState"] == "OBS_WEBSOCKET_OUTPUT_STARTED", Duration::from_secs(8))
            .await;
        if started.is_none() {
            let st = self.req("GetRecordStatus", Value::Null).await?;
            if st["outputActive"].as_bool() != Some(true) {
                bail!("OBS didn't start recording. Check OBS for an encoder error (is your GPU driver up to date?).");
            }
        }
        if let Err(e) = self.req("StartReplayBuffer", Value::Null).await {
            log::warn!("replay buffer: {e:#}");
        }
        let rb = self.req("GetReplayBufferStatus", Value::Null).await.ok().and_then(|v| v["outputActive"].as_bool()).unwrap_or(false);
        self.set_status(|s| {
            s.recording = true;
            s.replay_buffer = rb;
        });
        Ok(())
    }

    async fn stop_recording(&self) -> anyhow::Result<PathBuf> {
        let _ = self.req("StopReplayBuffer", Value::Null).await;
        let mut rx = self.events().await?;
        let v = self.req("StopRecord", Value::Null).await?;
        // Wait until OBS has finished writing the file.
        let stopped = self
            .wait_event(&mut rx, |e| e.event_type == "RecordStateChanged" && e.data["outputState"] == "OBS_WEBSOCKET_OUTPUT_STOPPED", Duration::from_secs(30))
            .await;
        self.set_status(|s| {
            s.recording = false;
            s.replay_buffer = false;
        });
        let path = stopped
            .and_then(|e| e.data["outputPath"].as_str().map(str::to_string))
            .or_else(|| v["outputPath"].as_str().map(str::to_string))
            .filter(|p| !p.is_empty())
            .ok_or_else(|| anyhow!("OBS didn't report where it saved the recording"))?;
        Ok(PathBuf::from(path))
    }

    async fn record_elapsed(&self) -> anyhow::Result<Option<Duration>> {
        let v = self.req("GetRecordStatus", Value::Null).await?;
        if v["outputActive"].as_bool() != Some(true) {
            return Ok(None);
        }
        Ok(v["outputDuration"].as_u64().map(Duration::from_millis))
    }

    async fn save_replay(&self) -> anyhow::Result<PathBuf> {
        let mut rx = self.events().await?;
        self.req("SaveReplayBuffer", Value::Null).await?;
        let ev = self.wait_event(&mut rx, |e| e.event_type == "ReplayBufferSaved", Duration::from_secs(15)).await;
        let path = match ev.and_then(|e| e.data["savedReplayPath"].as_str().map(str::to_string)) {
            Some(p) => p,
            None => self.req("GetLastReplayBufferReplay", Value::Null).await?["savedReplayPath"].as_str().unwrap_or("").to_string(),
        };
        if path.is_empty() {
            bail!("OBS didn't save the replay (is the replay buffer running?)");
        }
        Ok(PathBuf::from(path))
    }

    async fn screenshot(&self, path: &Path, width: u32) -> anyhow::Result<()> {
        self.req(
            "SaveSourceScreenshot",
            json!({ "sourceName": SCENE, "imageFormat": "jpg", "imageFilePath": path.to_string_lossy(), "imageWidth": width, "imageCompressionQuality": 80 }),
        )
        .await?;
        Ok(())
    }

    async fn finish(&self) -> anyhow::Result<()> {
        let r = std::mem::take(&mut *self.restore.lock().unwrap());
        if let Some(c) = r.collection.filter(|c| c != COLLECTION && !c.is_empty()) {
            self.req("SetCurrentSceneCollection", json!({ "sceneCollectionName": c })).await?;
        }
        if let Some(p) = r.profile.filter(|p| p != PROFILE && !p.is_empty()) {
            self.req("SetCurrentProfile", json!({ "profileName": p })).await?;
        }
        Ok(())
    }

    async fn status(&self) -> RecorderStatus {
        let alive = self.client.lock().await.as_ref().is_some_and(|c| c.is_alive());
        let mut s = self.status.lock().unwrap().clone();
        s.connected = alive;
        if !alive {
            s.recording = false;
            s.replay_buffer = false;
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn versions_and_sizes() {
        assert!(parse_version("32.0.1") >= (30, 2, 0));
        assert!(parse_version("30.1.2") < (30, 2, 0));
        assert_eq!(output_size(2560, 1440, 1080), (1920, 1080));
        assert_eq!(output_size(3440, 1440, 1080), (2580, 1080));
        assert_eq!(output_size(1920, 1080, 1440), (1920, 1080));
        assert_eq!(output_size(1920, 1080, 0), (1920, 1080));
        assert!(is_hardware("nvenc") && !is_hardware("x264"));
    }
}
