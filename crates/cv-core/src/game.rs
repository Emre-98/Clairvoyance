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

/// Watching instead of playing: never recorded as a game (spectating can be allowed, see
/// [`GameIntegration::record_spectating`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WatchKind {
    /// A replay (League: a `.rofl` file, "Watch" in match history).
    Replay,
    /// Spectating someone else's live game.
    Spectate,
    /// Spectator mode, but which of the two isn't known.
    Unknown,
}

impl WatchKind {
    /// For the tray / status: "Replay: not recorded".
    pub fn label(self) -> &'static str {
        match self {
            WatchKind::Replay => "Replay",
            WatchKind::Spectate => "Spectating",
            WatchKind::Unknown => "Replay or spectating",
        }
    }
}

/// What the game module can tell about a game process that just started, before anything is
/// recorded ([`GameIntegration::session_check`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionCheck {
    /// A match you play (League: the client has a game session in progress).
    Playing,
    /// Watching a replay / spectating: nothing is recorded.
    Watching(WatchKind),
    /// Most likely watching (League: the client has no game session at all), but not proven:
    /// nothing is recorded until the game's own API says one way or the other
    /// ([`PollUpdate::watching`] / [`PollUpdate::playing`]).
    Unsure(WatchKind),
    /// Can't tell (e.g. the League client can't be reached): recorded; the game's API is
    /// checked once it answers and a replay found then is deleted.
    Unknown,
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
    /// The game's API says this is spectator mode (a replay or spectating), not a match you play.
    pub watching: Option<WatchKind>,
    /// The game's API confirms you're playing (League: your champion's data is there).
    pub playing: bool,
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

/// One press of a tracked game key (League: the ult), kept with the recording whether or not
/// it became a timeline event, so the check against the video can decide afterwards.
#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
pub struct KeyMark {
    /// In-game clock (seconds).
    pub game_time: f64,
    /// What the key does, e.g. "ult".
    pub action: String,
    /// The key as pressed, e.g. "Shift+R".
    pub key: String,
    /// True if it became a timeline event live.
    pub accepted: bool,
    /// Why it was filtered out live: "cooldown", "not_learned", "dead", "chat", ...
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// A rectangle in video pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct Region {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// An RGB image (3 bytes per pixel, rows top to bottom).
#[derive(Debug, Clone, PartialEq)]
pub struct Rgb {
    pub w: u32,
    pub h: u32,
    pub data: Vec<u8>,
}

impl Rgb {
    pub fn px(&self, x: u32, y: u32) -> [u8; 3] {
        let i = ((y * self.w + x) * 3) as usize;
        [self.data[i], self.data[i + 1], self.data[i + 2]]
    }
    /// Reads a binary PPM (P6), the format of the detector test images.
    pub fn from_ppm(b: &[u8]) -> Option<Rgb> {
        let mut fields = Vec::new();
        let mut i = 0;
        while fields.len() < 4 && i < b.len() {
            while i < b.len() && b[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < b.len() && b[i] == b'#' {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            let start = i;
            while i < b.len() && !b[i].is_ascii_whitespace() {
                i += 1;
            }
            fields.push(String::from_utf8_lossy(&b[start..i]).to_string());
        }
        if fields.first().map(|s| s.as_str()) != Some("P6") || fields.len() < 4 {
            return None;
        }
        let (w, h): (u32, u32) = (fields[1].parse().ok()?, fields[2].parse().ok()?);
        let data = b.get(i + 1..i + 1 + (w * h * 3) as usize)?.to_vec();
        Some(Rgb { w, h, data })
    }
    pub fn to_ppm(&self) -> Vec<u8> {
        let mut v = format!("P6\n{} {}\n255\n", self.w, self.h).into_bytes();
        v.extend_from_slice(&self.data);
        v
    }
    pub fn crop(&self, r: Region) -> Rgb {
        let mut data = Vec::with_capacity((r.w * r.h * 3) as usize);
        for y in r.y..(r.y + r.h).min(self.h) {
            let a = ((y * self.w + r.x) * 3) as usize;
            let b = ((y * self.w + (r.x + r.w).min(self.w)) * 3) as usize;
            data.extend_from_slice(&self.data[a..b]);
        }
        Rgb { w: r.w.min(self.w - r.x), h: r.h.min(self.h - r.y), data }
    }
}

/// Decoded frames of a finished recording, for checking events against the video after the
/// game (only ever used by the maintenance pass, never during a game).
pub trait FrameSource {
    /// Video size in pixels.
    fn size(&self) -> (u32, u32);
    fn duration(&self) -> f64;
    /// Keyframe times (seconds); decoding one of these needs no other frame.
    fn keyframes(&self) -> &[f64];
    /// The frame at (or just after) `t`, cropped to `region`. Fast when `t` is a keyframe.
    fn frame_at(&mut self, t: f64, region: Region) -> anyhow::Result<Option<(f64, Rgb)>>;
    /// Every frame from `t0` to `t1`, cropped; `f` returns false to stop early.
    fn frames(&mut self, t0: f64, t1: f64, region: Region, f: &mut dyn FnMut(f64, &Rgb) -> bool) -> anyhow::Result<()>;
}

/// A setting a game module exposes in the Settings page (rendered generically).
#[derive(Debug, Clone, Serialize)]
pub struct ConfigField {
    pub key: &'static str,
    pub label: &'static str,
    /// "text", "key" (single key picker), "bool" or "select" (one of `options`).
    pub kind: &'static str,
    pub help: &'static str,
    /// Choices for "select": (value, label).
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    pub options: &'static [(&'static str, &'static str)],
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
    /// Presses of tracked keys since the last call (accepted or filtered), kept in the session.
    fn take_key_marks(&mut self) -> Vec<KeyMark> {
        Vec::new()
    }

    // Optional capabilities. A game that has one returns `Some(self)` and implements its trait;
    // the defaults (`None`) mean the game doesn't do that (CS2 has none of them).

    /// Mouse and keyboard recording for cursor-based games (League): the replay overlay, ability
    /// bubbles and Mechanics stats (Settings > Games: "Record mouse & keyboard input"). `None`
    /// (e.g. CS2: mouse-look, no cursor) = input is never recorded.
    fn cursor_input(&self) -> Option<&dyn CursorInput> {
        None
    }
    /// Checks the live events against the finished recording after the game (League: ult casts
    /// read from the ability bar).
    fn recording_check(&self) -> Option<&dyn RecordingCheck> {
        None
    }
    /// Tells a match you play from a replay / spectating before anything is recorded. `None` =
    /// every game process is recorded.
    fn watch_detection(&mut self) -> Option<&mut dyn WatchDetection> {
        None
    }
    /// Per-mode recording rules (Settings > Game modes: Record / Clips only / Off per queue).
    fn mode_rules(&mut self) -> Option<&mut dyn ModeRules> {
        None
    }
}

/// See [`GameIntegration::cursor_input`].
pub trait CursorInput: Send + Sync {
    /// The game's chat is open right now (from the keys seen by [`GameIntegration::on_key`]): no
    /// keys are recorded meanwhile.
    fn chat_open(&self) -> bool {
        false
    }
    /// After the game, before `stop()`: the mouse-button presses of the input recording (game
    /// time, button 1 = left, 2 = right, 3 = middle, 4/5 = side buttons). Return marks for the
    /// ones bound to a tracked action (League: ult on a mouse button), kept with the session for
    /// the check against the recording.
    fn mouse_marks(&self, _presses: &[(f64, u8)]) -> Vec<KeyMark> {
        Vec::new()
    }

    /// The game's "action keys" for the replay's ability bubbles (League: abilities, summoners,
    /// item slots, ward) with the binds in effect now. Called after `start()` when the input
    /// recording starts; saved with the session (binds can change between games). Empty = this
    /// game has no action keys.
    fn action_keys(&self) -> Vec<crate::input::actions::ActionKey> {
        Vec::new()
    }
    /// The same with the game's default binds: used for recordings made before the binds were
    /// saved with the session.
    fn default_action_keys(&self) -> Vec<crate::input::actions::ActionKey> {
        Vec::new()
    }
    /// Sub-toggles of the overlay's "Ability bubbles" option, one per category of
    /// [`Self::action_keys`].
    fn action_categories(&self) -> Vec<crate::input::actions::ActionCategory> {
        Vec::new()
    }
    /// Refines how presses are drawn from what the game knows about this recording (League: the
    /// ult check against the recording: "Ult used" solid, "no cast" faded). Never during a game.
    fn action_press_states(&self, _session: &crate::session::GameSession, _actions: &[crate::input::actions::ActionKey], _presses: &mut [crate::input::actions::ActionPress]) {}
}

/// See [`GameIntegration::recording_check`].
pub trait RecordingCheck: Send + Sync {
    /// After the game (maintenance pass, low priority): check the session's live events
    /// against the recording and correct them. `cancel` turns true when a game starts: stop and
    /// return an error.
    fn verify_recording(&self, session: &mut crate::session::GameSession, video: &mut dyn FrameSource, cancel: &dyn Fn() -> bool) -> anyhow::Result<()>;
    /// Version of `verify_recording` (at least 1): recordings checked by an older version are
    /// checked again.
    fn verify_version(&self) -> u32;
}

/// See [`GameIntegration::watch_detection`].
#[async_trait]
pub trait WatchDetection: Send + Sync {
    /// Called once when the game process appears, before anything is recorded: is this a match
    /// you play, or a replay / spectating? Keep it quick (League: two local requests to the
    /// League client).
    async fn session_check(&mut self) -> SessionCheck;
    /// Record games you spectate (setting); replays are never recorded.
    fn record_spectating(&self) -> bool {
        false
    }
}

/// See [`GameIntegration::mode_rules`].
#[async_trait]
pub trait ModeRules: Send + Sync {
    /// Groups for Settings > Game modes (at least one).
    fn mode_groups(&self) -> Vec<crate::modes::ModeGroupInfo>;
    /// The mode of the match that is starting. Called once, right after `start()` and before
    /// recording starts, so a mode that's switched off is never recorded. Keep it quick.
    async fn detect_mode(&mut self) -> Option<crate::modes::MatchMode>;
    /// The modes that exist, for the settings list (from live and cached sources; must work
    /// offline from `cache_dir`). The bool says whether the list is the authoritative set of
    /// modes playable right now (modes missing from it are then shown as unavailable).
    /// Never called during a game. Default: no list (modes are added as games are played).
    async fn mode_catalog(&mut self, _cache_dir: &std::path::Path) -> (Vec<crate::modes::CatalogMode>, bool) {
        (Vec::new(), false)
    }
}
