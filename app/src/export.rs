//! Exporting a clip with the input overlay burned in (clip editor, "Input overlay"). The page
//! draws the overlay with the player's own code, one PNG per output frame at the moments
//! `overlay_export_begin` returns, and streams them here; ffmpeg lays them over the clip and
//! re-encodes it (see `cv_capture::overlay_export`). One export at a time: starting another one
//! cancels the previous (e.g. a page that went away mid-export).

use crate::commands::{changed, frame_times_cached, session_dir};
use crate::state::AppState;
use cv_capture::overlay_export::{self, ExportParams, OverlayJob};
use cv_core::session::{ClipInfo, GameSession, CLIPS_DIR};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, State};

type R<T> = Result<T, String>;

struct Running {
    id: u64,
    job: Option<OverlayJob>,
    dir: PathBuf,
    title: String,
    start: f64,
    end: f64,
}

static RUNNING: Mutex<Option<Running>> = Mutex::new(None);

fn cancel_running() {
    if let Some(job) = RUNNING.lock().unwrap().take().and_then(|r| r.job) {
        job.cancel();
    }
}

#[derive(Serialize)]
pub struct ExportPlan {
    pub job: u64,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// The video time to draw the overlay at for each output frame (one PNG each, in order).
    pub times: Vec<f64>,
}

#[tauri::command]
pub async fn overlay_export_begin(st: State<'_, Arc<AppState>>, id: String, start: f64, end: f64, title: String) -> R<ExportPlan> {
    let dir = session_dir(&st, &id)?;
    let st = st.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let s = GameSession::load(&dir).map_err(|e| e.to_string())?;
        let video = s.video_file.as_ref().map(|f| dir.join(f)).filter(|p| p.exists()).ok_or("This game has no video.")?;
        if end <= start {
            return Err("The clip end must be after its start.".to_string());
        }
        let exe = st.ffmpeg.locate().ok_or("ffmpeg isn't installed yet (Settings > Clips > Download ffmpeg)")?;
        let info = cv_capture::remux::info(&video).map_err(|e| e.to_string())?;
        let frames = frame_times_cached(&video).ok_or("Couldn't read the video's frames.")?;
        let fps = overlay_export::nominal_fps(&frames);
        let times = overlay_export::plan(&frames, start, end, fps);
        let title = if title.trim().is_empty() { format!("Clip {}", cv_core::events::fmt_clock(start - s.video_offset)) } else { title.trim().to_string() };
        let clips = dir.join(CLIPS_DIR);
        std::fs::create_dir_all(&clips).map_err(|e| e.to_string())?;
        let out = cv_core::session::unique_path(&clips, &format!("{}.mp4", cv_core::session::sanitize(&title)));
        let encoder = st.ffmpeg.overlay_encoder(&exe);
        cancel_running();
        let p = ExportParams { video, start, end, fps, encoder, out };
        let job = OverlayJob::start(&exe, &p, times.len()).map_err(|e| format!("Couldn't start ffmpeg: {e}"))?;
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        log::info!("overlay export {n}: {} frames at {fps} fps, {}x{}, to {}", times.len(), info.width, info.height, p.out.display());
        *RUNNING.lock().unwrap() = Some(Running { id: n, job: Some(job), dir, title, start, end });
        Ok(ExportPlan { job: n, width: info.width, height: info.height, fps, times })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The next overlay frame: the request body is the PNG, header `x-job` the export.
#[tauri::command]
pub async fn overlay_export_frame(request: tauri::ipc::Request<'_>) -> R<()> {
    let tauri::ipc::InvokeBody::Raw(png) = request.body() else { return Err("expected the frame as bytes".into()) };
    let job_id: u64 = request.headers().get("x-job").and_then(|v| v.to_str().ok()).and_then(|v| v.parse().ok()).ok_or("missing x-job")?;
    let png = png.clone();
    // The job is taken out while ffmpeg reads (a write can block while it encodes).
    let mut job = {
        let mut r = RUNNING.lock().unwrap();
        let r = r.as_mut().filter(|r| r.id == job_id).ok_or("This export was cancelled.")?;
        r.job.take().ok_or("A frame is already being written.")?
    };
    let (job, res) = tauri::async_runtime::spawn_blocking(move || {
        let res = job.push(&png);
        (job, res)
    })
    .await
    .map_err(|e| e.to_string())?;
    match RUNNING.lock().unwrap().as_mut().filter(|r| r.id == job_id) {
        Some(r) => r.job = Some(job),
        None => job.cancel(), // cancelled meanwhile
    }
    res.map_err(|e| format!("{e:#}"))
}

/// All frames sent: waits for ffmpeg, adds the clip to the game. Returns the clip's path.
#[tauri::command]
pub async fn overlay_export_end(app: AppHandle, st: State<'_, Arc<AppState>>, job: u64) -> R<String> {
    let r = {
        let mut g = RUNNING.lock().unwrap();
        match g.take() {
            Some(r) if r.id == job => r,
            other => {
                *g = other;
                return Err("This export was cancelled.".into());
            }
        }
    };
    let Running { job: Some(j), dir, title, start, end, .. } = r else { return Err("A frame is still being written.".into()) };
    let t = std::time::Instant::now();
    let out = tauri::async_runtime::spawn_blocking(move || j.finish()).await.map_err(|e| e.to_string())?.map_err(|e| format!("{e:#}"))?;
    log::info!("overlay export {job}: {} written, {} ms after the last frame", out.display(), t.elapsed().as_millis());
    let mut s = GameSession::load(&dir).map_err(|e| e.to_string())?;
    s.clips.push(ClipInfo {
        file: out.file_name().unwrap().to_string_lossy().to_string(),
        title,
        video_start: Some(start),
        video_end: Some(end),
        created_at: chrono::Local::now(),
        source: "editor".into(),
        keep: false,
    });
    s.save(&dir).map_err(|e| e.to_string())?;
    changed(&app, &st);
    // Its thumbnail is made by the next maintenance pass.
    st.maintenance.kick();
    Ok(out.to_string_lossy().to_string())
}

#[tauri::command]
pub async fn overlay_export_cancel(job: u64) -> R<()> {
    let r = {
        let mut g = RUNNING.lock().unwrap();
        if g.as_ref().is_some_and(|r| r.id == job) {
            g.take()
        } else {
            None
        }
    };
    if let Some(j) = r.and_then(|r| r.job) {
        tauri::async_runtime::spawn_blocking(move || j.cancel()).await.map_err(|e| e.to_string())?;
    }
    Ok(())
}
