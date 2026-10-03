//! Replay benchmark (developer tool): `Clairvoyance.exe --bench-replays=<config.json>` opens the
//! given games N times in the real window, measures how fast the page, the first video frame,
//! playback and marker jumps are, writes the results as JSON and quits. The UI side is
//! `ui/src/lib/replaybench.ts`. Maintenance and update checks don't run in this mode.
//!
//! Config: `{ "sessions": ["<recording id>", ...], "runs": 5, "out": "<results.json>",
//!            "label": "before", "finalize": false }`
//! With `finalize: true` the videos are finalized (faststart) before the first open.

use crate::state::AppState;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use tauri::{AppHandle, Manager, State};

static CONFIG: OnceLock<Option<serde_json::Value>> = OnceLock::new();

pub fn config() -> Option<&'static serde_json::Value> {
    CONFIG
        .get_or_init(|| {
            let arg = std::env::args().find_map(|a| a.strip_prefix("--bench-replays=").or_else(|| a.strip_prefix("--ui-test=")).map(|s| s.to_string()))?;
            let text = std::fs::read_to_string(&arg).map_err(|e| log::error!("bench config {arg}: {e}")).ok()?;
            serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| log::error!("bench config {arg}: {e}")).ok()
        })
        .as_ref()
}

/// The replay benchmark turns background work off; the end-to-end tests need it.
pub fn maintenance_off() -> bool {
    config().is_some_and(|c| !c["maintenance"].as_bool().unwrap_or(false))
}

/// Also brings the window up and keeps it on top for the run: a hidden or covered window is
/// throttled by the browser (no frames painted), which would stall the measurements.
#[tauri::command]
pub fn bench_config(app: AppHandle) -> Option<serde_json::Value> {
    let c = config().cloned()?;
    crate::show_main_window(&app);
    if let Some(w) = app.get_webview_window(crate::MAIN) {
        let _ = w.set_always_on_top(true);
        let _ = w.set_focus();
    }
    Some(c)
}

/// Layout/codec/keyframes of each game's video; finalizes them first when the config asks.
#[tauri::command]
pub async fn bench_prepare(st: State<'_, Arc<AppState>>, sessions: Vec<String>, finalize: bool) -> Result<serde_json::Value, String> {
    let st = st.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut out = serde_json::Map::new();
        for id in sessions {
            let dir = st.save_dir().join(&id);
            let s = cv_core::session::GameSession::load(&dir).map_err(|e| e.to_string())?;
            let Some(video) = s.video_file.as_ref().map(|f| dir.join(f)) else { continue };
            let before = cv_capture::remux::info(&video).map_err(|e| e.to_string())?;
            let mut entry = serde_json::json!({ "video": video, "before": before });
            if finalize && before.layout.needs_finalize() {
                let t = std::time::Instant::now();
                let r = cv_capture::remux::finalize_in_place(&video, &|| false).map_err(|e| e.to_string())?;
                entry["finalize"] = serde_json::json!({ "report": r, "ms": t.elapsed().as_millis() as u64 });
                entry["after"] = serde_json::to_value(cv_capture::remux::info(&video).map_err(|e| e.to_string())?).unwrap();
            }
            out.insert(id, entry);
        }
        st.library_dirty.store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(serde_json::Value::Object(out))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn bench_finish(app: AppHandle, result: serde_json::Value) -> Result<(), String> {
    let out: PathBuf = config().and_then(|c| c["out"].as_str()).map(PathBuf::from).ok_or("no bench config")?;
    std::fs::write(&out, serde_json::to_string_pretty(&result).unwrap()).map_err(|e| e.to_string())?;
    log::info!("replay benchmark written to {}", out.display());
    let app2 = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(300));
        app2.exit(0);
    });
    let _ = app.get_webview_window(crate::MAIN);
    Ok(())
}

#[tauri::command]
pub fn bench_log(line: String) {
    log::info!("bench: {line}");
}

/// End-to-end tests: the folders in the recordings folder (a replay must leave none behind).
#[tauri::command]
pub fn bench_save_dir(st: State<'_, Arc<AppState>>) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(st.save_dir()).map(|rd| rd.flatten().filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().to_string()).collect()).unwrap_or_default();
    v.sort();
    v
}
