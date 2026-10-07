//! Background upkeep, only while no game is running or recording:
//! - keeps the library index up to date,
//! - creates missing thumbnails (from the finished video, in the thumbnail folder),
//! - removes thumbnails of games that are gone,
//! - finalizes finished recordings for instant playback: the crash-safe fragmented MP4 written
//!   during the game is rewritten as a "faststart" MP4 (index at the front, same media, no
//!   re-encode; see `cv_capture::remux`), newest first, older recordings too,
//! - checks live key-press events against the finished recording (League: ult casts read from
//!   the ability bar, hardware decoding), newest first, older recordings too when the check
//!   improves (`GameIntegration::verify_version`),
//! - applies the storage limit (oldest games first; favorites and kept clips are protected).
//!
//! Runs a little after start-up, after every game (once its auto clips are cut), after the
//! storage settings change, and on demand ("Clean up now"). Each step checks again that no game
//! has started, and the work runs on a background-priority thread.

use crate::state::AppState;
use cv_core::engine::EngineState;
use cv_core::library::{self, CleanupDone, CleanupPlan};
use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

pub const THUMB_WIDTH: u32 = 480;
const LOG_KEEP: usize = 200;

#[derive(Default)]
pub struct Maintenance {
    wake: tokio::sync::Notify,
    /// One run at a time.
    running: tokio::sync::Mutex<()>,
    /// Thumbnails that couldn't be made this session (not retried every run).
    failed: Mutex<HashSet<PathBuf>>,
    /// Last clean-up result, for the Storage page.
    pub last_plan: Mutex<Option<CleanupPlan>>,
    /// The video the replay player has open: not finalized (replaced) under it.
    pub open_video: Mutex<Option<PathBuf>>,
    /// Videos that couldn't be finalized this session (not retried every run).
    finalize_failed: Mutex<HashSet<PathBuf>>,
    /// Games whose recording check failed this session (not retried every run).
    verify_failed: Mutex<HashSet<PathBuf>>,
    /// Games whose input recording couldn't be processed (not retried every run).
    input_failed: Mutex<HashSet<PathBuf>>,
}

impl Maintenance {
    pub fn kick(&self) {
        self.wake.notify_one();
    }
}

/// A game is being played or recorded (or a performance test runs): no background work.
pub fn busy(st: &AppState) -> bool {
    let live = st.live.lock().unwrap().state != EngineState::Idle;
    let perf = matches!(st.perf.lock().unwrap().state.as_str(), "preparing" | "waiting_for_game" | "running" | "waiting_for_exit" | "analyzing");
    live || perf
}

pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        // Let start-up finish first (the window must open fast).
        tokio::time::sleep(Duration::from_secs(20)).await;
        let st = app.state::<Arc<AppState>>().inner().clone();
        loop {
            // Wait until no game is running, checking every 30 s (a mutex read, nothing more).
            while busy(&st) {
                tokio::time::sleep(Duration::from_secs(30)).await;
            }
            run(&app, false).await;
            // Next run: when kicked, or every 6 hours as a safety net.
            let _ = tokio::time::timeout(Duration::from_secs(6 * 3600), st.maintenance.wake.notified()).await;
        }
    });
}

/// Right after a game is saved (v1.7.1): its thumbnail at once, so the game card is complete
/// within a second, instead of after the auto clips and the maintenance pass. The recording is
/// already playable (indexed in place when it stopped). Background priority; skipped if a new
/// game has started; holds the maintenance lock so the two never make the same file.
pub fn post_game_thumbnail(app: AppHandle, session_id: String) {
    tauri::async_runtime::spawn(async move {
        let st = app.state::<Arc<AppState>>().inner().clone();
        let _one = st.maintenance.running.lock().await;
        if busy(&st) {
            return;
        }
        let s2 = st.clone();
        let id = session_id.clone();
        let made = tauri::async_runtime::spawn_blocking(move || {
            crate::platform::in_background_mode(|| {
                let t = std::time::Instant::now();
                let lib = s2.library.clone();
                lib.refresh(&s2.save_dir());
                let g = lib.summary(&id)?;
                let (Some(video), None) = (&g.video_path, &g.thumb_path) else { return None };
                let out = lib.thumbs().session(&g.id);
                let ok = make_thumb(&s2, video, g.thumb_at, &out, s2.ffmpeg.locate().as_deref());
                if ok {
                    lib.thumb_ready(&g.id);
                    lib.save();
                }
                Some((ok, t.elapsed().as_millis()))
            })
        })
        .await
        .ok()
        .flatten();
        if let Some((ok, ms)) = made {
            log::info!("post-game: thumbnail of {session_id} {} in {ms} ms", if ok { "made" } else { "failed" });
            if ok && st.ui_visible.load(Ordering::SeqCst) {
                let _ = app.emit("library-changed", ());
            }
        }
    });
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct RunReport {
    pub thumbs_made: usize,
    /// Recordings rewritten for instant playback.
    pub finalized: usize,
    /// Games whose events were checked against the recording.
    pub verified: usize,
    /// Input recordings processed (stats, heatmap, compression).
    pub inputs: usize,
    /// A game interrupted by a crash got its video back.
    pub recovered: bool,
    pub removed: Vec<CleanupDone>,
    pub freed_bytes: u64,
    /// Over the limit with only protected games left.
    pub still_over: bool,
    pub skipped_busy: bool,
}

/// One maintenance pass. `forced` = the user pressed "Clean up now" (cleans even if automatic
/// clean-up is off).
pub async fn run(app: &AppHandle, forced: bool) -> RunReport {
    let st = app.state::<Arc<AppState>>().inner().clone();
    let _one = st.maintenance.running.lock().await;
    if busy(&st) {
        return RunReport { skipped_busy: true, ..Default::default() };
    }
    // Game mode lists (League queues): small downloads/cached files, never during a game.
    if crate::modes::refresh_catalog(&st).await {
        let _ = app.emit("modes-changed", ());
    }
    let s2 = st.clone();
    let report = tauri::async_runtime::spawn_blocking(move || crate::platform::in_background_mode(|| run_blocking(&s2, forced))).await.unwrap_or_default();

    if report.thumbs_made > 0 || report.finalized > 0 || report.verified > 0 || report.inputs > 0 || report.recovered || !report.removed.is_empty() {
        let _ = app.emit("library-changed", ());
    }
    if !report.removed.is_empty() {
        let n = report.removed.len();
        let text = format!(
            "Storage clean-up: removed {} old recording{} ({}) to stay under your {} GB limit. Details in Settings > Storage.",
            n,
            if n == 1 { "" } else { "s" },
            fmt_bytes(report.freed_bytes),
            st.settings().max_disk_gb
        );
        log::info!("{text}");
        let _ = app.emit("engine", serde_json::json!({ "type": "notice", "level": "info", "text": text }));
    }
    if report.still_over {
        let text = format!(
            "Your recordings are over the {} GB storage limit, but everything left is a favorite or has clips marked \"keep\". Remove some favorites or raise the limit in Settings > Storage.",
            st.settings().max_disk_gb
        );
        log::warn!("{text}");
        let _ = app.emit("engine", serde_json::json!({ "type": "notice", "level": "warn", "text": text }));
    }
    report
}

fn run_blocking(st: &Arc<AppState>, forced: bool) -> RunReport {
    let mut report = RunReport::default();
    let root = st.save_dir();
    let lib = st.library.clone();
    lib.refresh(&root);
    st.library_dirty.store(false, Ordering::SeqCst);

    // Thumbnails older versions kept in the game folder move to the thumbnail folder.
    let mut adopted = false;
    for g in lib.summaries() {
        if g.thumb_path.is_none() && lib.thumbs().adopt_legacy(&g.id, &g.dir) {
            lib.thumb_ready(&g.id);
            adopted = true;
        }
    }
    if adopted {
        lib.save();
    }

    // Recordings of a replay / spectating that couldn't be deleted right away (file in use).
    remove_discarded(&root);

    // Games interrupted by a crash or power cut: give them back their (partial) video.
    if recover_interrupted(st) > 0 {
        lib.refresh(&root);
        report.recovered = true;
    }

    // Storage limit first (no point making thumbnails for games about to be removed).
    let settings = st.settings();
    if settings.auto_cleanup || forced {
        let limit = settings.max_disk_gb as u64 * 1024 * 1024 * 1024;
        let plan = library::plan_cleanup(&lib.summaries(), limit, settings.auto_delete_days, chrono::Local::now(), None);
        if !plan.steps.is_empty() {
            let removed = library::apply_cleanup(&lib, &plan, || !busy(st));
            report.freed_bytes = removed.iter().map(|r| r.bytes).sum();
            for r in &removed {
                log::info!("clean-up: removed {} ({:?}, {}, {})", r.id, r.action, fmt_bytes(r.bytes), r.reason);
            }
            append_log(&st.paths.data_dir.join("cleanup-log.json"), &removed);
            report.removed = removed;
            lib.refresh(&root);
        }
        report.still_over = plan.still_over;
        *st.maintenance.last_plan.lock().unwrap() = Some(plan);
    }

    // Missing thumbnails: newest games first, then clips.
    let ffmpeg = st.ffmpeg.locate();
    for g in lib.summaries() {
        if busy(st) {
            return report;
        }
        let (Some(video), None) = (&g.video_path, &g.thumb_path) else { continue };
        let out = lib.thumbs().session(&g.id);
        if make_thumb(st, video, g.thumb_at, &out, ffmpeg.as_deref()) {
            lib.thumb_ready(&g.id);
            report.thumbs_made += 1;
        }
    }
    for c in lib.clips() {
        if busy(st) {
            return report;
        }
        if c.thumb_path.is_some() {
            continue;
        }
        let out = lib.thumbs().clip(&c.session_id, &c.file);
        let at = c.duration.map(|d| (d * 0.4).min(8.0)).unwrap_or(1.0);
        if make_thumb(st, &c.path, at, &out, ffmpeg.as_deref()) {
            lib.thumb_ready(&c.session_id);
            report.thumbs_made += 1;
        }
    }
    if report.thumbs_made > 0 {
        lib.save();
    }

    // Finished recordings -> faststart, newest first (the one you're most likely to watch).
    for g in lib.summaries() {
        if busy(st) {
            return report;
        }
        let Some(video) = &g.video_path else { continue };
        if finalize_video(st, video) {
            report.finalized += 1;
        }
    }
    if report.finalized > 0 {
        lib.refresh(&root);
    }

    // Live events checked against the recording, newest first.
    let games = crate::games::all();
    for g in lib.summaries() {
        if busy(st) {
            break;
        }
        let Some(video) = &g.video_path else { continue };
        let Some(game) = games.iter().find(|x| x.id() == g.game_id) else { continue };
        if game.verify_version() == 0 || st.maintenance.verify_failed.lock().unwrap().contains(&g.dir) {
            continue;
        }
        if verify_game(st, game.as_ref(), &g.dir, video) {
            report.verified += 1;
        }
    }
    if report.verified > 0 {
        lib.refresh(&root);
    }

    // Input recordings (replay overlay): Mechanics stats + heatmap, then compressed. After the
    // ult check, newest first, stops when a game starts.
    for g in lib.summaries() {
        if busy(st) {
            break;
        }
        if g.input_bytes == 0 {
            continue;
        }
        if process_input(st, &g.dir) {
            report.inputs += 1;
        }
    }
    if report.inputs > 0 {
        lib.refresh(&root);
    }

    // Thumbnails of games deleted outside the app.
    let ids: Vec<String> = lib.summaries().into_iter().map(|g| g.id).collect();
    let orphans = lib.thumbs().remove_orphans(&ids);
    if orphans > 0 {
        log::info!("removed {orphans} unused thumbnails");
    }
    report
}

fn remove_discarded(root: &Path) {
    let Ok(rd) = std::fs::read_dir(root) else { return };
    for d in rd.flatten().map(|e| e.path()).filter(|p| p.join(cv_core::session::DISCARDED_MARK).is_file()) {
        match std::fs::remove_dir_all(&d) {
            Ok(()) => log::info!("removed the discarded recording {}", d.display()),
            Err(e) => log::debug!("discarded recording {}: {e}", d.display()),
        }
    }
}

/// A game whose app was closed hard (crash, power cut, killed) during the recording has a
/// session.json without a video: the crash-safe recording is still in its folder. Link it,
/// with the length that made it to disk. Only runs when no game is active.
fn recover_interrupted(st: &AppState) -> usize {
    let mut n = 0;
    for g in st.library.summaries() {
        if g.video_path.is_some() || g.video_removed || g.record_mode.as_deref() == Some("clips_only") {
            continue;
        }
        let Ok(mut s) = cv_core::session::GameSession::load(&g.dir) else { continue };
        if s.video_file.is_some() {
            continue;
        }
        let Ok(rd) = std::fs::read_dir(&g.dir) else { continue };
        let video = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("mp4")))
            .filter(|p| !p.file_name().unwrap().to_string_lossy().starts_with("Replay_"))
            .max_by_key(|p| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0));
        let Some(video) = video else { continue };
        let Ok(info) = cv_capture::remux::info(&video) else { continue };
        if info.video_frames == 0 {
            continue;
        }
        s.video_file = Some(video.file_name().unwrap().to_string_lossy().to_string());
        s.video_duration = Some(info.duration_secs);
        if s.ended_at.is_none() {
            s.ended_at = std::fs::metadata(&video).and_then(|m| m.modified()).ok().map(chrono::DateTime::<chrono::Local>::from);
        }
        s.warnings.push("Clairvoyance was closed during this game (crash or power cut?): the video was recovered up to that moment.".into());
        if s.save(&g.dir).is_ok() {
            log::info!("recovered the video of interrupted game {} ({:.0} s)", g.id, info.duration_secs);
            n += 1;
        }
    }
    n
}

/// Checks one game's live events against its recording if that wasn't done yet (or was done
/// by an older version of the check). Stops as soon as a game starts (done again next run).
fn verify_game(st: &AppState, game: &dyn cv_core::game::GameIntegration, dir: &Path, video: &Path) -> bool {
    use cv_core::session::GameSession;
    let Ok(s) = GameSession::load(dir) else { return false };
    if s.verification.as_ref().is_some_and(|v| v.version >= game.verify_version()) {
        return false;
    }
    // Still being written (fragmented, not finalized yet): next run.
    if cv_capture::remux::probe(video).map(|l| l.needs_finalize()).unwrap_or(true) {
        return false;
    }
    let t = std::time::Instant::now();
    let mut checked = s.clone();
    let r = open_frames(video).and_then(|mut frames| match game.verify_recording(&mut checked, frames.as_mut(), &|| busy(st)) {
        Some(r) => r,
        None => Err(anyhow::anyhow!("not supported")),
    });
    match r {
        Ok(()) => {
            // Re-read just before saving: keep anything the user changed meanwhile (favorite,
            // clips); only the checked parts are replaced.
            let Ok(mut now) = GameSession::load(dir) else { return false };
            now.events = checked.events;
            now.key_presses = checked.key_presses;
            now.verification = checked.verification;
            match now.save(dir) {
                Ok(()) => {
                    if let Some(v) = &now.verification {
                        log::info!(
                            "checked {} against its recording in {:.1} s: {} ({} casts, {} confirmed presses, {} only in the video, {} presses without a cast)",
                            dir.display(),
                            t.elapsed().as_secs_f64(),
                            v.status,
                            v.casts,
                            v.confirmed,
                            v.video_only,
                            v.unconfirmed
                        );
                    }
                    true
                }
                Err(e) => {
                    log::warn!("saving the recording check of {}: {e:#}", dir.display());
                    false
                }
            }
        }
        Err(e) if busy(st) => {
            log::info!("recording check of {} paused ({e}); later", dir.display());
            false
        }
        Err(e) => {
            log::warn!("recording check of {}: {e:#}", dir.display());
            st.maintenance.verify_failed.lock().unwrap().insert(dir.to_path_buf());
            false
        }
    }
}

/// One game's input recording after the game: Mechanics stats into session.json, the whole-game
/// heatmap into the file, and the file compressed. Skipped when already done.
fn process_input(st: &AppState, dir: &Path) -> bool {
    use cv_core::input::{self, stats};
    use cv_core::session::GameSession;
    if st.maintenance.input_failed.lock().unwrap().contains(dir) {
        return false;
    }
    let Ok(s) = GameSession::load(dir) else { return false };
    let Some(name) = s.input_file.clone() else { return false };
    let path = dir.join(&name);
    let t = std::time::Instant::now();
    let r = (|| -> anyhow::Result<Option<String>> {
        let f = input::read(&path)?;
        let stats_ok = s.mechanics.as_ref().is_some_and(|m| m.version >= stats::STATS_VERSION);
        if stats_ok && f.compressed {
            return Ok(None);
        }
        let an = stats::Analysis::from_file(&f);
        let mech = stats::mechanics_whole(&an, s.video_offset);
        if busy(st) {
            anyhow::bail!("paused: a game started");
        }
        let mut sizes = String::new();
        if !f.compressed {
            let heat = stats::heatmap(&an, 0.0, an.end, stats::HEAT_W, stats::HEAT_H);
            let (a, b) = input::compress_in_place(&path, Some(&heat))?;
            sizes = format!(", {} KB -> {} KB", a / 1024, b / 1024);
        }
        // Re-read just before saving: keep what changed meanwhile.
        let mut now = GameSession::load(dir)?;
        now.mechanics = Some(mech.clone());
        now.save(dir)?;
        Ok(Some(format!(
            "{} samples, APM {:.0}, {} clicks, {} keys{sizes}{}",
            mech.cursor_samples,
            mech.apm,
            mech.clicks,
            mech.key_presses,
            if f.truncated { " (cut off by a crash: kept up to that point)" } else { "" }
        )))
    })();
    match r {
        Ok(Some(what)) => {
            log::info!("input recording of {} processed in {} ms: {what}", dir.display(), t.elapsed().as_millis());
            true
        }
        Ok(None) => false,
        Err(e) if busy(st) => {
            log::info!("input recording of {} paused ({e}); later", dir.display());
            false
        }
        Err(e) => {
            log::warn!("input recording of {}: {e:#}", dir.display());
            st.maintenance.input_failed.lock().unwrap().insert(dir.to_path_buf());
            false
        }
    }
}

#[cfg(windows)]
fn open_frames(video: &Path) -> anyhow::Result<Box<dyn cv_core::game::FrameSource>> {
    Ok(Box::new(cv_capture::win::frames::VideoFrames::open(video)?))
}

#[cfg(not(windows))]
fn open_frames(_video: &Path) -> anyhow::Result<Box<dyn cv_core::game::FrameSource>> {
    anyhow::bail!("no video decoder on this platform")
}

/// Rewrites one recording as a faststart MP4 if it isn't one yet. Never while a game runs (the
/// copy stops and the original stays as it was), never under the open player, and only with
/// enough free disk space for the copy.
fn finalize_video(st: &AppState, video: &Path) -> bool {
    use cv_capture::remux;
    // Leftover from a finalize that was cut off (crash or power cut).
    let tmp = video.with_extension("mp4.finalizing");
    if tmp.exists() {
        let _ = std::fs::remove_file(&tmp);
    }
    if st.maintenance.finalize_failed.lock().unwrap().contains(video) {
        return false;
    }
    let layout = match remux::probe(video) {
        Ok(l) => l,
        Err(e) => {
            log::warn!("finalize: can't read {}: {e}", video.display());
            st.maintenance.finalize_failed.lock().unwrap().insert(video.to_path_buf());
            return false;
        }
    };
    if !layout.needs_finalize() {
        return false;
    }
    let is_open = || st.maintenance.open_video.lock().unwrap().as_deref() == Some(video);
    if is_open() {
        return false; // next run
    }
    let t = std::time::Instant::now();
    // Recordings since v1.7.1 are indexed in place when they stop (no copy, no extra space);
    // this resumes one whose in-place index was cut short (power cut). Older recordings, or one
    // too long for its reserved room (> ~3 h), are copied (below).
    match remux::index_in_place(video) {
        Ok(r) => {
            log::info!("finalized {} for instant playback in place in {} ms ({:?}, index {} KB, {} fragments)", video.display(), t.elapsed().as_millis(), r.from, r.moov_bytes / 1024, r.fragments);
            return true;
        }
        Err(e) if e.kind() == std::io::ErrorKind::Unsupported => log::debug!("finalize {}: {e}; copying", video.display()),
        Err(e) => log::warn!("in-place index of {}: {e}; copying", video.display()),
    }
    let size = std::fs::metadata(video).map(|m| m.len()).unwrap_or(0);
    if let Some(free) = video.parent().and_then(crate::platform::free_space) {
        if free < size + (2 << 30) {
            log::warn!("finalize: not enough free disk space for {} ({} free)", video.display(), fmt_bytes(free));
            st.maintenance.finalize_failed.lock().unwrap().insert(video.to_path_buf());
            return false;
        }
    }
    match remux::finalize_in_place(video, &|| busy(st) || is_open()) {
        Ok(r) => {
            log::info!(
                "finalized {} for instant playback: {:?} -> faststart by copying, {} fragments, {} in {:.1} s",
                video.display(),
                r.from,
                r.fragments,
                fmt_bytes(r.bytes_out),
                t.elapsed().as_secs_f64()
            );
            true
        }
        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {
            log::info!("finalize of {} paused (game started or video opened); later", video.display());
            false
        }
        Err(e) => {
            // E.g. the file is open in another player (can't be replaced): try again next session.
            log::warn!("finalize {}: {e}", video.display());
            st.maintenance.finalize_failed.lock().unwrap().insert(video.to_path_buf());
            false
        }
    }
}

fn make_thumb(st: &AppState, video: &Path, at: f64, out: &Path, ffmpeg: Option<&Path>) -> bool {
    if st.maintenance.failed.lock().unwrap().contains(out) {
        return false;
    }
    match generate_thumbnail(video, at, out, ffmpeg) {
        Ok(()) => true,
        Err(e) => {
            log::warn!("thumbnail for {}: {e:#}", video.display());
            st.maintenance.failed.lock().unwrap().insert(out.to_path_buf());
            false
        }
    }
}

/// Media Foundation (built into Windows) first; ffmpeg as a fallback if it's installed.
pub fn generate_thumbnail(video: &Path, at: f64, out: &Path, ffmpeg: Option<&Path>) -> anyhow::Result<()> {
    if let Some(d) = out.parent() {
        std::fs::create_dir_all(d)?;
    }
    #[cfg(windows)]
    let first = cv_capture::win::thumb::video_thumbnail(video, at, out, THUMB_WIDTH);
    #[cfg(not(windows))]
    let first: anyhow::Result<()> = Err(anyhow::anyhow!("no built-in decoder on this platform"));
    let Err(e) = first else { return Ok(()) };
    let Some(ff) = ffmpeg else { return Err(e) };
    log::debug!("thumbnail via Media Foundation failed ({e:#}); trying ffmpeg");
    let mut cmd = std::process::Command::new(ff);
    cmd.args(["-hide_banner", "-loglevel", "error", "-y", "-ss", &format!("{at:.2}"), "-i"])
        .arg(video)
        .args(["-frames:v", "1", "-vf", &format!("scale={THUMB_WIDTH}:-2"), "-q:v", "5"])
        .arg(out)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000 | 0x0000_4000); // no window, below-normal priority
    }
    let status = cmd.status()?;
    if status.success() && out.exists() {
        Ok(())
    } else {
        Err(e.context(format!("ffmpeg fallback failed too ({status})")))
    }
}

pub fn read_log(path: &Path) -> Vec<CleanupDone> {
    std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn append_log(path: &Path, new: &[CleanupDone]) {
    if new.is_empty() {
        return;
    }
    let mut all = read_log(path);
    all.extend(new.iter().cloned());
    let skip = all.len().saturating_sub(LOG_KEEP);
    let all: Vec<_> = all.into_iter().skip(skip).collect();
    if let Ok(b) = serde_json::to_vec_pretty(&all) {
        let _ = std::fs::write(path, b);
    }
}

pub fn fmt_bytes(n: u64) -> String {
    let gb = n as f64 / (1024.0 * 1024.0 * 1024.0);
    if gb >= 1.0 {
        format!("{gb:.1} GB")
    } else {
        format!("{:.0} MB", n as f64 / (1024.0 * 1024.0))
    }
}
