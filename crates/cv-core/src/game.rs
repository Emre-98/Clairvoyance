//! The contract every supported game implements.
//!
//! To add a game, create a new crate under `games/` with a type implementing
//! [`GameIntegration`] and register it in `app/src/games.rs`. Nothing else in the
//! core, the engine or the UI needs to change. See `docs/ADDING_A_GAME.md`.

use crate::events::GameEvent;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// What the recorder should capture for this game.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureTarget {
    /// Executable name, e.g. "League of Legends.exe". The recorder captures its largest window.
    pub exe: String,
    /// Always capture the whole monitor instead of the game window.
    pub display_capture_only: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MatchPhase {
    /// Process is running but no match data yet (client, lobby, loading screen).
    #[default]
    Waiting,
    /// Match data exists but the clock hasn't started.
    Loading,
    InProgress,
    Ended,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameResult {
    Win,
    Loss,
    Draw,
}

impl GameResult {
    pub fn label(self) -> &'static str {
        match self {
            GameResult::Win => "Win",
            GameResult::Loss => "Loss",
            GameResult::Draw => "Draw",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PlayerInfo {
    /// In-game name (e.g. Riot ID).
    pub name: String,
    /// Champion / agent / hero display name, e.g. "Ahri".
    #[serde(default)]
    pub character: Option<String>,
    /// Stable id for images, e.g. "Ahri" or "MonkeyKing".
    #[serde(default)]
    pub character_id: Option<String>,
    #[serde(default)]
    pub team: Option<String>,
    /// Game mode / map, e.g. "Ranked Solo", "ARAM", "de_mirage".
    #[serde(default)]
    pub mode: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PlayerStats {
    pub kills: u32,
    pub deaths: u32,
    pub assists: u32,
    #[serde(default)]
    pub cs: Option<u32>,
    /// Total gold value (items + unspent) where the game exposes it.
    #[serde(default)]
    pub gold: Option<u32>,
    #[serde(default)]
    pub level: Option<u32>,
    #[serde(default)]
    pub vision_score: Option<f64>,
    /// Game-specific extra numbers shown in the summary (label -> value).
    #[serde(default)]
    pub extra: Vec<(String, String)>,
}

/// Result of one poll of the game's API.
#[derive(Debug, Clone, Default)]
pub struct PollUpdate {
    pub phase: MatchPhase,
    /// Current in-game clock in seconds, if known.
    pub game_time: Option<f64>,
    /// Only events not returned before.
    pub events: Vec<GameEvent>,
    pub player: Option<PlayerInfo>,
    pub stats: Option<PlayerStats>,
    pub result: Option<GameResult>,
}

/// A key press seen while the game window is focused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyPress {
    /// Key name, e.g. "R", "F8", "Enter", "Escape".
    pub key: String,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

/// A setting a game module exposes in the Settings page (rendered generically).
#[derive(Debug, Clone, Serialize)]
pub struct ConfigField {
    pub key: &'static str,
    pub label: &'static str,
    /// "text", "key" (single key picker) or "bool".
    pub kind: &'static str,
    pub help: &'static str,
}

#[async_trait]
pub trait GameIntegration: Send + Sync {
    /// Stable id used in settings and session files, e.g. "league".
    fn id(&self) -> &'static str;
    /// Display name, e.g. "League of Legends".
    fn name(&self) -> &'static str;
    /// Short name for file names, e.g. "League".
    fn short_name(&self) -> &'static str;
    /// Lower-case executable names that mean the game is running.
    fn process_names(&self) -> &'static [&'static str];
    fn capture(&self) -> CaptureTarget;
    /// False for games without an event API (they still get recording, clips and markers).
    fn supports_events(&self) -> bool;

    /// Game-specific settings (e.g. Riot ID, ult key).
    fn config_fields(&self) -> Vec<ConfigField> {
        Vec::new()
    }
    fn default_config(&self) -> serde_json::Value {
        serde_json::json!({})
    }
    fn configure(&mut self, _config: &serde_json::Value) {}

    /// Called when the game process is detected, before the first poll.
    async fn start(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
    /// Called about once per second while the game runs.
    async fn poll(&mut self) -> anyhow::Result<PollUpdate>;
    /// Called when the game process is gone. Must forget everything from this match.
    async fn stop(&mut self) {}

    /// True for games you keep open between matches (e.g. CS2): recording starts when a
    /// match starts (phase Loading/InProgress) instead of when the process starts, and
    /// stops when the match ends or you leave it.
    fn match_only(&self) -> bool {
        false
    }

    fn poll_interval(&self) -> Duration {
        Duration::from_secs(1)
    }
    /// How long to keep recording after the match ends (victory screen).
    fn end_grace(&self) -> Duration {
        Duration::from_secs(6)
    }

    /// Key presses while the game is focused and in progress (e.g. ult key).
    /// Return an event to put on the timeline. `game_time` is the estimated clock.
    fn on_key(&mut self, _key: &KeyPress, _game_time: f64) -> Option<GameEvent> {
        None
    }

    /// Groups for Settings > Game modes. Empty = this game has no per-mode recording rules.
    fn mode_groups(&self) -> Vec<crate::modes::ModeGroupInfo> {
        Vec::new()
    }
    /// The mode of the match that is starting. Called once, right after `start()` and before
    /// recording starts, so a mode that's switched off is never recorded. Keep it quick.
    async fn detect_mode(&mut self) -> Option<crate::modes::MatchMode> {
        None
    }
    /// The modes that exist, for the settings list (from live and cached sources; must work
    /// offline from `cache_dir`). The bool says whether the list is the authoritative set of
    /// modes playable right now (modes missing from it are then shown as unavailable).
    /// Never called during a game.
    async fn mode_catalog(&mut self, _cache_dir: &std::path::Path) -> (Vec<crate::modes::CatalogMode>, bool) {
        (Vec::new(), false)
    }
}
