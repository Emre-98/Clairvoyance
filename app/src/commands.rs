//! Commands the UI can call (`invoke("name", {...})`).

use crate::state::{AppState, GpuInfo};
use cv_core::engine::{EngineCommand, LiveStatus};
use cv_core::library::{self, ClipEntry, SessionSummary};
use cv_core::session::{ClipInfo, GameSession, CLIPS_DIR, THUMB_FILE};
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
    let _ = app.emit("library-changed", ());
    Ok(())
}

#[tauri::command]
pub async fn list_sessions(st: St<'_>) -> R<Vec<SessionSummary>> {
    let dir = st.save_dir();
    tauri::async_runtime::spawn_blocking(move || library::list_summaries(&dir)).await.map_err(err)
}

#[derive(Serialize)]
pub struct ClipView {
    #[serde(flatten)]
    info: ClipInfo,
    path: String,
    exists: bool,
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
    let thumb = dir.join(THUMB_FILE);
    let clips = s
        .clips
        .iter()
        .map(|c| {
            let p = dir.join(CLIPS_DIR).join(&c.file);
            ClipView { info: c.clone(), exists: p.exists(), path: p.to_string_lossy().to_string() }
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
    let _ = app.emit("library-changed", ());
    Ok(())
}

#[tauri::command]
pub async fn delete_session(app: AppHandle, st: St<'_>, id: String) -> R<()> {
    if st.live.lock().unwrap().session_id.as_deref() == Some(id.as_str()) {
        return Err("This game is still being recorded.".into());
    }
    let dir = session_dir(&st, &id)?;
    std::fs::remove_dir_all(&dir).map_err(|e| format!("Couldn't delete (is the video open somewhere?): {e}"))?;
    let _ = app.emit("library-changed", ());
    Ok(())
}

#[tauri::command]
pub async fn list_clips(st: St<'_>) -> R<Vec<ClipEntry>> {
    let dir = st.save_dir();
    tauri::async_runtime::spawn_blocking(move || library::list_clips(&dir)).await.map_err(err)
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
    let _ = app.emit("library-changed", ());
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
    });
    s.save(&dir).map_err(err)?;
    let _ = app.emit("library-changed", ());
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
    free_bytes: Option<u64>,
    games: usize,
}

#[tauri::command]
pub async fn storage_info(st: St<'_>) -> R<StorageInfo> {
    let dir = st.save_dir();
    tauri::async_runtime::spawn_blocking(move || {
        let used = library::dir_size(&dir);
        let games = library::scan(&dir).len();
        StorageInfo { save_dir: dir.to_string_lossy().to_string(), used_bytes: used, free_bytes: crate::platform::free_space(&dir), games }
    })
    .await
    .map_err(err)
}

#[tauri::command]
pub async fn apply_retention_now(app: AppHandle, st: St<'_>) -> R<Vec<String>> {
    let s = st.settings();
    let protect = st.live.lock().unwrap().session_id.clone();
    let removed = library::apply_retention(&st.save_dir(), s.auto_delete_days, s.max_disk_gb, chrono::Local::now(), protect.as_deref());
    let _ = app.emit("library-changed", ());
    Ok(removed)
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
        apply_retention_now,
        finish_first_run,
        simulate_game,
        builtin_encoders,
        recorder_selftest,
        perf_test_start,
        perf_test_status,
        perf_test_cancel,
        quit_app,
        remove_legacy_app,
    ]
}
