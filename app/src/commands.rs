//! Commands the UI can call (`invoke("name", {...})`).

use crate::state::{AppState, GpuInfo};
use cv_core::engine::ClipCutter;
use cv_core::engine::{EngineCommand, LiveStatus};
use cv_core::library::{ClipEntry, SessionSummary};
use cv_core::session::{ClipInfo, GameSession, CLIPS_DIR};
use cv_core::Settings;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

type St<'a> = State<'a, Arc<AppState>>;
type R<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

pub(crate) fn session_dir(st: &AppState, id: &str) -> R<PathBuf> {
    if id.is_empty() || id.contains(['/', '\\']) || id.contains("..") {
        return Err("invalid game id".into());
    }
    let d = st.save_dir().join(id);
    if !d.join(cv_core::session::SESSION_FILE).exists() {
        return Err("That game no longer exists.".into());
    }
    Ok(d)
}

#[tauri::command]
pub fn get_status(st: St) -> LiveStatus {
    st.live.lock().unwrap().clone()
}

#[derive(Serialize)]
pub struct AppInfo {
    version: &'static str,
    log_file: String,
    config_file: String,
    default_save_dir: String,
    save_dir: String,
    gpu: Option<GpuInfo>,
    games: Vec<crate::games::GameMeta>,
    /// The old app (GameRecorder, before the rename) is still installed.
    legacy_install: bool,
}

#[tauri::command]
pub fn app_info(st: St) -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        log_file: st.paths.log_file.to_string_lossy().into(),
        config_file: st.paths.config_file.to_string_lossy().into(),
        default_save_dir: st.paths.default_save_dir.to_string_lossy().into(),
        save_dir: st.save_dir().to_string_lossy().into(),
        gpu: st.gpu.clone(),
        games: crate::games::meta(),
        legacy_install: crate::migrate::legacy_uninstaller().is_some(),
    }
}

/// Uninstalls the old GameRecorder app (recordings and settings are kept).
#[tauri::command]
pub async fn remove_legacy_app() -> R<()> {
    tauri::async_runtime::spawn_blocking(crate::migrate::remove_legacy_install).await.map_err(err)?
}

#[tauri::command]
pub fn get_settings(st: St) -> Settings {
    st.settings()
}

#[tauri::command]
pub async fn save_settings(app: AppHandle, st: St<'_>, settings: Settings) -> R<()> {
    let old = st.settings();
    // Game modes are changed through their own commands (saved at once); the page's draft
    // copy may be older, so never let it overwrite them.
    let mut settings = settings;
    settings.modes = old.modes.clone();
    settings.save(&st.paths.config_file).map_err(err)?;
    *st.settings.write().unwrap() = settings.clone();
    if old.start_with_windows != settings.start_with_windows || settings.start_with_windows {
        if let Err(e) = crate::autostart::set(settings.start_with_windows) {
            log::warn!("autostart: {e:#}");
        }
    }
    st.ffmpeg.set_configured(settings.ffmpeg_path.clone());
    let _ = app.asset_protocol_scope().allow_directory(st.save_dir(), true);
    let _ = st.cmd.send(EngineCommand::ReloadSettings(Box::new(st.engine_settings(&settings))));
    if old.save_dir != settings.save_dir {
        st.library_dirty.store(true, std::sync::atomic::Ordering::SeqCst);
    }
    if (old.max_disk_gb, old.auto_cleanup, old.auto_delete_days, &old.save_dir) != (settings.max_disk_gb, settings.auto_cleanup, settings.auto_delete_days, &settings.save_dir) {
        st.maintenance.kick();
    }
    let _ = app.emit("library-changed", ());
    Ok(())
}

/// Switches the theme at once (no restart) and remembers it. `dark_now` is what the page shows,
/// used to match the native window background (it matters while resizing).
#[tauri::command]
pub fn set_theme(app: AppHandle, st: St, theme: String, dark_now: bool) -> R<()> {
    let theme = match theme.as_str() {
        "dark" | "light" => theme,
        _ => "system".to_string(),
    };
    let changed = {
        let mut s = st.settings.write().unwrap();
        let changed = s.theme != theme;
        s.theme = theme;
        if changed {
            s.save(&st.paths.config_file).map_err(err)?;
        }
        changed
    };
    if let Some(w) = app.get_webview_window(crate::MAIN) {
        let _ = w.set_background_color(Some(crate::theme_background(dark_now)));
    }
    if changed {
        log::info!("theme: {}", st.settings.read().unwrap().theme);
    }
    Ok(())
}

/// Makes sure the library index is loaded and current. The first call after start-up answers
/// from the cache file at once and refreshes in the background (then tells the UI if anything
/// changed); later calls only re-read folders that changed.
fn library_ready(app: &AppHandle, st: &Arc<AppState>) {
    use std::sync::atomic::Ordering;
    let root = st.save_dir();
    let lib = st.library.clone();
    lib.ensure_loaded(&root);
    if lib.needs_first_refresh() && !lib.summaries().is_empty() {
        if !st.library_first_refresh.swap(true, Ordering::SeqCst) {
            let (app, lib) = (app.clone(), lib.clone());
            std::thread::spawn(move || {
                if crate::platform::in_background_mode(|| lib.refresh(&root)) {
                    let _ = app.emit("library-changed", ());
                }
            });
        }
        return;
    }
    if lib.needs_first_refresh() || st.library_dirty.swap(false, Ordering::SeqCst) {
        lib.refresh(&root);
    }
}

#[tauri::command]
pub async fn list_sessions(app: AppHandle, st: St<'_>) -> R<Vec<SessionSummary>> {
    let st = st.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        library_ready(&app, &st);
        st.library.summaries()
    })
    .await
    .map_err(err)
}

#[derive(Serialize)]
pub struct ClipView {
    #[serde(flatten)]
    info: ClipInfo,
    path: String,
    exists: bool,
    thumb_path: Option<String>,
}

#[derive(Serialize)]
pub struct SessionView {
    session: GameSession,
    dir: String,
    video_path: Option<String>,
    /// Size of the video file: part of its URL, so the player never mixes up cached bytes of
    /// the recording before and after it was finalized.
    video_bytes: Option<u64>,
    thumb_path: Option<String>,
    clips: Vec<ClipView>,
    /// Size of the input recording (replay overlay), if this game has one.
    input_bytes: Option<u64>,
}

#[tauri::command]
pub async fn get_session(st: St<'_>, id: String) -> R<SessionView> {
    let dir = session_dir(&st, &id)?;
    let s = GameSession::load(&dir).map_err(err)?;
    let video = s.video_file.as_ref().map(|f| dir.join(f)).and_then(|p| std::fs::metadata(&p).ok().map(|m| (p, m.len())));
    let video_bytes = video.as_ref().map(|v| v.1);
    let video_path = video.map(|(p, _)| p.to_string_lossy().to_string());
    let thumb = st.library.thumbs().session(&id);
    let clips = s
        .clips
        .iter()
        .map(|c| {
            let p = dir.join(CLIPS_DIR).join(&c.file);
            let t = st.library.thumbs().clip(&id, &c.file);
            ClipView { info: c.clone(), exists: p.exists(), path: p.to_string_lossy().to_string(), thumb_path: t.exists().then(|| t.to_string_lossy().to_string()) }
        })
        .collect();
    Ok(SessionView {
        dir: dir.to_string_lossy().to_string(),
        video_path,
        video_bytes,
        thumb_path: thumb.exists().then(|| thumb.to_string_lossy().to_string()),
        clips,
        input_bytes: s.input_file.as_ref().and_then(|f| std::fs::metadata(dir.join(f)).ok()).map(|m| m.len()),
        session: s,
    })
}

/// The replay player opened (or closed, `None`) a video: maintenance won't replace that file
/// while it's open.
#[tauri::command]
pub fn player_open(st: St<'_>, path: Option<String>) {
    if path.is_none() {
        // The replay closed: free the decoded input recording.
        *INPUT_CACHE.lock().unwrap() = None;
    }
    *st.maintenance.open_video.lock().unwrap() = path.map(PathBuf::from);
}

/// The decoded input recording of the replay that's open (only one at a time).
struct InputCached {
    path: PathBuf,
    len: u64,
    analysis: Arc<cv_core::input::stats::Analysis>,
    heat: Option<cv_core::input::Heatmap>,
    rate: u32,
}
static INPUT_CACHE: std::sync::Mutex<Option<InputCached>> = std::sync::Mutex::new(None);

/// Held while a recording is decoded: the overlay data and the ability bubbles are requested
/// together on the first switch-on, and the second request waits for the first decode instead
/// of decoding the same file again.
static INPUT_DECODING: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn input_cached(dir: &Path) -> R<(Arc<cv_core::input::stats::Analysis>, Option<cv_core::input::Heatmap>, u32)> {
    use cv_core::input::{self, stats};
    let _decoding = INPUT_DECODING.lock().unwrap_or_else(|e| e.into_inner());
    let s = GameSession::load(dir).map_err(err)?;
    let path = dir.join(s.input_file.as_deref().ok_or("No input recorded for this game")?);
    let len = std::fs::metadata(&path).map_err(|_| "The input recording of this game is missing.".to_string())?.len();
    if let Some(c) = INPUT_CACHE.lock().unwrap().as_ref() {
        if c.path == path && c.len == len {
            return Ok((c.analysis.clone(), c.heat.clone(), c.rate));
        }
    }
    let f = input::read(&path).map_err(err)?;
    let an = Arc::new(stats::Analysis::from_file(&f));
    // Compressed files carry the whole-game heatmap; a raw one (just after the game) gets it now.
    let heat = f.heatmap.clone().or_else(|| Some(stats::heatmap(&an, 0.0, an.end, stats::HEAT_W, stats::HEAT_H)));
    *INPUT_CACHE.lock().unwrap() = Some(InputCached { path, len, analysis: an.clone(), heat: heat.clone(), rate: f.rate });
    Ok((an, heat, f.rate))
}

/// The replay overlay's data (binary, see `cv_core::input::stats::ui_payload`). Only loaded when
/// the overlay is first switched on for a replay.
#[tauri::command]
pub async fn input_load(st: St<'_>, id: String) -> R<tauri::ipc::Response> {
    let dir = session_dir(&st, &id)?;
    tauri::async_runtime::spawn_blocking(move || {
        let t = std::time::Instant::now();
        let (an, heat, rate) = input_cached(&dir)?;
        let b = cv_core::input::stats::ui_payload(&an, heat.as_ref(), rate);
        log::debug!("input overlay data for {}: {} samples, {} KB in {} ms", dir.display(), an.moves.len(), b.len() / 1024, t.elapsed().as_millis());
        Ok(tauri::ipc::Response::new(b))
    })
    .await
    .map_err(err)?
}

/// Size + modification time of a file: changes when it is finalized (the in-place index keeps
/// the size).
fn file_version(path: &Path) -> Option<(u64, u128)> {
    let m = std::fs::metadata(path).ok()?;
    let t = m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_nanos()).unwrap_or(0);
    Some((m.len(), t))
}

/// Start times of every frame of a video (cached per file, size and modification time).
pub(crate) fn frame_times_cached(path: &Path) -> Option<Arc<Vec<f64>>> {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<(PathBuf, (u64, u128)), Arc<Vec<f64>>>>> = OnceLock::new();
    let key = (path.to_path_buf(), file_version(path)?);
    if let Some(f) = CACHE.get_or_init(Default::default).lock().unwrap().get(&key) {
        return Some(f.clone());
    }
    let f = Arc::new(cv_capture::remux::frame_times(path).map_err(|e| log::debug!("frame times of {}: {e}", path.display())).ok()?);
    let mut c = CACHE.get_or_init(Default::default).lock().unwrap();
    if c.len() > 8 {
        c.clear();
    }
    c.insert(key, f.clone());
    Some(f)
}

#[derive(Serialize)]
pub struct ActionsView {
    /// The game's action keys with the binds of this game (or the game's defaults).
    actions: Vec<cv_core::input::actions::ActionKey>,
    categories: Vec<cv_core::input::actions::ActionCategory>,
    /// The binds were saved with this game (else: the game's defaults).
    saved_binds: bool,
    /// Bubble times were snapped to the video's real frame times.
    frame_exact: bool,
    presses: cv_core::input::actions::PressArrays,
}

/// The replay's ability bubbles: every action-key press of the game with its exact time, the
/// frame it shows on, the interpolated cursor position and how to draw it. Computed once when
/// the overlay is first switched on (from the input recording that's already cached for it).
#[tauri::command]
pub async fn input_actions(st: St<'_>, id: String) -> R<ActionsView> {
    use cv_core::input::actions;
    let dir = session_dir(&st, &id)?;
    tauri::async_runtime::spawn_blocking(move || {
        let t = std::time::Instant::now();
        let s = GameSession::load(&dir).map_err(err)?;
        let (an, _, rate) = input_cached(&dir)?;
        let cursor = crate::games::by_id(&s.game_id).and_then(|g| g.cursor_input());
        let (keys, saved) = actions::session_actions(s.action_keys.as_deref(), || cursor.map(|c| c.default_action_keys()).unwrap_or_default());
        let frames = s.video_file.as_ref().map(|f| dir.join(f)).and_then(|p| frame_times_cached(&p));
        let mut presses = actions::presses(&an, &keys, rate, frames.as_deref().map(|f| f.as_slice()).unwrap_or(&[]));
        if let Some(c) = cursor {
            c.action_press_states(&s, &keys, &mut presses);
        }
        log::debug!("ability bubbles for {}: {} presses, {} ms", dir.display(), presses.len(), t.elapsed().as_millis());
        Ok(ActionsView {
            categories: cursor.map(|c| c.action_categories()).unwrap_or_default(),
            actions: keys,
            saved_binds: saved,
            frame_exact: frames.is_some(),
            presses: actions::to_arrays(&presses),
        })
    })
    .await
    .map_err(err)?
}

/// Mechanics stats for a time range (video seconds).
#[tauri::command]
pub async fn input_stats(st: St<'_>, id: String, from: f64, to: f64) -> R<cv_core::input::stats::Mechanics> {
    let dir = session_dir(&st, &id)?;
    tauri::async_runtime::spawn_blocking(move || {
        let offset = GameSession::load(&dir).map_err(err)?.video_offset;
        let (an, _, _) = input_cached(&dir)?;
        Ok(cv_core::input::stats::mechanics(&an, from.max(0.0), to, offset))
    })
    .await
    .map_err(err)?
}

/// Start time of every frame of a video, from its index (no decoding; cached per file and size),
/// as little-endian f64 bytes: frame-exact stepping and the frame ruler of the zoomed timeline.
#[tauri::command]
pub async fn video_frame_times(path: String) -> R<tauri::ipc::Response> {
    tauri::async_runtime::spawn_blocking(move || {
        let f = frame_times_cached(Path::new(&path)).ok_or_else(|| "no frame index".to_string())?;
        let mut b = Vec::with_capacity(f.len() * 8);
        for t in f.iter() {
            b.extend_from_slice(&t.to_le_bytes());
        }
        Ok(tauri::ipc::Response::new(b))
    })
    .await
    .map_err(err)?
}

/// Layout, codec and keyframe spacing of a video (Settings > Advanced, test report).
#[tauri::command]
pub async fn video_info(path: String) -> R<cv_capture::remux::VideoInfo> {
    tauri::async_runtime::spawn_blocking(move || cv_capture::remux::info(std::path::Path::new(&path)).map_err(err)).await.map_err(err)?
}

/// Keyframe times of a video (cached per file, size and modification time): marker jumps land
/// on a keyframe so the frame shows without decoding the frames before it.
#[tauri::command]
pub async fn video_keyframes(path: String) -> R<std::sync::Arc<Vec<f64>>> {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<(String, (u64, u128)), Arc<Vec<f64>>>>> = OnceLock::new();
    let ver = file_version(Path::new(&path)).ok_or_else(|| format!("can't read {path}"))?;
    let key = (path.clone(), ver);
    if let Some(k) = CACHE.get_or_init(Default::default).lock().unwrap().get(&key) {
        return Ok(k.clone());
    }
    let k = tauri::async_runtime::spawn_blocking(move || cv_capture::remux::keyframes(std::path::Path::new(&path)).map_err(err)).await.map_err(err)??;
    let k = Arc::new(k);
    let mut c = CACHE.get_or_init(Default::default).lock().unwrap();
    if c.len() > 64 {
        c.clear();
    }
    c.insert(key, k.clone());
    Ok(k)
}

#[tauri::command]
pub async fn set_favorite(app: AppHandle, st: St<'_>, id: String, favorite: bool) -> R<()> {
    let dir = session_dir(&st, &id)?;
    let mut s = GameSession::load(&dir).map_err(err)?;
    s.favorite = favorite;
    s.save(&dir).map_err(err)?;
    changed(&app, &st);
    Ok(())
}

/// The Share button's "Fit for Discord" tick: the same saved setting as in Settings, changed
/// on its own (no engine reload, so it's harmless during a game).
#[tauri::command]
pub async fn set_share_fit_discord(st: St<'_>, on: bool) -> R<()> {
    let s = {
        let mut g = st.settings.write().unwrap();
        g.share_fit_discord = on;
        g.clone()
    };
    s.save(&st.paths.config_file).map_err(err)
}

/// Marks a clip "keep" (never removed by the storage clean-up) or not.
#[tauri::command]
pub async fn set_clip_keep(app: AppHandle, st: St<'_>, id: String, file: String, keep: bool) -> R<()> {
    let dir = session_dir(&st, &id)?;
    let mut s = GameSession::load(&dir).map_err(err)?;
    let c = s.clips.iter_mut().find(|c| c.file == file).ok_or("That clip no longer exists.")?;
    c.keep = keep;
    s.save(&dir).map_err(err)?;
    changed(&app, &st);
    Ok(())
}

/// Something on disk changed because of the UI: re-read it on the next request and tell the UI.
pub(crate) fn changed(app: &AppHandle, st: &AppState) {
    st.library_dirty.store(true, std::sync::atomic::Ordering::SeqCst);
    let _ = app.emit("library-changed", ());
}

#[tauri::command]
pub async fn delete_session(app: AppHandle, st: St<'_>, id: String) -> R<()> {
    if st.live.lock().unwrap().session_id.as_deref() == Some(id.as_str()) {
        return Err("This game is still being recorded.".into());
    }
    let dir = session_dir(&st, &id)?;
    std::fs::remove_dir_all(&dir).map_err(|e| format!("Couldn't delete (is the video open somewhere?): {e}"))?;
    st.library.thumbs().remove_session(&id);
    st.library.remove(&id);
    changed(&app, &st);
    Ok(())
}

#[tauri::command]
pub async fn list_clips(app: AppHandle, st: St<'_>) -> R<Vec<ClipEntry>> {
    let st = st.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        library_ready(&app, &st);
        st.library.clips()
    })
    .await
    .map_err(err)
}

#[tauri::command]
pub async fn delete_clip(app: AppHandle, st: St<'_>, id: String, file: String) -> R<()> {
    let dir = session_dir(&st, &id)?;
    if file.contains(['/', '\\']) || file.contains("..") {
        return Err("invalid clip".into());
    }
    let mut s = GameSession::load(&dir).map_err(err)?;
    s.clips.retain(|c| c.file != file);
    let p = dir.join(CLIPS_DIR).join(&file);
    if p.exists() {
        std::fs::remove_file(&p).map_err(err)?;
    }
    s.save(&dir).map_err(err)?;
    st.library.thumbs().remove_clip(&id, &file);
    changed(&app, &st);
    Ok(())
}

#[tauri::command]
pub async fn export_clip(app: AppHandle, st: St<'_>, id: String, start: f64, end: f64, title: String, precise: bool) -> R<String> {
    let dir = session_dir(&st, &id)?;
    let s = GameSession::load(&dir).map_err(err)?;
    let video = s.video_file.as_ref().map(|f| dir.join(f)).filter(|p| p.exists()).ok_or("This game has no video.")?;
    if end <= start {
        return Err("The clip end must be after its start.".into());
    }
    let clips = dir.join(CLIPS_DIR);
    std::fs::create_dir_all(&clips).map_err(err)?;
    let title = if title.trim().is_empty() { format!("Clip {}", cv_core::events::fmt_clock(start - s.video_offset)) } else { title.trim().to_string() };
    let name = format!("{}.mp4", cv_core::session::sanitize(&title));
    let out = cv_core::session::unique_path(&clips, &name);
    st.ffmpeg.cut(&video, start, end, &out, precise).await.map_err(|e| format!("{e:#}"))?;
    let mut s = GameSession::load(&dir).map_err(err)?;
    s.clips.push(ClipInfo {
        file: out.file_name().unwrap().to_string_lossy().to_string(),
        title,
        video_start: Some(start),
        video_end: Some(end),
        created_at: chrono::Local::now(),
        source: "editor".into(),
        keep: false,
    });
    s.save(&dir).map_err(err)?;
    changed(&app, &st);
    // Its thumbnail is made by the next maintenance pass.
    st.maintenance.kick();
    Ok(out.to_string_lossy().to_string())
}

#[tauri::command]
pub fn save_clip_now(st: St) {
    let _ = st.cmd.send(EngineCommand::SaveClip);
}

#[tauri::command]
pub fn add_marker_now(st: St) {
    let _ = st.cmd.send(EngineCommand::AddMarker);
}

#[tauri::command]
pub fn stop_session(st: St) {
    let _ = st.cmd.send(EngineCommand::StopSession);
}

#[tauri::command]
pub async fn reveal_path(st: St<'_>, path: String) -> R<()> {
    // Only what the app keeps: recordings, its data (logs, tools) and its settings folder.
    let config_dir = st.paths.config_file.parent().map(Path::to_path_buf).unwrap_or_default();
    let save_dir = st.save_dir();
    if !cv_core::session::path_within(Path::new(&path), &[&save_dir, &st.paths.data_dir, &config_dir]) {
        return Err("That file isn't one of Clairvoyance's.".into());
    }
    tauri_plugin_opener::reveal_item_in_dir(Path::new(&path)).map_err(err)
}

#[tauri::command]
pub async fn open_url(url: String) -> R<()> {
    if !(url.starts_with("https://")) {
        return Err("only https links".into());
    }
    tauri_plugin_opener::open_url(url, None::<&str>).map_err(err)
}

#[tauri::command]
pub async fn open_path(st: St<'_>, path: String) -> R<()> {
    // Opens a recording or clip in the default player: only videos in the recordings folder.
    let p = Path::new(&path);
    if !cv_core::session::is_video_file(p) || !cv_core::session::path_within(p, &[&st.save_dir()]) {
        return Err("Only recordings and clips can be opened.".into());
    }
    tauri_plugin_opener::open_path(path, None::<&str>).map_err(err)
}

#[tauri::command]
pub async fn recorder_status(st: St<'_>) -> R<cv_core::recorder::RecorderStatus> {
    use cv_core::Recorder;
    Ok(st.recorder.status().await)
}

#[derive(Serialize)]
pub struct FfmpegStatus {
    available: bool,
    path: Option<String>,
}

#[tauri::command]
pub async fn ffmpeg_status(st: St<'_>) -> R<FfmpegStatus> {
    let p = st.ffmpeg.locate();
    Ok(FfmpegStatus { available: p.is_some(), path: p.map(|p| p.to_string_lossy().to_string()) })
}

#[tauri::command]
pub async fn ffmpeg_download(app: AppHandle, st: St<'_>) -> R<String> {
    let a = app.clone();
    let p = st
        .ffmpeg
        .download(move |done, total| {
            let _ = a.emit("ffmpeg-progress", serde_json::json!({ "done": done, "total": total }));
        })
        .await
        .map_err(|e| format!("Download failed: {e:#}"))?;
    Ok(p.to_string_lossy().to_string())
}

#[derive(Serialize)]
pub struct PerfNow {
    cpu: f64,
    ram_mb: f64,
}

#[tauri::command]
pub fn perf_now(st: St) -> Option<PerfNow> {
    use cv_core::engine::Platform;
    st.platform.perf_sample().map(|(cpu, ram_mb)| PerfNow { cpu, ram_mb })
}

#[derive(Serialize)]
pub struct StorageInfo {
    save_dir: String,
    used_bytes: u64,
    limit_bytes: u64,
    auto_cleanup: bool,
    free_bytes: Option<u64>,
    games: usize,
    /// Size of favorites (and kept clips), which the clean-up never removes.
    protected_bytes: u64,
    /// Over the limit and only protected games would be left.
    stuck_over_limit: bool,
    /// What the clean-up removed recently (newest first).
    recent_cleanups: Vec<cv_core::library::CleanupDone>,
    thumbnails_dir: String,
}

#[tauri::command]
pub async fn storage_info(app: AppHandle, st: St<'_>) -> R<StorageInfo> {
    let st = st.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        library_ready(&app, &st);
        let s = st.settings();
        let dir = st.save_dir();
        let games = st.library.summaries();
        let limit = s.max_disk_gb as u64 * 1024 * 1024 * 1024;
        let protect = st.live.lock().unwrap().session_id.clone();
        let plan = cv_core::library::plan_cleanup(&games, limit, 0, chrono::Local::now(), protect.as_deref());
        let protected_bytes = games
            .iter()
            .map(|g| {
                if g.favorite {
                    g.size_bytes
                } else if g.kept_clips > 0 {
                    g.size_bytes.saturating_sub(g.video_bytes)
                } else {
                    0
                }
            })
            .sum();
        let mut recent = crate::maintenance::read_log(&st.paths.data_dir.join("cleanup-log.json"));
        recent.reverse();
        recent.truncate(20);
        StorageInfo {
            save_dir: dir.to_string_lossy().to_string(),
            used_bytes: games.iter().map(|g| g.size_bytes).sum(),
            limit_bytes: limit,
            auto_cleanup: s.auto_cleanup,
            free_bytes: crate::platform::free_space(&dir),
            games: games.len(),
            protected_bytes,
            stuck_over_limit: plan.still_over,
            recent_cleanups: recent,
            thumbnails_dir: st.library.thumbs().dir.to_string_lossy().to_string(),
        }
    })
    .await
    .map_err(err)
}

/// "Clean up now" (Settings > Storage): applies the storage limit right away, even if automatic
/// clean-up is off. Refuses while a game is running.
#[tauri::command]
pub async fn cleanup_now(app: AppHandle, st: St<'_>) -> R<crate::maintenance::RunReport> {
    if crate::maintenance::busy(&st) {
        return Err("A game is running. The clean-up waits until it's over.".into());
    }
    let r = crate::maintenance::run(&app, true).await;
    if r.skipped_busy {
        return Err("A game is running. The clean-up waits until it's over.".into());
    }
    Ok(r)
}

#[tauri::command]
pub async fn finish_first_run(app: AppHandle, st: St<'_>) -> R<()> {
    let mut s = st.settings();
    s.first_run_done = true;
    s.save(&st.paths.config_file).map_err(err)?;
    *st.settings.write().unwrap() = s;
    let _ = app.emit("library-changed", ());
    Ok(())
}

/// Plays a scripted League match against a fake game API, recording the desktop with the
/// built-in recorder, so everything can be tested without playing. `speed` = game seconds per second.
#[tauri::command]
pub fn simulate_game(st: St, speed: f64, length: f64, queue: Option<i64>, watch: Option<String>) -> R<()> {
    start_simulation(st.inner().clone(), speed, length, queue, watch.as_deref())
}

/// Plays a fake League match (and a fake League client reporting `queue`, default Draft Pick),
/// so detection, mode rules, recording, events and the timeline can be tried without playing.
/// `watch`: "replay" / "spectate" / "spectate-live" / "spectate-unreadable" / "match-unreadable" / "replay-late" / "replay-unsure" plays spectator mode instead
/// (v1.7.1: never recorded), see `cv_mock_league::Watch`.
pub fn start_simulation(st: Arc<AppState>, speed: f64, length: f64, queue: Option<i64>, watch: Option<&str>) -> R<()> {
    // Each simulation has a number: the clean-up of an earlier one (which waits for its game to
    // be saved) must not switch off a newer one that started in the meantime.
    static SIMULATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    // The fake game "process" of the previous simulation must be gone first, or the next one
    // looks like the same process still open after its match (no new game would start).
    static CLOSING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if st.live.lock().unwrap().session_id.is_some() {
        return Err("A game is already running.".into());
    }
    if CLOSING.load(std::sync::atomic::Ordering::SeqCst) {
        return Err("The previous simulated game is still closing. Try again in a few seconds.".into());
    }
    let generation = SIMULATION.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
    let mut opts = cv_mock_league::MockOptions {
        speed: speed.clamp(1.0, 60.0),
        length: length.clamp(60.0, 3600.0),
        queue: cv_mock_league::queue_json(queue.unwrap_or(400)),
        watch: cv_mock_league::Watch::parse(watch.unwrap_or("")),
        ..Default::default()
    };
    let mock = (2998..3010)
        .find_map(|port| {
            opts.port = port;
            cv_mock_league::spawn(opts.clone()).ok()
        })
        .ok_or("Couldn't start the simulator (ports 2998-3009 busy).")?;
    let real = st.settings();
    let mut sim = st.engine_settings(&real);
    let mut league = sim.games.get("league").cloned().unwrap_or_else(|| serde_json::json!({}));
    league["api_base"] = serde_json::json!(mock.base_url);
    // The fake League client: a lockfile in our own data folder, found before the real one.
    let client_dir = st.paths.data_dir.join("simulator");
    let _ = std::fs::create_dir_all(&client_dir);
    std::fs::write(client_dir.join("lockfile"), mock.lockfile_text()).map_err(err)?;
    league["client_dir"] = serde_json::json!(client_dir.to_string_lossy());
    sim.games.insert("league".into(), league);
    sim.video.display_capture = true;
    let _ = st.cmd.send(EngineCommand::ReloadSettings(Box::new(sim)));
    st.platform.set_fake_process(Some(("league of legends.exe".into(), mock.running.clone())));
    log::info!("simulated game started at {} ({})", mock.base_url, watch.filter(|w| !w.is_empty()).unwrap_or("a match"));

    CLOSING.store(true, std::sync::atomic::Ordering::SeqCst);
    let state = st.clone();
    tauri::async_runtime::spawn(async move {
        while mock.running.load(std::sync::atomic::Ordering::SeqCst) {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
        // Let the engine finish the session, then go back to the real settings.
        for _ in 0..60 {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            if state.live.lock().unwrap().session_id.is_none() {
                break;
            }
        }
        mock.stop();
        if SIMULATION.load(std::sync::atomic::Ordering::SeqCst) == generation {
            state.platform.set_fake_process(None);
            let _ = state.cmd.send(EngineCommand::ReloadSettings(Box::new(state.engine_settings(&state.settings()))));
        }
        // Give the engine a moment to see the "process" gone.
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        CLOSING.store(false, std::sync::atomic::Ordering::SeqCst);
    });
    Ok(())
}

#[derive(Serialize)]
pub struct EncoderList {
    gpu: String,
    encoders: Vec<String>,
}

/// Hardware encoders the built-in recorder can use on this PC.
#[tauri::command]
pub async fn builtin_encoders() -> R<EncoderList> {
    tauri::async_runtime::spawn_blocking(cv_capture::NativeRecorder::available_encoders)
        .await
        .map_err(err)?
        .map(|(gpu, encoders)| EncoderList { gpu, encoders })
        .map_err(|e| format!("{e:#}"))
}

#[derive(Serialize)]
pub struct SelfTest {
    path: String,
    bytes: u64,
    frames: u64,
    dropped: u64,
    fps: f64,
    cpu_percent: f64,
}

/// Records a few seconds of the screen with the built-in recorder to check it works.
#[tauri::command]
pub async fn recorder_selftest(st: St<'_>, secs: u64) -> R<SelfTest> {
    if st.live.lock().unwrap().session_id.is_some() {
        return Err("A game is running; try again afterwards.".into());
    }
    let dir = st.save_dir().join("_selftest");
    let platform = st.platform.clone();
    let v = st.settings().video.clone();
    let secs = secs.clamp(3, 30);
    tauri::async_runtime::spawn_blocking(move || -> R<SelfTest> {
        use cv_core::engine::Platform;
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).map_err(err)?;
        let _ = platform.perf_sample();
        let r = run_selftest(&dir, secs, v)?;
        let cpu = platform.perf_sample().map(|p| p.0).unwrap_or(0.0);
        Ok(SelfTest { bytes: std::fs::metadata(&r.0).map(|m| m.len()).unwrap_or(0), path: r.0.to_string_lossy().into(), frames: r.1, dropped: r.2, fps: r.1 as f64 / secs as f64, cpu_percent: cpu })
    })
    .await
    .map_err(err)?
}

#[cfg(windows)]
fn run_selftest(dir: &std::path::Path, secs: u64, v: cv_core::settings::VideoSettings) -> R<(PathBuf, u64, u64)> {
    // The user's own codec / rate-control choice, so the test shows what games will get.
    let video = cv_capture::win::TestVideo { codec: v.codec, rate_control: v.rate_control, playable: v.playable_codecs };
    cv_capture::win::self_test(dir, secs, None, false, video).map_err(|e| format!("{e:#}"))
}

#[cfg(not(windows))]
fn run_selftest(_dir: &std::path::Path, _secs: u64, _v: cv_core::settings::VideoSettings) -> R<(PathBuf, u64, u64)> {
    Err("The built-in recorder only works on Windows.".into())
}

/// Starts the performance test (see perftest.rs). Poll `perf_test_status`.
#[tauri::command]
pub fn perf_test_start(st: St, phase_secs: u64) -> R<()> {
    let state = st.perf.lock().unwrap().state.clone();
    if matches!(state.as_str(), "preparing" | "waiting_for_game" | "running" | "analyzing") {
        return Err("A performance test is already running.".into());
    }
    st.perf_cancel.store(false, std::sync::atomic::Ordering::SeqCst);
    let s = st.inner().clone();
    tauri::async_runtime::spawn(crate::perftest::run(s, phase_secs.clamp(20, 300)));
    Ok(())
}

#[tauri::command]
pub fn perf_test_status(st: St) -> crate::perftest::PerfTestStatus {
    st.perf.lock().unwrap().clone()
}

#[tauri::command]
pub fn perf_test_cancel(st: St) {
    st.perf_cancel.store(true, std::sync::atomic::Ordering::SeqCst);
}

#[tauri::command]
pub fn modes_get(st: St) -> Vec<crate::modes::GameModesView> {
    crate::modes::view(&st)
}

/// Refreshes the mode lists (client queue list, Riot's list). Not during a game.
#[tauri::command]
pub async fn modes_refresh(st: St<'_>) -> R<Vec<crate::modes::GameModesView>> {
    let st = st.inner().clone();
    if !crate::maintenance::busy(&st) {
        crate::modes::refresh_catalog(&st).await;
    }
    Ok(crate::modes::view(&st))
}

/// `what`: "mode" (key), "group" (group id), "unknown", "preset" (everything / ranked /
/// ranked_normal), "seen" (clear the "new" badges).
#[tauri::command]
pub fn modes_set(st: St, game: String, what: String, key: Option<String>, rule: Option<String>) -> R<Vec<crate::modes::GameModesView>> {
    let rule = rule.as_deref().map(crate::modes::parse_rule).transpose()?;
    crate::modes::edit(&st, &game, |m| match what.as_str() {
        "mode" => {
            if let (Some(k), Some(r)) = (key.as_deref(), rule) {
                m.set_rule(k, r);
            }
        }
        "group" => {
            if let (Some(g), Some(r)) = (key.as_deref(), rule) {
                m.set_group(g, r);
            }
        }
        "unknown" => {
            if let Some(r) = rule {
                m.unknown_rule = r;
            }
        }
        "preset" => m.apply_preset(key.as_deref().unwrap_or("everything")),
        "seen" => m.clear_new(),
        _ => {}
    })?;
    Ok(crate::modes::view(&st))
}

#[tauri::command]
pub fn update_status(st: St) -> crate::updater::UpdateStatus {
    crate::updater::status(&st)
}

/// "Check now" (works even with automatic checks off).
#[tauri::command]
pub async fn update_check(app: AppHandle) -> R<crate::updater::UpdateStatus> {
    crate::updater::check(&app).await?;
    Ok(crate::updater::status(&app.state::<Arc<AppState>>()))
}

/// "Update now": download, install, restart.
#[tauri::command]
pub async fn update_install(app: AppHandle) -> R<()> {
    crate::updater::install(&app).await
}

#[derive(Serialize)]
pub struct UiTimings {
    startup_ms: Option<f64>,
    page_ms: Option<f64>,
}

/// The UI reports its first painted content. The first report of a run is the cold start
/// (process launch -> window showing the library); it's logged.
#[tauri::command]
pub fn ui_ready(st: St, page_ms: f64) -> UiTimings {
    let mut s = st.startup.lock().unwrap();
    if s.0.is_none() {
        let since = crate::LAUNCHED.get().map(|t| t.elapsed().as_secs_f64() * 1000.0);
        *s = (since, Some(page_ms));
        log::info!("startup: window content painted {:.0} ms after launch (page {:.0} ms)", since.unwrap_or(0.0), page_ms);
    }
    UiTimings { startup_ms: s.0, page_ms: s.1 }
}

#[tauri::command]
pub fn ui_timings(st: St) -> UiTimings {
    let s = st.startup.lock().unwrap();
    UiTimings { startup_ms: s.0, page_ms: s.1 }
}

/// Settings > Advanced > "Save test report": a zip with the log, the latest game's data and
/// the numbers needed to look into a problem (no videos). Returns the zip's path.
#[tauri::command]
pub async fn test_report(st: St<'_>, ui: serde_json::Value) -> R<String> {
    let st = st.inner().clone();
    tauri::async_runtime::spawn_blocking(move || crate::report::write(&st, &ui).map(|p| p.to_string_lossy().to_string()).map_err(|e| format!("{e:#}")))
        .await
        .map_err(err)?
}

#[tauri::command]
pub fn quit_app(app: AppHandle) {
    crate::quit(&app);
}

pub fn handler() -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        get_status,
        app_info,
        get_settings,
        save_settings,
        list_sessions,
        get_session,
        set_favorite,
        delete_session,
        list_clips,
        delete_clip,
        export_clip,
        crate::export::overlay_export_begin,
        crate::export::overlay_export_frame,
        crate::export::overlay_export_end,
        crate::export::overlay_export_cancel,
        crate::share::share_clip,
        set_share_fit_discord,
        save_clip_now,
        add_marker_now,
        stop_session,
        reveal_path,
        open_path,
        open_url,
        recorder_status,
        ffmpeg_status,
        ffmpeg_download,
        perf_now,
        storage_info,
        cleanup_now,
        set_clip_keep,
        ui_ready,
        ui_timings,
        update_status,
        modes_get,
        modes_refresh,
        modes_set,
        update_check,
        update_install,
        finish_first_run,
        simulate_game,
        builtin_encoders,
        recorder_selftest,
        perf_test_start,
        perf_test_status,
        perf_test_cancel,
        quit_app,
        remove_legacy_app,
        set_theme,
        player_open,
        video_info,
        test_report,
        video_keyframes,
        video_frame_times,
        input_load,
        input_actions,
        input_stats,
        crate::bench::bench_config,
        crate::bench::bench_prepare,
        crate::bench::bench_finish,
        crate::bench::bench_log,
        crate::bench::bench_save_dir,
    ]
}
