//! The game library: every sub-folder of the save folder that has a `session.json`.
//!
//! - [`LibraryIndex`] keeps a summary of every game in memory and in a small cache file, so the
//!   app never rescans the disk when it opens. `refresh` is incremental: only folders whose
//!   `session.json`, folder or `clips` folder changed are read again.
//! - [`ThumbStore`] is the thumbnail folder (`<app data>\Thumbnails`), one small JPEG per
//!   recording (`<game id>.jpg`, the id being the game's folder name) and per clip
//!   (`<game id>@<clip file name>.jpg`).
//! - [`plan_cleanup`] / [`apply_cleanup`] keep the recordings under the storage limit.

use crate::game::{GameResult, PlayerInfo, PlayerStats};
use crate::session::{sanitize, GameSession, CLIPS_DIR, LEGACY_THUMB_FILE, SESSION_FILE};
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, RwLock};
use std::time::UNIX_EPOCH;

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// Game-level "favorite / keep": never auto-deleted.
    pub favorite: bool,
    /// Some of its clips are marked "keep" (the clean-up may still remove the full video).
    #[serde(default)]
    pub kept_clips: usize,
    /// The full video was removed by the clean-up (clips kept).
    #[serde(default)]
    pub video_removed: bool,
    pub size_bytes: u64,
    #[serde(default)]
    pub video_bytes: u64,
    /// Size of the mouse/keyboard recording (replay overlay); 0 = none.
    #[serde(default)]
    pub input_bytes: u64,
    pub event_count: usize,
    pub clip_count: usize,
    /// Where in the video a thumbnail should be taken (seconds).
    #[serde(default)]
    pub thumb_at: f64,
    /// Queue / mode, e.g. 420 "Ranked Solo/Duo" (older recordings: the coarse mode name).
    #[serde(default)]
    pub queue_id: Option<i64>,
    #[serde(default)]
    pub mode_name: Option<String>,
    #[serde(default)]
    pub mode_key: Option<String>,
    /// "full" or "clips_only".
    #[serde(default)]
    pub record_mode: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipEntry {
    pub session_id: String,
    pub game_name: String,
    pub character: Option<String>,
    pub character_id: Option<String>,
    pub path: PathBuf,
    /// File name inside the game's `clips` folder.
    #[serde(default)]
    pub file: String,
    pub title: String,
    pub created_at: DateTime<Local>,
    pub size_bytes: u64,
    pub source: String,
    #[serde(default)]
    pub keep: bool,
    #[serde(default)]
    pub thumb_path: Option<PathBuf>,
    /// Clip length (seconds), if known.
    #[serde(default)]
    pub duration: Option<f64>,
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

/// The thumbnail folder.
#[derive(Debug, Clone)]
pub struct ThumbStore {
    pub dir: PathBuf,
}

impl ThumbStore {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn session(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.jpg"))
    }

    pub fn clip(&self, id: &str, file: &str) -> PathBuf {
        let stem = file.rsplit_once('.').map(|(s, _)| s).unwrap_or(file);
        self.dir.join(format!("{id}@{}.jpg", sanitize(stem)))
    }

    /// Removes the game's thumbnail and all of its clips' thumbnails.
    pub fn remove_session(&self, id: &str) {
        let _ = std::fs::remove_file(self.session(id));
        let prefix = format!("{id}@");
        if let Ok(rd) = std::fs::read_dir(&self.dir) {
            for e in rd.flatten() {
                if e.file_name().to_string_lossy().starts_with(&prefix) {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
    }

    pub fn remove_clip(&self, id: &str, file: &str) {
        let _ = std::fs::remove_file(self.clip(id, file));
    }

    /// Moves a thumbnail an older version kept inside the game folder. Returns true if moved.
    pub fn adopt_legacy(&self, id: &str, dir: &Path) -> bool {
        let old = dir.join(LEGACY_THUMB_FILE);
        if !old.is_file() {
            return false;
        }
        let new = self.session(id);
        let _ = std::fs::create_dir_all(&self.dir);
        if new.exists() {
            let _ = std::fs::remove_file(&old);
            return false;
        }
        std::fs::rename(&old, &new).is_ok() || (std::fs::copy(&old, &new).is_ok() && std::fs::remove_file(&old).is_ok())
    }

    /// Deletes thumbnails whose game no longer exists. Returns how many were removed.
    pub fn remove_orphans(&self, live_ids: &[String]) -> usize {
        let keep: std::collections::HashSet<&str> = live_ids.iter().map(String::as_str).collect();
        let mut n = 0;
        if let Ok(rd) = std::fs::read_dir(&self.dir) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                let Some(stem) = name.strip_suffix(".jpg") else { continue };
                // "<id>" or "<id>@<clip>" (clip names never contain '@').
                let owner = stem.rsplit_once('@').map(|(id, _)| id);
                let live = keep.contains(stem) || owner.is_some_and(|id| keep.contains(id));
                if !live && std::fs::remove_file(e.path()).is_ok() {
                    n += 1;
                }
            }
        }
        n
    }
}

fn mtime_ns(p: &Path) -> u128 {
    std::fs::metadata(p).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_nanos()).unwrap_or(0)
}

/// What a folder looked like when it was last read. If it's unchanged, the cached summary is used.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
struct Signature {
    session_mtime: u128,
    session_len: u64,
    dir_mtime: u128,
    clips_mtime: u128,
    video_len: u64,
}

fn signature(dir: &Path, video: Option<&Path>) -> Signature {
    let sj = dir.join(SESSION_FILE);
    Signature {
        session_mtime: mtime_ns(&sj),
        session_len: std::fs::metadata(&sj).map(|m| m.len()).unwrap_or(0),
        dir_mtime: mtime_ns(dir),
        clips_mtime: mtime_ns(&dir.join(CLIPS_DIR)),
        video_len: video.and_then(|v| std::fs::metadata(v).ok()).map(|m| m.len()).unwrap_or(0),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    sig: Signature,
    summary: SessionSummary,
    clips: Vec<ClipEntry>,
}

fn read_entry(dir: &Path, s: &GameSession, thumbs: &ThumbStore) -> Entry {
    let video_path = s.video_file.as_ref().map(|f| dir.join(f)).filter(|p| p.exists());
    let video_bytes = video_path.as_ref().and_then(|v| std::fs::metadata(v).ok()).map(|m| m.len()).unwrap_or(0);
    let sig = signature(dir, video_path.as_deref());
    let thumb = thumbs.session(&s.id);
    let character = s.player.as_ref().and_then(|p| p.character.clone());
    let character_id = s.player.as_ref().and_then(|p| p.character_id.clone());
    let mut clips = Vec::new();
    for c in &s.clips {
        let path = dir.join(CLIPS_DIR).join(&c.file);
        if let Ok(meta) = std::fs::metadata(&path) {
            let t = thumbs.clip(&s.id, &c.file);
            clips.push(ClipEntry {
                session_id: s.id.clone(),
                game_name: s.game_name.clone(),
                character: character.clone(),
                character_id: character_id.clone(),
                path,
                file: c.file.clone(),
                title: c.title.clone(),
                created_at: c.created_at,
                size_bytes: meta.len(),
                source: c.source.clone(),
                keep: c.keep,
                thumb_path: t.exists().then_some(t),
                duration: match (c.video_start, c.video_end) {
                    (Some(a), Some(b)) if b > a => Some(b - a),
                    _ => None,
                },
            });
        }
    }
    let video_len = s.video_duration.unwrap_or(0.0);
    // ~90 s into the match (the classic "action has started" moment), or a third in for short games.
    let thumb_at = if video_len > 0.0 { (s.video_offset + 90.0).min(video_len / 3.0).max(0.0) } else { s.video_offset + 90.0 };
    let summary = SessionSummary {
        id: s.id.clone(),
        dir: dir.to_path_buf(),
        game_id: s.game_id.clone(),
        game_name: s.game_name.clone(),
        started_at: s.started_at,
        player: s.player.clone(),
        stats: s.stats.clone(),
        result: s.result,
        thumb_path: thumb.exists().then_some(thumb),
        video_path,
        duration: s.game_duration.or(s.video_duration),
        favorite: s.favorite,
        kept_clips: s.clips.iter().filter(|c| c.keep).count(),
        video_removed: s.video_removed_at.is_some(),
        size_bytes: dir_size(dir),
        video_bytes,
        input_bytes: s.input_file.as_ref().and_then(|f| std::fs::metadata(dir.join(f)).ok()).map(|m| m.len()).unwrap_or(0),
        event_count: s.events.len(),
        clip_count: clips.len(),
        thumb_at,
        queue_id: s.queue_id,
        mode_name: s.mode_name.clone().or_else(|| s.player.as_ref().and_then(|p| p.mode.clone())),
        mode_key: s.mode_key.clone(),
        record_mode: s.record_mode.clone(),
    };
    Entry { sig, summary, clips }
}

#[derive(Default, Serialize, Deserialize)]
struct CacheFile {
    version: u32,
    root: PathBuf,
    entries: Vec<Entry>,
}

const CACHE_VERSION: u32 = 2;

/// In-memory library, persisted to a cache file, refreshed incrementally.
pub struct LibraryIndex {
    cache_file: PathBuf,
    thumbs: ThumbStore,
    state: RwLock<IndexState>,
    /// Serializes refreshes (one walk at a time).
    refreshing: Mutex<()>,
}

#[derive(Default)]
struct IndexState {
    root: PathBuf,
    entries: HashMap<String, Entry>,
    loaded: bool,
    refreshed_once: bool,
}

impl LibraryIndex {
    pub fn new(cache_file: PathBuf, thumbs: ThumbStore) -> Self {
        Self { cache_file, thumbs, state: RwLock::new(IndexState::default()), refreshing: Mutex::new(()) }
    }

    pub fn thumbs(&self) -> &ThumbStore {
        &self.thumbs
    }

    /// Loads the cache file (fast) if nothing is loaded for `root` yet.
    pub fn ensure_loaded(&self, root: &Path) {
        {
            let st = self.state.read().unwrap();
            if st.loaded && st.root == root {
                return;
            }
        }
        let cached: Option<CacheFile> = std::fs::read(&self.cache_file).ok().and_then(|b| serde_json::from_slice(&b).ok());
        let mut st = self.state.write().unwrap();
        st.root = root.to_path_buf();
        st.entries = match cached {
            Some(c) if c.version == CACHE_VERSION && c.root == root => c.entries.into_iter().map(|e| (e.summary.id.clone(), e)).collect(),
            _ => HashMap::new(),
        };
        st.loaded = true;
        st.refreshed_once = false;
    }

    /// True until the first `refresh` for the current folder has run.
    pub fn needs_first_refresh(&self) -> bool {
        !self.state.read().unwrap().refreshed_once
    }

    /// All games, newest first.
    pub fn summaries(&self) -> Vec<SessionSummary> {
        let st = self.state.read().unwrap();
        let mut v: Vec<SessionSummary> = st.entries.values().map(|e| e.summary.clone()).collect();
        v.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        v
    }

    pub fn summary(&self, id: &str) -> Option<SessionSummary> {
        self.state.read().unwrap().entries.get(id).map(|e| e.summary.clone())
    }

    /// All clips, newest first.
    pub fn clips(&self) -> Vec<ClipEntry> {
        let st = self.state.read().unwrap();
        let mut v: Vec<ClipEntry> = st.entries.values().flat_map(|e| e.clips.iter().cloned()).collect();
        v.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        v
    }

    pub fn total_bytes(&self) -> u64 {
        self.state.read().unwrap().entries.values().map(|e| e.summary.size_bytes).sum()
    }

    /// Re-reads only what changed on disk. Returns true if anything changed.
    pub fn refresh(&self, root: &Path) -> bool {
        let _guard = self.refreshing.lock().unwrap();
        self.ensure_loaded(root);
        let old: HashMap<String, Signature> = self.state.read().unwrap().entries.iter().map(|(k, e)| (k.clone(), e.sig.clone())).collect();
        let mut seen = Vec::new();
        let mut updates: Vec<Entry> = Vec::new();
        if let Ok(rd) = std::fs::read_dir(root) {
            for e in rd.flatten() {
                let dir = e.path();
                if !dir.join(SESSION_FILE).is_file() {
                    continue;
                }
                let id = e.file_name().to_string_lossy().to_string();
                seen.push(id.clone());
                if let Some(prev) = old.get(&id) {
                    // Cheap check first: the folder, session.json and clips folder unchanged.
                    let video = self.state.read().unwrap().entries.get(&id).and_then(|x| x.summary.video_path.clone());
                    let now = signature(&dir, video.as_deref());
                    if &now == prev {
                        // Thumbnails can appear later (generated after the game).
                        self.update_thumbs(&id);
                        continue;
                    }
                }
                match GameSession::load(&dir) {
                    Ok(s) if s.id == id => updates.push(read_entry(&dir, &s, &self.thumbs)),
                    Ok(mut s) => {
                        // Folder renamed by hand: the folder name is the id.
                        s.id = id.clone();
                        updates.push(read_entry(&dir, &s, &self.thumbs));
                    }
                    Err(err) => log::warn!("skipping {}: {err}", dir.display()),
                }
            }
        }
        let mut changed = !updates.is_empty();
        {
            let mut st = self.state.write().unwrap();
            let before = st.entries.len();
            st.entries.retain(|k, _| seen.contains(k));
            changed |= st.entries.len() != before;
            for u in updates {
                st.entries.insert(u.summary.id.clone(), u);
            }
            st.refreshed_once = true;
        }
        if changed {
            self.save();
        }
        changed
    }

    fn update_thumbs(&self, id: &str) {
        let mut st = self.state.write().unwrap();
        if let Some(e) = st.entries.get_mut(id) {
            if e.summary.thumb_path.is_none() {
                let t = self.thumbs.session(id);
                if t.exists() {
                    e.summary.thumb_path = Some(t);
                }
            }
            for c in e.clips.iter_mut().filter(|c| c.thumb_path.is_none()) {
                let t = self.thumbs.clip(id, &c.file);
                if t.exists() {
                    c.thumb_path = Some(t);
                }
            }
        }
    }

    /// Marks a thumbnail as present (after generating it).
    pub fn thumb_ready(&self, id: &str) {
        self.update_thumbs(id);
    }

    /// Forgets one game (after deleting it) without walking the folder.
    pub fn remove(&self, id: &str) {
        self.state.write().unwrap().entries.remove(id);
        self.save();
    }

    /// Writes the cache file (atomic).
    pub fn save(&self) {
        let st = self.state.read().unwrap();
        let file = CacheFile { version: CACHE_VERSION, root: st.root.clone(), entries: st.entries.values().cloned().collect() };
        drop(st);
        if let Some(d) = self.cache_file.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let tmp = self.cache_file.with_extension("json.tmp");
        if let Ok(b) = serde_json::to_vec(&file) {
            if std::fs::write(&tmp, b).is_ok() {
                let _ = std::fs::rename(&tmp, &self.cache_file);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Storage clean-up
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CleanupAction {
    /// The whole game: video, timeline (session.json), clips and thumbnails.
    DeleteGame,
    /// Only the full video; the game keeps its timeline and its clips marked "keep".
    DeleteVideo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupStep {
    pub id: String,
    pub action: CleanupAction,
    pub bytes: u64,
    /// Why: "storage limit" or "older than N days".
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CleanupPlan {
    pub steps: Vec<CleanupStep>,
    pub total_before: u64,
    pub total_after: u64,
    pub limit: u64,
    /// Still over the limit after the plan: only protected games (favorites, kept clips,
    /// the game being recorded) are left.
    pub still_over: bool,
}

/// Decides what to delete, oldest first. Favorites and `protect` (the game being recorded)
/// are never touched; a game with clips marked "keep" only loses its full video.
/// `limit_bytes` 0 = no size limit; `max_age_days` 0 = no age limit.
pub fn plan_cleanup(games: &[SessionSummary], limit_bytes: u64, max_age_days: u32, now: DateTime<Local>, protect: Option<&str>) -> CleanupPlan {
    let mut oldest_first: Vec<&SessionSummary> = games.iter().collect();
    oldest_first.sort_by(|a, b| a.started_at.cmp(&b.started_at));
    let total_before: u64 = games.iter().map(|g| g.size_bytes).sum();
    let mut total = total_before;
    let mut steps: Vec<CleanupStep> = Vec::new();
    let mut done: std::collections::HashSet<&str> = Default::default();
    let deletable = |g: &SessionSummary| !g.favorite && Some(g.id.as_str()) != protect;
    let step_for = |g: &SessionSummary, reason: String| -> Option<CleanupStep> {
        if g.kept_clips > 0 {
            (g.video_bytes > 0 && !g.video_removed).then(|| CleanupStep { id: g.id.clone(), action: CleanupAction::DeleteVideo, bytes: g.video_bytes + g.input_bytes, reason })
        } else {
            Some(CleanupStep { id: g.id.clone(), action: CleanupAction::DeleteGame, bytes: g.size_bytes, reason })
        }
    };
    if max_age_days > 0 {
        for g in &oldest_first {
            if deletable(g) && (now - g.started_at).num_days() >= max_age_days as i64 {
                if let Some(s) = step_for(g, format!("older than {max_age_days} days")) {
                    total = total.saturating_sub(s.bytes);
                    done.insert(g.id.as_str());
                    steps.push(s);
                }
            }
        }
    }
    if limit_bytes > 0 {
        for g in &oldest_first {
            if total <= limit_bytes {
                break;
            }
            if done.contains(g.id.as_str()) || !deletable(g) {
                continue;
            }
            if let Some(s) = step_for(g, "storage limit".into()) {
                total = total.saturating_sub(s.bytes);
                steps.push(s);
            }
        }
    }
    CleanupPlan { steps, total_before, total_after: total, limit: limit_bytes, still_over: limit_bytes > 0 && total > limit_bytes }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupDone {
    pub at: DateTime<Local>,
    pub id: String,
    /// e.g. "League of Legends · Ahri · 28 Sep 2026"
    pub title: String,
    pub action: CleanupAction,
    pub bytes: u64,
    pub reason: String,
}

/// Human title for logs and notices.
pub fn game_title(g: &SessionSummary) -> String {
    let mut t = g.game_name.clone();
    if let Some(c) = g.player.as_ref().and_then(|p| p.character.as_deref()) {
        t.push_str(" · ");
        t.push_str(c);
    }
    t.push_str(" · ");
    t.push_str(&g.started_at.format("%-d %b %Y %H:%M").to_string());
    t
}

/// Carries out a plan. `still_ok(id)` is asked before each step (e.g. "no game started");
/// returning false stops the clean-up. Returns what was actually removed.
pub fn apply_cleanup(index: &LibraryIndex, plan: &CleanupPlan, mut still_ok: impl FnMut() -> bool) -> Vec<CleanupDone> {
    let mut out = Vec::new();
    for step in &plan.steps {
        if !still_ok() {
            break;
        }
        let Some(g) = index.summary(&step.id) else { continue };
        let ok = match step.action {
            CleanupAction::DeleteGame => match std::fs::remove_dir_all(&g.dir) {
                Ok(()) => {
                    index.thumbs().remove_session(&g.id);
                    index.remove(&g.id);
                    true
                }
                Err(e) => {
                    log::warn!("clean-up: couldn't delete {} ({e})", g.dir.display());
                    false
                }
            },
            CleanupAction::DeleteVideo => remove_video_keep_clips(&g.dir).map_err(|e| log::warn!("clean-up: {} ({e:#})", g.dir.display())).is_ok(),
        };
        if ok {
            out.push(CleanupDone { at: Local::now(), id: g.id.clone(), title: game_title(&g), action: step.action, bytes: step.bytes, reason: step.reason.clone() });
        }
    }
    out
}

/// Deletes a game's full video but keeps the game (timeline + clips).
pub fn remove_video_keep_clips(dir: &Path) -> anyhow::Result<()> {
    let mut s = GameSession::load(dir)?;
    if let Some(v) = s.video_file.take() {
        let p = dir.join(&v);
        if p.exists() {
            std::fs::remove_file(&p)?;
        }
    }
    // The input recording only makes sense over the video: it goes too (its stats stay).
    if let Some(f) = s.input_file.take() {
        let p = dir.join(&f);
        if p.exists() {
            std::fs::remove_file(&p)?;
        }
    }
    s.video_removed_at = Some(Local::now());
    s.save(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::ClipInfo;
    use chrono::Duration;

    fn make(root: &Path, id: &str, age_days: i64, fav: bool, bytes: usize, kept_clip: bool) {
        let dir = root.join(id);
        let mut s = GameSession::new(id.into(), "league", "League of Legends", Local::now() - Duration::days(age_days));
        s.favorite = fav;
        s.video_file = Some("v.mp4".into());
        // Every game also has its input recording (replay overlay).
        s.input_file = Some(format!("{id}.input"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{id}.input")), vec![0u8; 1000]).unwrap();
        if kept_clip {
            std::fs::create_dir_all(dir.join(CLIPS_DIR)).unwrap();
            std::fs::write(dir.join(CLIPS_DIR).join("c.mp4"), vec![0u8; 10]).unwrap();
            s.clips.push(ClipInfo { file: "c.mp4".into(), title: "c".into(), video_start: Some(1.0), video_end: Some(5.0), created_at: Local::now(), source: "replay".into(), keep: true });
        }
        s.save(&dir).unwrap();
        std::fs::write(dir.join("v.mp4"), vec![0u8; bytes]).unwrap();
    }

    fn setup(name: &str) -> (PathBuf, LibraryIndex) {
        let root = std::env::temp_dir().join(format!("cv-lib-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("games")).unwrap();
        let idx = LibraryIndex::new(root.join("cache.json"), ThumbStore::new(root.join("Thumbnails")));
        (root, idx)
    }

    #[test]
    fn index_refreshes_incrementally_and_uses_its_cache() {
        let (root, idx) = setup("idx");
        let games = root.join("games");
        make(&games, "a", 2, false, 100, false);
        make(&games, "b", 1, false, 100, false);
        assert!(idx.refresh(&games));
        assert_eq!(idx.summaries().iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), vec!["b", "a"]);
        assert!(!idx.refresh(&games), "nothing changed");
        // A new index reads the cache file without touching the games.
        let idx2 = LibraryIndex::new(root.join("cache.json"), ThumbStore::new(root.join("Thumbnails")));
        idx2.ensure_loaded(&games);
        assert_eq!(idx2.summaries().len(), 2);
        // Delete one outside the app: the next refresh notices.
        std::fs::remove_dir_all(games.join("a")).unwrap();
        assert!(idx2.refresh(&games));
        assert_eq!(idx2.summaries().len(), 1);
        // A thumbnail appearing later is picked up.
        std::fs::create_dir_all(root.join("Thumbnails")).unwrap();
        std::fs::write(idx2.thumbs().session("b"), b"jpg").unwrap();
        idx2.refresh(&games);
        assert!(idx2.summary("b").unwrap().thumb_path.is_some());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn cleanup_deletes_oldest_first_and_respects_protection() {
        let (root, idx) = setup("clean");
        let games = root.join("games");
        make(&games, "old", 5, false, 100_000, false);
        make(&games, "oldfav", 4, true, 100_000, false);
        make(&games, "keptclip", 3, false, 100_000, true);
        make(&games, "mid", 2, false, 100_000, false);
        make(&games, "new", 1, false, 100_000, false);
        idx.refresh(&games);
        idx.thumbs().dir.exists().then_some(()).unwrap_or_else(|| std::fs::create_dir_all(&idx.thumbs().dir).unwrap());
        std::fs::write(idx.thumbs().session("old"), b"x").unwrap();
        std::fs::write(idx.thumbs().clip("old", "c.mp4"), b"x").unwrap();

        let total = idx.total_bytes();
        // Room for about 3 games: "old" goes; "oldfav" is a favorite (skipped); "keptclip" only
        // loses its video.
        let plan = plan_cleanup(&idx.summaries(), total - 150_000, 0, Local::now(), Some("new"));
        let ids: Vec<_> = plan.steps.iter().map(|s| (s.id.as_str(), s.action)).collect();
        assert_eq!(ids, vec![("old", CleanupAction::DeleteGame), ("keptclip", CleanupAction::DeleteVideo)]);
        assert_eq!(plan.steps[1].bytes, 101_000, "video + input recording");
        assert!(!plan.still_over);

        let done = apply_cleanup(&idx, &plan, || true);
        assert_eq!(done.len(), 2);
        assert!(!games.join("old").exists());
        assert!(!idx.thumbs().session("old").exists() && !idx.thumbs().clip("old", "c.mp4").exists(), "thumbnails go too");
        assert!(games.join("keptclip").join(CLIPS_DIR).join("c.mp4").exists(), "kept clip survives");
        assert!(!games.join("keptclip").join("v.mp4").exists());
        assert!(!games.join("keptclip").join("keptclip.input").exists(), "the input recording goes with the video");
        assert!(games.join("oldfav").join("oldfav.input").exists(), "favorites keep everything");
        idx.refresh(&games);
        assert!(idx.summary("keptclip").unwrap().video_removed);
        assert_eq!(idx.summary("keptclip").unwrap().input_bytes, 0);
        assert_eq!(idx.summary("mid").unwrap().input_bytes, 1000);

        // Only protected games left and still over: nothing to do, and it says so.
        let plan = plan_cleanup(&idx.summaries(), 10, 0, Local::now(), Some("new"));
        let ids: Vec<_> = plan.steps.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["mid"]);
        assert!(plan.still_over);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn cleanup_by_age() {
        let (root, idx) = setup("age");
        let games = root.join("games");
        make(&games, "old", 40, false, 10, false);
        make(&games, "oldfav", 40, true, 10, false);
        make(&games, "new", 1, false, 10, false);
        idx.refresh(&games);
        let plan = plan_cleanup(&idx.summaries(), 0, 30, Local::now(), None);
        assert_eq!(plan.steps.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), vec!["old"]);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn orphan_and_legacy_thumbnails() {
        let (root, idx) = setup("thumbs");
        let games = root.join("games");
        make(&games, "g1", 1, false, 10, false);
        std::fs::write(games.join("g1").join(LEGACY_THUMB_FILE), b"old").unwrap();
        assert!(idx.thumbs().adopt_legacy("g1", &games.join("g1")));
        assert!(idx.thumbs().session("g1").exists() && !games.join("g1").join(LEGACY_THUMB_FILE).exists());
        std::fs::write(idx.thumbs().session("gone"), b"x").unwrap();
        std::fs::write(idx.thumbs().clip("gone", "a.mp4"), b"x").unwrap();
        assert_eq!(idx.thumbs().remove_orphans(&["g1".to_string()]), 2);
        assert!(idx.thumbs().session("g1").exists());
        std::fs::remove_dir_all(root).ok();
    }
}
