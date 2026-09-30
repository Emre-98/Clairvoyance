//! One recorded game. Saved as `session.json` in the game's folder, next to the video,
//! so an old game reopens with its timeline intact.

use crate::events::GameEvent;
use crate::game::{GameResult, PlayerInfo, PlayerStats};
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const SESSION_FILE: &str = "session.json";
pub const THUMB_FILE: &str = "thumb.jpg";
pub const CLIPS_DIR: &str = "clips";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipInfo {
    /// File name inside the session's `clips` folder.
    pub file: String,
    pub title: String,
    /// Start/end in the full recording (seconds), if known.
    #[serde(default)]
    pub video_start: Option<f64>,
    #[serde(default)]
    pub video_end: Option<f64>,
    pub created_at: DateTime<Local>,
    /// "replay" (hotkey), "event" (auto clip) or "editor".
    pub source: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PerfStats {
    pub samples: u32,
    pub cpu_avg: f64,
    pub cpu_max: f64,
    pub ram_avg_mb: f64,
    pub ram_max_mb: f64,
}

impl PerfStats {
    pub fn add(&mut self, cpu: f64, ram_mb: f64) {
        let n = self.samples as f64;
        self.cpu_avg = (self.cpu_avg * n + cpu) / (n + 1.0);
        self.ram_avg_mb = (self.ram_avg_mb * n + ram_mb) / (n + 1.0);
        self.cpu_max = self.cpu_max.max(cpu);
        self.ram_max_mb = self.ram_max_mb.max(ram_mb);
        self.samples += 1;
    }
}

/// Periodic snapshot for the post-game graphs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatSample {
    pub t: f64,
    pub kills: u32,
    pub deaths: u32,
    pub assists: u32,
    #[serde(default)]
    pub cs: Option<u32>,
    #[serde(default)]
    pub gold: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameSession {
    pub version: u32,
    /// Folder name, unique.
    pub id: String,
    pub game_id: String,
    pub game_name: String,
    pub started_at: DateTime<Local>,
    #[serde(default)]
    pub ended_at: Option<DateTime<Local>>,
    /// Video file name inside the session folder.
    #[serde(default)]
    pub video_file: Option<String>,
    /// Video position (seconds) at the moment the in-game clock was 0:00.
    /// The recording starts before the game clock (loading screen), so:
    ///   video position = event game time + video_offset
    #[serde(default)]
    pub video_offset: f64,
    /// Length of the recording in seconds, if known.
    #[serde(default)]
    pub video_duration: Option<f64>,
    /// In-game clock at the end.
    #[serde(default)]
    pub game_duration: Option<f64>,
    #[serde(default)]
    pub player: Option<PlayerInfo>,
    #[serde(default)]
    pub stats: Option<PlayerStats>,
    #[serde(default)]
    pub result: Option<GameResult>,
    #[serde(default)]
    pub events: Vec<GameEvent>,
    #[serde(default)]
    pub clips: Vec<ClipInfo>,
    #[serde(default)]
    pub timeline: Vec<StatSample>,
    #[serde(default)]
    pub perf: PerfStats,
    #[serde(default)]
    pub favorite: bool,
    /// Non-fatal problems during the game (shown on the detail page).
    #[serde(default)]
    pub warnings: Vec<String>,
}

impl GameSession {
    pub fn new(id: String, game_id: &str, game_name: &str, started_at: DateTime<Local>) -> Self {
        Self {
            version: 1,
            id,
            game_id: game_id.to_string(),
            game_name: game_name.to_string(),
            started_at,
            ended_at: None,
            video_file: None,
            video_offset: 0.0,
            video_duration: None,
            game_duration: None,
            player: None,
            stats: None,
            result: None,
            events: Vec::new(),
            clips: Vec::new(),
            timeline: Vec::new(),
            perf: PerfStats::default(),
            favorite: false,
            warnings: Vec::new(),
        }
    }

    pub fn to_video_position(&self, game_time: f64) -> f64 {
        game_time + self.video_offset
    }

    pub fn load(dir: &Path) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(dir.join(SESSION_FILE))?;
        Ok(serde_json::from_str(&text)?)
    }

    /// Atomic save (write temp file, then rename) so a crash never leaves half a file.
    pub fn save(&self, dir: &Path) -> anyhow::Result<()> {
        std::fs::create_dir_all(dir)?;
        let tmp = dir.join(format!("{SESSION_FILE}.tmp"));
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(&tmp, dir.join(SESSION_FILE))?;
        Ok(())
    }

    /// Final video name, e.g. `2026-09-30_League_Ahri_Win.mp4`.
    pub fn video_name(&self, short_game: &str, ext: &str) -> String {
        let mut parts = vec![self.started_at.format("%Y-%m-%d").to_string(), sanitize(short_game)];
        if let Some(c) = self.player.as_ref().and_then(|p| p.character.as_deref()) {
            parts.push(sanitize(c));
        }
        if let Some(r) = self.result {
            parts.push(r.label().to_string());
        }
        format!("{}.{}", parts.join("_"), ext.trim_start_matches('.'))
    }
}

/// Folder name for a new game, e.g. `2026-09-30_21-14_League`.
pub fn session_folder_name(started: DateTime<Local>, short_game: &str) -> String {
    format!("{}_{}", started.format("%Y-%m-%d_%H-%M-%S"), sanitize(short_game))
}

/// Removes characters that aren't allowed in Windows file names.
pub fn sanitize(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '.' { c } else if c == ' ' || c == '_' { '-' } else { '\0' })
        .filter(|c| *c != '\0')
        .collect();
    let trimmed = cleaned.trim_matches(|c| c == '.' || c == '-').to_string();
    if trimmed.is_empty() { "Unknown".into() } else { trimmed }
}

/// Picks a path that doesn't exist yet by appending `_2`, `_3`, ...
pub fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) => (s.to_string(), format!(".{e}")),
        None => (name.to_string(), String::new()),
    };
    for i in 2.. {
        let p = dir.join(format!("{stem}_{i}{ext}"));
        if !p.exists() {
            return p;
        }
    }
    unreachable!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn names() {
        let t = Local.with_ymd_and_hms(2026, 9, 30, 21, 14, 5).unwrap();
        let mut s = GameSession::new("x".into(), "league", "League of Legends", t);
        s.player = Some(PlayerInfo { character: Some("Kai'Sa".into()), ..Default::default() });
        s.result = Some(GameResult::Win);
        assert_eq!(s.video_name("League", "mp4"), "2026-09-30_League_KaiSa_Win.mp4");
        assert_eq!(session_folder_name(t, "League"), "2026-09-30_21-14-05_League");
        assert_eq!(sanitize("Nunu & Willump"), "Nunu--Willump");
        assert_eq!(sanitize("???"), "Unknown");
    }

    #[test]
    fn offset() {
        let mut s = GameSession::new("x".into(), "g", "G", Local::now());
        s.video_offset = 42.5;
        assert_eq!(s.to_video_position(100.0), 142.5);
    }

    #[test]
    fn perf_avg() {
        let mut p = PerfStats::default();
        p.add(1.0, 50.0);
        p.add(3.0, 70.0);
        assert_eq!(p.cpu_avg, 2.0);
        assert_eq!(p.ram_max_mb, 70.0);
    }

    #[test]
    fn roundtrip() {
        let dir = std::env::temp_dir().join(format!("cv-test-{}", std::process::id()));
        let s = GameSession::new("abc".into(), "league", "League of Legends", Local::now());
        s.save(&dir).unwrap();
        let l = GameSession::load(&dir).unwrap();
        assert_eq!(l.id, "abc");
        std::fs::remove_dir_all(dir).ok();
    }
}
