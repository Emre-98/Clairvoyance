//! Share: one click puts a clip on the clipboard as a file, ready to paste in Discord. With
//! "Fit for Discord" on (the default) it's a copy under Discord's free upload limit
//! (`cv_capture::share`), kept out of sight in the app's data folder and deleted at the next
//! start or after 24 hours (the file must outlive the paste, and a second share reuses it).
//! Off: the original clip file. Works during a game: ffmpeg runs at below-normal priority.

use crate::commands::{frame_times_cached, session_dir};
use crate::state::AppState;
use cv_capture::share::{self, DISCORD_TARGET_BYTES};
use cv_core::session::{GameSession, CLIPS_DIR};
use serde::Serialize;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::State;

type R<T> = Result<T, String>;

/// Folder (in the app's data folder) of the Discord copies.
pub const SHARE_DIR: &str = "Share";
pub const KEEP: Duration = Duration::from_secs(24 * 3600);

#[derive(Serialize)]
pub struct Shared {
    pub path: String,
    pub bytes: u64,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// A Discord copy was made (or reused) rather than the clip itself.
    pub fitted: bool,
    /// Put on the clipboard (else only `path` is usable).
    pub copied: bool,
}

/// One share at a time (two encodes at once would only slow both down).
static BUSY: Mutex<()> = Mutex::new(());

#[tauri::command]
pub async fn share_clip(st: State<'_, Arc<AppState>>, id: String, file: String) -> R<Shared> {
    if file.is_empty() || file.contains(['/', '\\']) || file.contains("..") {
        return Err("invalid clip".into());
    }
    let dir = session_dir(&st, &id)?;
    let st = st.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let clip = dir.join(CLIPS_DIR).join(&file);
        if !clip.is_file() {
            return Err("That clip's file is missing.".to_string());
        }
        let _one = BUSY.lock().unwrap_or_else(|e| e.into_inner());
        let root = st.paths.data_dir.join(SHARE_DIR);
        sweep(&root, KEEP);
        let fit = st.settings().share_fit_discord;
        let t = std::time::Instant::now();
        let mut out = if fit { prepare(&st, &dir, &id, &file, &clip, &root)? } else { as_is(&clip)? };
        // The clipboard holds a path: an absolute one, to a file that stays put (the Discord copy
        // is only swept at the next start or after a day).
        if let Ok(abs) = std::path::absolute(&out.path) {
            out.path = abs.to_string_lossy().to_string();
        }
        out.copied = match copy_file_to_clipboard(Path::new(&out.path)) {
            Ok(()) => true,
            Err(e) => {
                log::warn!("share: clipboard: {e:#}");
                false
            }
        };
        log::info!("share {id}/{file}: {} ({} bytes, {}x{} {} fps, fitted {}) in {} ms", out.path, out.bytes, out.width, out.height, out.fps, out.fitted, t.elapsed().as_millis());
        Ok(out)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn as_is(clip: &Path) -> R<Shared> {
    let info = cv_capture::remux::info(clip).map_err(|e| format!("Couldn't read the clip: {e}"))?;
    let fps = frame_times_cached(clip).map(|f| cv_capture::overlay_export::nominal_fps(&f)).unwrap_or(60);
    Ok(Shared { path: clip.to_string_lossy().to_string(), bytes: info.bytes, width: info.width, height: info.height, fps, fitted: false, copied: false })
}

/// The clip itself when it already fits, else its Discord copy (made now or reused).
fn prepare(st: &AppState, dir: &Path, id: &str, file: &str, clip: &Path, root: &Path) -> R<Shared> {
    let info = cv_capture::remux::info(clip).map_err(|e| format!("Couldn't read the clip: {e}"))?;
    if share::fits_as_is(info.bytes, info.codec.as_deref(), info.audio_tracks, DISCORD_TARGET_BYTES) {
        return as_is(clip);
    }
    let s = GameSession::load(dir).map_err(|e| e.to_string())?;
    let title = s.clips.iter().find(|c| c.file == file).map(|c| c.title.clone()).unwrap_or_else(|| file.trim_end_matches(".mp4").to_string());
    let champion = s.player.as_ref().and_then(|p| p.character.clone());
    // One folder per clip, so two clips with the same title never share a file name.
    let key = root.join(format!("{}_{}", cv_core::session::sanitize(id), cv_core::session::sanitize(file)));
    let out = key.join(share::file_name(champion.as_deref(), &title));
    let src_fps = frame_times_cached(clip).map(|f| cv_capture::overlay_export::nominal_fps(&f)).unwrap_or(60);
    let plan = share::plan(info.duration_secs, info.width, info.height, src_fps, DISCORD_TARGET_BYTES).ok_or_else(|| {
        format!(
            "This clip is too long to fit in {:.1} MB (up to about {:.1} min). Trim it in the clip editor, or turn off Fit for Discord.",
            DISCORD_TARGET_BYTES as f64 / 1e6,
            share::max_secs(DISCORD_TARGET_BYTES) / 60.0
        )
    })?;
    if newer_than(&out, clip) {
        let bytes = std::fs::metadata(&out).map(|m| m.len()).unwrap_or(0);
        return Ok(Shared { path: out.to_string_lossy().to_string(), bytes, width: plan.width, height: plan.height, fps: plan.fps, fitted: true, copied: false });
    }
    let _ = std::fs::remove_dir_all(&key);
    std::fs::create_dir_all(&key).map_err(|e| e.to_string())?;
    let exe = st.ffmpeg.locate().ok_or("ffmpeg isn't installed yet (Settings > Clips > Download ffmpeg)")?;
    let encoder = st.ffmpeg.h264_encoder(&exe);
    let (used, bytes) = share::make(&exe, clip, &out, plan, info.audio_tracks, &encoder, DISCORD_TARGET_BYTES).map_err(|e| {
        let _ = std::fs::remove_dir_all(&key);
        format!("{e:#}")
    })?;
    Ok(Shared { path: out.to_string_lossy().to_string(), bytes, width: used.width, height: used.height, fps: used.fps, fitted: true, copied: false })
}

/// `a` exists and was written after `b` last changed (an auto clip re-cut gets a new copy).
fn newer_than(a: &Path, b: &Path) -> bool {
    let m = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    matches!((m(a), m(b)), (Some(a), Some(b)) if a >= b)
}

pub use cv_capture::share::sweep;

/// Puts `path` on the clipboard as a file (what Explorer's Copy does): Ctrl+V in Discord,
/// a chat app or a folder attaches / copies it.
#[cfg(windows)]
fn copy_file_to_clipboard(path: &Path) -> anyhow::Result<()> {
    cv_capture::win::clipboard::copy_file(path)
}

#[cfg(not(windows))]
fn copy_file_to_clipboard(_path: &Path) -> anyhow::Result<()> {
    anyhow::bail!("copying files to the clipboard is only supported on Windows")
}
