//! Self-updating from GitHub Releases (tauri-plugin-updater).
//!
//! - Checks `latest.json` of the newest GitHub release a minute after start-up and then every
//!   4 hours, but never while a game is running or recording (and not at all if the user turned
//!   automatic checks off; "Check now" always works).
//! - A found update is announced to the UI ("Update available: vX.Y.Z" + release notes). Nothing
//!   is downloaded until the user clicks "Update now": then it downloads in the background with
//!   progress, verifies the signature (the public key is in tauri.conf.json), runs the installer
//!   in passive mode and Clairvoyance restarts on the new version.

use crate::state::AppState;
use serde::Serialize;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

const FIRST_CHECK: Duration = Duration::from_secs(60);
const EVERY: Duration = Duration::from_secs(4 * 3600);

#[derive(Debug, Clone, Serialize, Default)]
pub struct UpdateStatus {
    /// "idle", "checking", "up_to_date", "available", "downloading", "installing", "error"
    pub state: String,
    pub current_version: String,
    pub version: Option<String>,
    pub notes: Option<String>,
    pub date: Option<String>,
    pub downloaded: u64,
    pub total: Option<u64>,
    pub error: Option<String>,
    /// When the last check finished (local time, RFC 3339).
    pub checked_at: Option<String>,
}

#[derive(Default)]
pub struct Updater {
    pub status: Mutex<UpdateStatus>,
    pending: Mutex<Option<Update>>,
    busy: tokio::sync::Mutex<()>,
}

fn set(app: &AppHandle, f: impl FnOnce(&mut UpdateStatus)) {
    let st = app.state::<Arc<AppState>>();
    let snapshot = {
        let mut s = st.updater.status.lock().unwrap();
        f(&mut s);
        s.current_version = env!("CARGO_PKG_VERSION").to_string();
        s.clone()
    };
    let _ = app.emit("update-status", &snapshot);
}

pub fn status(st: &AppState) -> UpdateStatus {
    let mut s = st.updater.status.lock().unwrap().clone();
    s.current_version = env!("CARGO_PKG_VERSION").to_string();
    if s.state.is_empty() {
        s.state = "idle".into();
    }
    s
}

/// Background schedule.
pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_CHECK).await;
        loop {
            let st = app.state::<Arc<AppState>>().inner().clone();
            if st.settings().auto_update_check {
                // Wait for the game to end first (a mutex read every 30 s, nothing more).
                while crate::maintenance::busy(&st) {
                    tokio::time::sleep(Duration::from_secs(30)).await;
                }
                if let Err(e) = check(&app).await {
                    log::info!("update check failed: {e}");
                }
            }
            tokio::time::sleep(EVERY).await;
        }
    });
}

/// Asks GitHub for the newest version. Ok(true) if one is available.
pub async fn check(app: &AppHandle) -> Result<bool, String> {
    let st = app.state::<Arc<AppState>>().inner().clone();
    let _one = st.updater.busy.lock().await;
    if matches!(st.updater.status.lock().unwrap().state.as_str(), "downloading" | "installing") {
        return Ok(true);
    }
    set(app, |s| {
        s.state = "checking".into();
        s.error = None;
    });
    let result = async { app.updater().map_err(|e| e.to_string())?.check().await.map_err(|e| e.to_string()) }.await;
    let now = chrono::Local::now().to_rfc3339();
    match result {
        Ok(Some(u)) => {
            log::info!("update available: {} -> {}", u.current_version, u.version);
            let (v, notes, date) = (u.version.clone(), u.body.clone(), u.date.map(|d| d.to_string()));
            *st.updater.pending.lock().unwrap() = Some(u);
            set(app, |s| {
                s.state = "available".into();
                s.version = Some(v);
                s.notes = notes;
                s.date = date;
                s.checked_at = Some(now);
            });
            Ok(true)
        }
        Ok(None) => {
            set(app, |s| {
                s.state = "up_to_date".into();
                s.version = None;
                s.notes = None;
                s.checked_at = Some(now);
            });
            Ok(false)
        }
        Err(e) => {
            set(app, |s| {
                s.state = "error".into();
                s.error = Some(e.clone());
                s.checked_at = Some(now);
            });
            Err(e)
        }
    }
}

/// Downloads, verifies and installs the pending update, then restarts (the installer relaunches
/// the app). Refuses while a game is running or recording.
pub async fn install(app: &AppHandle) -> Result<(), String> {
    let st = app.state::<Arc<AppState>>().inner().clone();
    if crate::maintenance::busy(&st) {
        return Err("A game is running. Update after the game.".into());
    }
    let _one = st.updater.busy.lock().await;
    let Some(update) = st.updater.pending.lock().unwrap().clone() else {
        return Err("No update to install. Check for updates first.".into());
    };
    set(app, |s| {
        s.state = "downloading".into();
        s.downloaded = 0;
        s.total = None;
        s.error = None;
    });
    let a = app.clone();
    let mut done: u64 = 0;
    let mut last_emit = std::time::Instant::now();
    let bytes = update
        .download(
            move |chunk, total| {
                done += chunk as u64;
                if last_emit.elapsed() >= Duration::from_millis(150) {
                    last_emit = std::time::Instant::now();
                    set(&a, |s| {
                        s.downloaded = done;
                        s.total = total;
                    });
                }
            },
            || {},
        )
        .await
        .map_err(|e| {
            let msg = format!("Download failed: {e}");
            set(app, |s| {
                s.state = "error".into();
                s.error = Some(msg.clone());
            });
            msg
        })?;
    // Nothing may be recording now; save what's in memory before the installer takes over.
    if crate::maintenance::busy(&st) {
        set(app, |s| s.state = "available".into());
        return Err("A game started. The update will wait until it's over.".into());
    }
    set(app, |s| s.state = "installing".into());
    log::info!("installing update {}", update.version);
    st.library.save();
    let _ = app.remove_tray_by_id("main");
    // On Windows this starts the installer and exits; it relaunches Clairvoyance when done.
    update.install(bytes).map_err(|e| {
        let msg = format!("Install failed: {e}");
        set(app, |s| {
            s.state = "error".into();
            s.error = Some(msg.clone());
        });
        msg
    })?;
    app.restart();
}
