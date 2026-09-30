//! The game library: every sub-folder of the save folder that has a `session.json`.

use crate::game::{GameResult, PlayerInfo, PlayerStats};
use crate::session::{GameSession, CLIPS_DIR, SESSION_FILE, THUMB_FILE};
use chrono::{DateTime, Local};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct SessionSummary {
    pub id: String,
    pub dir: PathBuf,
    pub game_id: String,
    pub game_name: String,
    pub started_at: DateTime<Local>,
    pub player: Option<PlayerInfo>,
    pub stats: Option<PlayerStats>,
    pub result: Option<GameResult>,
    pub video_path: Option<PathBuf>,
    pub thumb_path: Option<PathBuf>,
    pub duration: Option<f64>,
    pub favorite: bool,
    pub size_bytes: u64,
    pub event_count: usize,
    pub clip_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClipEntry {
    pub session_id: String,
    pub game_name: String,
    pub character: Option<String>,
    pub character_id: Option<String>,
    pub path: PathBuf,
    pub title: String,
    pub created_at: DateTime<Local>,
    pub size_bytes: u64,
    pub source: String,
}

pub fn dir_size(dir: &Path) -> u64 {
    let mut total = 0;
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            match e.file_type() {
                Ok(t) if t.is_dir() => total += dir_size(&e.path()),
                Ok(_) => total += e.metadata().map(|m| m.len()).unwrap_or(0),
                Err(_) => {}
            }
        }
    }
    total
}

pub fn summarize(dir: &Path, s: &GameSession) -> SessionSummary {
    let video_path = s.video_file.as_ref().map(|f| dir.join(f)).filter(|p| p.exists());
    let thumb = dir.join(THUMB_FILE);
    SessionSummary {
        id: s.id.clone(),
        dir: dir.to_path_buf(),
        game_id: s.game_id.clone(),
        game_name: s.game_name.clone(),
        started_at: s.started_at,
        player: s.player.clone(),
        stats: s.stats.clone(),
        result: s.result,
        video_path,
        thumb_path: thumb.exists().then_some(thumb),
        duration: s.game_duration.or(s.video_duration),
        favorite: s.favorite,
        size_bytes: dir_size(dir),
        event_count: s.events.len(),
        clip_count: s.clips.len(),
    }
}

/// All sessions in the save folder, newest first.
pub fn scan(root: &Path) -> Vec<(PathBuf, GameSession)> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(root) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() && p.join(SESSION_FILE).exists() {
                match GameSession::load(&p) {
                    Ok(s) => out.push((p, s)),
                    Err(err) => log::warn!("skipping {}: {err}", p.display()),
                }
            }
        }
    }
    out.sort_by(|a, b| b.1.started_at.cmp(&a.1.started_at));
    out
}

pub fn list_summaries(root: &Path) -> Vec<SessionSummary> {
    scan(root).iter().map(|(d, s)| summarize(d, s)).collect()
}

pub fn list_clips(root: &Path) -> Vec<ClipEntry> {
    let mut out = Vec::new();
    for (dir, s) in scan(root) {
        for c in &s.clips {
            let path = dir.join(CLIPS_DIR).join(&c.file);
            if let Ok(meta) = std::fs::metadata(&path) {
                out.push(ClipEntry {
                    session_id: s.id.clone(),
                    game_name: s.game_name.clone(),
                    character: s.player.as_ref().and_then(|p| p.character.clone()),
                    character_id: s.player.as_ref().and_then(|p| p.character_id.clone()),
                    path,
                    title: c.title.clone(),
                    created_at: c.created_at,
                    size_bytes: meta.len(),
                    source: c.source.clone(),
                });
            }
        }
    }
    out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    out
}

/// Deletes old games. Favorites and `protect` (the game being recorded) are never deleted.
/// Returns the ids that were removed.
pub fn apply_retention(root: &Path, max_age_days: u32, max_gb: u32, now: DateTime<Local>, protect: Option<&str>) -> Vec<String> {
    let mut removed = Vec::new();
    if max_age_days == 0 && max_gb == 0 {
        return removed;
    }
    // Oldest first.
    let mut sessions: Vec<(PathBuf, GameSession, u64)> =
        scan(root).into_iter().map(|(d, s)| { let size = dir_size(&d); (d, s, size) }).collect();
    sessions.reverse();
    let deletable = |s: &GameSession| !s.favorite && Some(s.id.as_str()) != protect;

    let mut keep = Vec::new();
    for (d, s, size) in sessions {
        let too_old = max_age_days > 0 && (now - s.started_at).num_days() >= max_age_days as i64;
        if too_old && deletable(&s) {
            if std::fs::remove_dir_all(&d).is_ok() {
                removed.push(s.id.clone());
            }
        } else {
            keep.push((d, s, size));
        }
    }
    if max_gb > 0 {
        let limit = max_gb as u64 * 1024 * 1024 * 1024;
        let mut total: u64 = keep.iter().map(|k| k.2).sum();
        for (d, s, size) in &keep {
            if total <= limit {
                break;
            }
            if deletable(s) && std::fs::remove_dir_all(d).is_ok() {
                total = total.saturating_sub(*size);
                removed.push(s.id.clone());
            }
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn make(root: &Path, id: &str, age_days: i64, fav: bool, bytes: usize) {
        let dir = root.join(id);
        let mut s = GameSession::new(id.into(), "league", "League of Legends", Local::now() - Duration::days(age_days));
        s.favorite = fav;
        s.save(&dir).unwrap();
        std::fs::write(dir.join("v.mp4"), vec![0u8; bytes]).unwrap();
    }

    #[test]
    fn retention_by_age_keeps_favorites() {
        let root = std::env::temp_dir().join(format!("gr-lib-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        make(&root, "old", 40, false, 10);
        make(&root, "oldfav", 40, true, 10);
        make(&root, "new", 1, false, 10);
        let removed = apply_retention(&root, 30, 0, Local::now(), None);
        assert_eq!(removed, vec!["old".to_string()]);
        let ids: Vec<_> = list_summaries(&root).into_iter().map(|s| s.id).collect();
        assert_eq!(ids, vec!["new".to_string(), "oldfav".to_string()]);
        std::fs::remove_dir_all(root).ok();
    }
}
