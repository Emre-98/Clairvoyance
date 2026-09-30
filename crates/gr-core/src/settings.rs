//! User settings, stored as JSON in the app's config folder. Every field has a default,
//! so older settings files keep working when new options are added.

use crate::events::EventKind;
use crate::game::KeyPress;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ObsSettings {
    pub host: String,
    pub port: u16,
    pub password: String,
    /// Path to obs64.exe (detected on first run).
    pub exe_path: Option<String>,
    /// Launch OBS (minimized to tray) when a game starts and it isn't running.
    pub auto_launch: bool,
}

impl Default for ObsSettings {
    fn default() -> Self {
        Self { host: "127.0.0.1".into(), port: 4455, password: String::new(), exe_path: None, auto_launch: true }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct VideoSettings {
    /// "auto", "nvenc", "amd", "qsv".
    pub encoder: String,
    /// "standard" (smaller files) or "high".
    pub quality: String,
    pub fps: u32,
    /// 0 = native resolution.
    pub height: u32,
    pub replay_buffer_secs: u32,
    pub record_mic: bool,
    /// Use Display Capture instead of Game Capture.
    pub display_capture: bool,
}

impl Default for VideoSettings {
    fn default() -> Self {
        Self {
            encoder: "auto".into(),
            quality: "standard".into(),
            fps: 60,
            height: 1080,
            replay_buffer_secs: 30,
            record_mic: false,
            display_capture: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct EventSettings {
    /// Event kinds that get an automatic short clip after the game.
    pub clip_kinds: Vec<EventKind>,
    pub clip_before_secs: f64,
    pub clip_after_secs: f64,
    /// Spoken callouts (Windows text-to-speech).
    pub tts_enabled: bool,
    pub tts_kinds: Vec<EventKind>,
    pub tts_volume: u8,
}

impl Default for EventSettings {
    fn default() -> Self {
        Self {
            clip_kinds: vec![EventKind::Multikill, EventKind::Ace],
            clip_before_secs: 10.0,
            clip_after_secs: 4.0,
            tts_enabled: false,
            tts_kinds: vec![EventKind::Kill, EventKind::Death, EventKind::Multikill, EventKind::Clip],
            tts_volume: 70,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub first_run_done: bool,
    /// Where games are saved. Empty = `<Videos>\GameRecorder`.
    pub save_dir: String,
    /// Delete games older than this many days (0 = never). Favorites are kept.
    pub auto_delete_days: u32,
    /// Keep total size under this many GB (0 = no limit). Oldest non-favorites go first.
    pub max_disk_gb: u32,
    pub hotkey_clip: String,
    pub hotkey_marker: String,
    pub auto_record: bool,
    /// Close the main window when a game starts (lightest option; off by default so you can
    /// alt-tab to the app during the loading screen).
    pub close_ui_in_game: bool,
    /// Keep the window loaded in the background when it's closed, so it reopens instantly
    /// (paused and trimmed while hidden). Off = closing frees the UI's memory completely.
    pub keep_ui_loaded: bool,
    /// Bumped when defaults change in a way existing settings files should pick up.
    #[serde(default)]
    pub settings_version: u32,
    pub start_with_windows: bool,
    pub start_minimized: bool,
    /// Show the CPU/RAM debug stat in the UI.
    pub show_perf: bool,
    pub obs: ObsSettings,
    pub video: VideoSettings,
    pub events: EventSettings,
    /// Per-game settings, keyed by game id (see `GameIntegration::config_fields`).
    pub games: BTreeMap<String, serde_json::Value>,
    /// Game ids that are switched off.
    pub disabled_games: Vec<String>,
    /// Path to ffmpeg.exe used for clip export (empty = auto).
    pub ffmpeg_path: String,
    /// "builtin" (GameRecorder's own recorder) or "obs" (OBS Studio as a backup).
    pub recorder: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            first_run_done: false,
            save_dir: String::new(),
            auto_delete_days: 0,
            max_disk_gb: 0,
            hotkey_clip: "F8".into(),
            hotkey_marker: "F9".into(),
            auto_record: true,
            close_ui_in_game: false,
            keep_ui_loaded: true,
            settings_version: SETTINGS_VERSION,
            start_with_windows: false,
            start_minimized: false,
            show_perf: true,
            obs: ObsSettings::default(),
            video: VideoSettings::default(),
            events: EventSettings::default(),
            games: BTreeMap::new(),
            disabled_games: Vec::new(),
            ffmpeg_path: String::new(),
            recorder: "builtin".into(),
        }
    }
}

pub const SETTINGS_VERSION: u32 = 2;

impl Settings {
    pub fn load(path: &Path) -> Self {
        let mut s = match std::fs::read_to_string(path) {
            Ok(t) => serde_json::from_str(&t).unwrap_or_else(|e| {
                log::warn!("settings file unreadable ({e}), using defaults");
                Settings::default()
            }),
            Err(_) => Settings::default(),
        };
        s.migrate();
        s
    }

    /// Brings older settings files up to date with changed defaults.
    pub fn migrate(&mut self) {
        if self.settings_version < 2 {
            // v2: the window stays available during games (alt-tab in the loading screen)
            // and reopens instantly.
            self.close_ui_in_game = false;
            self.keep_ui_loaded = true;
        }
        self.settings_version = SETTINGS_VERSION;
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(tmp, path)?;
        Ok(())
    }

    pub fn save_dir_or(&self, default: &Path) -> PathBuf {
        if self.save_dir.trim().is_empty() { default.to_path_buf() } else { PathBuf::from(&self.save_dir) }
    }

    pub fn game_config(&self, id: &str, default: serde_json::Value) -> serde_json::Value {
        let mut merged = default;
        if let (Some(obj), Some(user)) = (merged.as_object_mut(), self.games.get(id).and_then(|v| v.as_object())) {
            for (k, v) in user {
                obj.insert(k.clone(), v.clone());
            }
        }
        merged
    }
}

/// A hotkey like "Ctrl+Shift+F8".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hotkey {
    pub key: String,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Hotkey {
    pub fn parse(s: &str) -> Option<Hotkey> {
        let mut hk = Hotkey { key: String::new(), ctrl: false, shift: false, alt: false };
        for part in s.split('+').map(str::trim).filter(|p| !p.is_empty()) {
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => hk.ctrl = true,
                "shift" => hk.shift = true,
                "alt" => hk.alt = true,
                _ => hk.key = normalize_key(part),
            }
        }
        if hk.key.is_empty() { None } else { Some(hk) }
    }

    pub fn matches(&self, k: &KeyPress) -> bool {
        self.key.eq_ignore_ascii_case(&k.key) && self.ctrl == k.ctrl && self.shift == k.shift && self.alt == k.alt
    }
}

pub fn normalize_key(k: &str) -> String {
    let k = k.trim();
    if k.len() == 1 {
        k.to_ascii_uppercase()
    } else {
        let mut c = k.chars();
        match c.next() {
            Some(f) => f.to_ascii_uppercase().to_string() + &c.as_str().to_ascii_lowercase(),
            None => String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hotkeys() {
        let h = Hotkey::parse("Ctrl+Shift+f8").unwrap();
        assert_eq!(h.key, "F8");
        assert!(h.ctrl && h.shift && !h.alt);
        let k = KeyPress { key: "F8".into(), ctrl: true, shift: true, alt: false };
        assert!(h.matches(&k));
        let r = Hotkey::parse("r").unwrap();
        assert_eq!(r.key, "R");
        assert!(Hotkey::parse("Ctrl+").is_none());
    }

    #[test]
    fn defaults_fill_missing_fields() {
        let s: Settings = serde_json::from_str(r#"{"hotkey_clip":"F7","video":{"fps":30}}"#).unwrap();
        assert_eq!(s.hotkey_clip, "F7");
        assert_eq!(s.video.fps, 30);
        assert_eq!(s.video.replay_buffer_secs, 30);
        assert_eq!(s.obs.port, 4455);
        assert_eq!(s.recorder, "builtin");
    }

    #[test]
    fn old_files_get_the_new_window_defaults_once() {
        // A v1 file had "close the window when a game starts" on.
        let mut old: Settings = serde_json::from_str(r#"{"close_ui_in_game":true}"#).unwrap();
        assert_eq!(old.settings_version, 0);
        old.migrate();
        assert!(!old.close_ui_in_game && old.keep_ui_loaded);
        assert_eq!(old.settings_version, SETTINGS_VERSION);
        // After that, the user's own choice sticks.
        let mut chosen: Settings = serde_json::from_str(r#"{"close_ui_in_game":true,"keep_ui_loaded":false,"settings_version":2}"#).unwrap();
        chosen.migrate();
        assert!(chosen.close_ui_in_game && !chosen.keep_ui_loaded);
    }

    #[test]
    fn game_config_merge() {
        let mut s = Settings::default();
        s.games.insert("league".into(), serde_json::json!({"riot_id": "Me#EUW"}));
        let c = s.game_config("league", serde_json::json!({"riot_id": "", "ult_key": "R"}));
        assert_eq!(c["riot_id"], "Me#EUW");
        assert_eq!(c["ult_key"], "R");
    }
}
