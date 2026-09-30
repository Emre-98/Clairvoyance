//! Clairvoyance desktop app.
//!
//! Process layout while you play: this app (Rust, below-normal priority) records on the GPU.
//! The UI (WebView2) stays loaded so it opens instantly and you can alt-tab to it during a
//! loading screen; when the window is hidden to the tray its page is suspended and trimmed
//! (see `webview_power`). Settings > General can instead close it completely in game.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autostart;
mod commands;
mod ffmpeg;
mod games;
mod input;
mod logger;
mod maintenance;
mod migrate;
mod perftest;
mod platform;
mod state;
mod tray;
mod updater;
mod webview_power;

use cv_core::engine::{Engine, EngineCommand, EngineEvent};
use cv_core::Settings;
use state::{AppState, GpuInfo, Paths};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder};

pub const MAIN: &str = "main";

fn env_dir(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).map(PathBuf::from)
}

pub const APP_NAME: &str = "Clairvoyance";

/// When the process started (for the "window ready" startup time).
pub static LAUNCHED: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

struct Roots {
    roaming: PathBuf,
    local: PathBuf,
    videos: PathBuf,
}

fn roots() -> Roots {
    Roots {
        roaming: env_dir("APPDATA").or_else(|| env_dir("HOME").map(|h| h.join(".config"))).unwrap_or_else(|| ".".into()),
        local: env_dir("LOCALAPPDATA").or_else(|| env_dir("HOME").map(|h| h.join(".local/share"))).unwrap_or_else(|| ".".into()),
        videos: env_dir("USERPROFILE").or_else(|| env_dir("HOME")).map(|h| h.join("Videos")).unwrap_or_else(|| ".".into()),
    }
}

/// Settings: `%APPDATA%\Clairvoyance\settings.json`. App data (logs, thumbnails, library
/// cache, downloaded tools): `%LOCALAPPDATA%\Clairvoyance`. Recordings: `Videos\Clairvoyance`.
fn paths() -> Paths {
    let r = roots();
    let data_dir = r.local.join(APP_NAME);
    Paths {
        config_file: r.roaming.join(APP_NAME).join("settings.json"),
        log_file: data_dir.join("logs").join("clairvoyance.log"),
        data_dir,
        default_save_dir: r.videos.join(APP_NAME),
    }
}

fn old_paths() -> migrate::OldPaths {
    let r = roots();
    migrate::OldPaths {
        config_file: r.roaming.join(migrate::OLD_NAME).join("settings.json"),
        data_dir: r.local.join(migrate::OLD_NAME),
        default_save_dir: r.videos.join(migrate::OLD_NAME),
    }
}

/// Native background behind the page: matches the theme so opening the window never flashes.
pub fn theme_background(dark: bool) -> tauri::window::Color {
    if dark {
        tauri::window::Color(11, 13, 20, 255)
    } else {
        tauri::window::Color(243, 244, 248, 255)
    }
}

/// True if the given theme setting ("system" / "dark" / "light") shows dark right now.
pub fn theme_is_dark(theme: &str) -> bool {
    match theme {
        "dark" => true,
        "light" => false,
        _ => platform::windows_prefers_dark(),
    }
}

fn build_main_window(app: &AppHandle, visible: bool) -> Option<tauri::WebviewWindow> {
    let theme = app.state::<Arc<AppState>>().settings.read().unwrap().theme().to_string();
    // The page reads this before its first paint (public/theme-boot.js), so the right theme is
    // there from the first frame.
    let boot = format!("window.__CV_THEME__ = {};", serde_json::to_string(&theme).unwrap_or_default());
    let built = WebviewWindowBuilder::new(app, MAIN, WebviewUrl::App("index.html".into()))
        .initialization_script(&boot)
        .title("Clairvoyance")
        .inner_size(1320.0, 820.0)
        .min_inner_size(980.0, 620.0)
        .decorations(false)
        .shadow(true)
        .background_color(theme_background(theme_is_dark(&theme)))
        .center()
        .visible(visible)
        .build();
    match built {
        Ok(w) => Some(w),
        Err(e) => {
            log::error!("couldn't open the window: {e}");
            None
        }
    }
}

pub fn show_main_window(app: &AppHandle) {
    let st = app.state::<Arc<AppState>>();
    if let Some(w) = app.get_webview_window(MAIN) {
        let _ = w.show();
        let _ = w.unminimize();
        webview_power::set_background(&w, false);
        let _ = w.set_focus();
        if !st.ui_visible.swap(true, Ordering::SeqCst) {
            // It was paused while hidden: bring it up to date.
            let live = st.live.lock().unwrap().clone();
            let _ = app.emit("resync", &live);
        }
        return;
    }
    st.ui_visible.store(true, Ordering::SeqCst);
    build_main_window(app, true);
}

/// Loads the UI invisibly (a few seconds after start-up, when starting in the tray), so the
/// first click on the tray icon opens it instantly.
fn preload_main_window(app: &AppHandle) {
    if app.get_webview_window(MAIN).is_some() {
        return;
    }
    if let Some(w) = build_main_window(app, false) {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            // Let the page load, then pause it until it's shown.
            tokio::time::sleep(Duration::from_secs(6)).await;
            let st = app.state::<Arc<AppState>>();
            if !st.ui_visible.load(Ordering::SeqCst) {
                webview_power::set_background(&w, true);
            }
        });
    }
}

/// Hides the window to the tray, keeping the (paused) UI loaded for instant reopening.
fn hide_main_window(app: &AppHandle) {
    let st = app.state::<Arc<AppState>>();
    if let Some(w) = app.get_webview_window(MAIN) {
        st.ui_visible.store(false, Ordering::SeqCst);
        let _ = w.hide();
        webview_power::set_background(&w, true);
    }
}

/// Closes the window: hides it (UI kept loaded) or destroys it, per Settings > General.
fn close_main_window(app: &AppHandle) {
    let keep = app.state::<Arc<AppState>>().settings.read().unwrap().keep_ui_loaded;
    if keep {
        hide_main_window(app);
    } else if let Some(w) = app.get_webview_window(MAIN) {
        app.state::<Arc<AppState>>().ui_visible.store(false, Ordering::SeqCst);
        let _ = w.destroy();
    }
}

/// Ends any recording cleanly, then exits.
pub fn quit(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let st = app.state::<Arc<AppState>>();
        let _ = st.cmd.send(EngineCommand::Shutdown);
        let task = st.engine_task.lock().unwrap().take();
        if let Some(t) = task {
            let _ = tokio::time::timeout(Duration::from_secs(45), t).await;
        }
        app.exit(0);
    });
}

fn handle_engine_event(app: &AppHandle, tray: &tray::Tray, ev: EngineEvent) {
    let st = app.state::<Arc<AppState>>();
    match &ev {
        EngineEvent::Status(s) => {
            tray.update(s);
            *st.live.lock().unwrap() = s.clone();
        }
        EngineEvent::GameStarted { game_name } => {
            log::info!("game started: {game_name}");
            if st.settings.read().unwrap().close_ui_in_game {
                close_main_window(app);
            }
        }
        EngineEvent::Notice { level, text } => log::info!("notice [{level}] {text}"),
        // The game is over and its clips are cut: thumbnails + storage clean-up now.
        EngineEvent::PostProcessed { .. } => st.maintenance.kick(),
        _ => {}
    }
    if matches!(ev, EngineEvent::LibraryChanged | EngineEvent::GameEnded { .. }) {
        // Re-read lazily: the next list request (now if the window is open, else when it's
        // shown) refreshes only what changed. Nothing is scanned during a game.
        st.library_dirty.store(true, Ordering::SeqCst);
    }
    // Only while the window is shown; a hidden (paused) UI catches up when it's shown.
    if st.ui_visible.load(Ordering::SeqCst) && app.get_webview_window(MAIN).is_some() {
        let _ = app.emit("engine", &ev);
        if matches!(ev, EngineEvent::LibraryChanged | EngineEvent::GameEnded { .. }) {
            let _ = app.emit("library-changed", ());
        }
    }
}

fn main() {
    let _ = LAUNCHED.set(std::time::Instant::now());
    let start_hidden = std::env::args().any(|a| a == "--minimized");
    let paths = paths();
    // First start after the rename from GameRecorder: bring the old settings, recordings and
    // tools over (before the logger opens its file inside the data folder).
    let migration = migrate::run(&paths, &old_paths());
    let log_file = logger::init(paths.log_file.parent().unwrap().to_path_buf());
    #[cfg(windows)]
    cv_capture::win::install_crash_logging();
    log::info!("Clairvoyance {} starting", env!("CARGO_PKG_VERSION"));
    for line in &migration {
        log::info!("migration: {line}");
    }
    platform::lower_priority();

    let settings = Settings::load(&paths.config_file);
    let paths = Paths { log_file, ..paths };

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_main_window(app)))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(commands::handler())
        .on_window_event(|w, ev| {
            if w.label() != MAIN {
                return;
            }
            match ev {
                // The title bar's X: hide to the tray (or close completely, per settings).
                tauri::WindowEvent::CloseRequested { api, .. } => {
                    let app = w.app_handle();
                    if app.state::<Arc<AppState>>().settings.read().unwrap().keep_ui_loaded {
                        api.prevent_close();
                        hide_main_window(app);
                    } else {
                        app.state::<Arc<AppState>>().ui_visible.store(false, Ordering::SeqCst);
                    }
                }
                tauri::WindowEvent::Destroyed => {
                    w.app_handle().state::<Arc<AppState>>().ui_visible.store(false, Ordering::SeqCst);
                }
                _ => {}
            }
        })
        .setup(move |app| {
            let handle = app.handle().clone();
            let (ev_tx, mut ev_rx) = tokio::sync::mpsc::unbounded_channel();
            let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
            let (in_tx, in_rx) = tokio::sync::mpsc::unbounded_channel();

            let input = input::start(in_tx);
            let platform = Arc::new(platform::WinPlatform::new(input));
            let gpu = platform::gpu().map(|(vendor, name, encoder)| GpuInfo { vendor, name, encoder });
            log::info!("GPU: {gpu:?}");
            let shared = Arc::new(RwLock::new(settings.clone()));

            let ffmpeg = Arc::new(ffmpeg::Ffmpeg::new(
                settings.ffmpeg_path.clone(),
                paths.data_dir.join("ffmpeg"),
                gpu.as_ref().map(|g| g.encoder.clone()).unwrap_or_default(),
            ));
            let recorder = Arc::new(cv_capture::NativeRecorder::new());
            let engine = Engine::new(
                games::all(),
                recorder.clone(),
                platform.clone(),
                Some(ffmpeg.clone()),
                state::resolve_encoder(&settings, gpu.as_ref()),
                paths.default_save_dir.clone(),
                ev_tx,
            );
            let task = tauri::async_runtime::spawn(engine.run(cmd_rx, in_rx));

            let st = Arc::new(AppState {
                paths: paths.clone(),
                settings: shared,
                cmd: cmd_tx,
                live: Mutex::new(Default::default()),
                recorder,
                ffmpeg,
                platform,
                gpu,
                engine_task: Mutex::new(Some(task)),
                perf: Mutex::new(Default::default()),
                perf_cancel: Default::default(),
                ui_visible: Default::default(),
                library: Arc::new(cv_core::library::LibraryIndex::new(
                    paths.data_dir.join("library-cache.json"),
                    cv_core::library::ThumbStore::new(paths.data_dir.join("Thumbnails")),
                )),
                library_dirty: Default::default(),
                library_first_refresh: Default::default(),
                maintenance: Default::default(),
                startup: Default::default(),
                updater: Default::default(),
            });
            let save_dir = st.save_dir();
            let _ = std::fs::create_dir_all(&save_dir);
            let _ = app.asset_protocol_scope().allow_directory(&save_dir, true);
            let thumbs_dir = st.library.thumbs().dir.clone();
            let _ = std::fs::create_dir_all(&thumbs_dir);
            let _ = app.asset_protocol_scope().allow_directory(&thumbs_dir, false);
            app.manage(st);

            maintenance::spawn(handle.clone());
            updater::spawn(handle.clone());
            let tray = tray::create(&handle)?;
            let h = handle.clone();
            tauri::async_runtime::spawn(async move {
                while let Some(ev) = ev_rx.recv().await {
                    handle_engine_event(&h, &tray, ev);
                }
            });

            if !(start_hidden || settings.start_minimized) || !settings.first_run_done {
                show_main_window(&handle);
            } else if settings.keep_ui_loaded {
                let h = handle.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(4)).await;
                    let h2 = h.clone();
                    let _ = h.run_on_main_thread(move || preload_main_window(&h2));
                });
            }
            // `--simulate[=seconds]`: play a fake League match right away (testing/demo).
            if let Some(arg) = std::env::args().find(|a| a.starts_with("--simulate")) {
                let length = arg.split_once('=').and_then(|(_, v)| v.parse().ok()).unwrap_or(120.0);
                let st = handle.state::<Arc<AppState>>().inner().clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(3)).await;
                    if let Err(e) = commands::start_simulation(st, 1.0, length) {
                        log::warn!("simulation: {e}");
                    }
                });
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building Clairvoyance");

    app.run(|_app, event| {
        if let RunEvent::ExitRequested { api, code, .. } = event {
            // Closing the window keeps the app in the tray; only "Quit" exits.
            if code.is_none() {
                api.prevent_exit();
            }
        }
    });
}
