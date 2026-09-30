//! Commands the UI can call (`invoke("name", {...})`).

use crate::state::{AppState, GpuInfo};
use cv_core::engine::{EngineCommand, LiveStatus};
use cv_core::library::{ClipEntry, SessionSummary};
use cv_core::session::{ClipInfo, GameSession, CLIPS_DIR};
use cv_core::Settings;
use cv_core::engine::ClipCutter;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

type St<'a> = State<'a, Arc<AppState>>;
type R<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn session_dir(st: &AppState, id: &str) -> R<PathBuf> {
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
    thumb_path: Option<String>,
    clips: Vec<ClipView>,
}

#[tauri::command]
pub async fn get_session(st: St<'_>, id: String) -> R<SessionView> {
    let dir = session_dir(&st, &id)?;
    let s = GameSession::load(&dir).map_err(err)?;
    let video_path = s.video_file.as_ref().map(|f| dir.join(f)).filter(|p| p.exists()).map(|p| p.to_string_lossy().to_string());
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
        thumb_path: thumb.exists().then(|| thumb.to_string_lossy().to_string()),
        clips,
        session: s,
    })
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
fn changed(app: &AppHandle, st: &AppState) {
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
pub async fn reveal_path(path: String) -> R<()> {
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
pub async fn open_path(path: String) -> R<()> {
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
            .map(|g| if g.favorite { g.size_bytes } else if g.kept_clips > 0 { g.size_bytes.saturating_sub(g.video_bytes) } else { 0 })
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
pub fn simulate_game(st: St, speed: f64, length: f64) -> R<()> {
    start_simulation(st.inner().clone(), speed, length)
}

pub fn start_simulation(st: Arc<AppState>, speed: f64, length: f64) -> R<()> {
    if st.live.lock().unwrap().session_id.is_some() {
        return Err("A game is already running.".into());
    }
    let mut opts = cv_mock_league::MockOptions { speed: speed.clamp(1.0, 60.0), length: length.clamp(60.0, 3600.0), ..Default::default() };
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
    sim.games.insert("league".into(), league);
    sim.video.display_capture = true;
    let _ = st.cmd.send(EngineCommand::ReloadSettings(Box::new(sim)));
    st.platform.set_fake_process(Some(("league of legends.exe".into(), mock.running.clone())));
    log::info!("simulated game started at {}", mock.base_url);

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
        state.platform.set_fake_process(None);
        let _ = state.cmd.send(EngineCommand::ReloadSettings(Box::new(state.engine_settings(&state.settings()))));
        mock.stop();
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
    tauri::async_runtime::spawn_blocking(|| cv_capture::NativeRecorder::available_encoders())
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
    let secs = secs.clamp(3, 30);
    tauri::async_runtime::spawn_blocking(move || -> R<SelfTest> {
        use cv_core::engine::Platform;
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).map_err(err)?;
        let _ = platform.perf_sample();
        let r = run_selftest(&dir, secs)?;
        let cpu = platform.perf_sample().map(|p| p.0).unwrap_or(0.0);
        Ok(SelfTest { bytes: std::fs::metadata(&r.0).map(|m| m.len()).unwrap_or(0), path: r.0.to_string_lossy().into(), frames: r.1, dropped: r.2, fps: r.1 as f64 / secs as f64, cpu_percent: cpu })
    })
    .await
    .map_err(err)?
}

#[cfg(windows)]
fn run_selftest(dir: &std::path::Path, secs: u64) -> R<(PathBuf, u64, u64)> {
    cv_capture::win::self_test(dir, secs, None, false).map_err(|e| format!("{e:#}"))
}

#[cfg(not(windows))]
fn run_selftest(_dir: &std::path::Path, _secs: u64) -> R<(PathBuf, u64, u64)> {
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
    ]
}
