//! Tray icon: shows idle / game detected / recording, and a small menu.

use crate::state::AppState;
use cv_core::engine::{EngineCommand, EngineState, LiveStatus};
use std::sync::{Arc, Mutex};
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

static ICON_IDLE: &[u8] = include_bytes!("../icons/tray-idle.png");
static ICON_DETECTED: &[u8] = include_bytes!("../icons/tray-detected.png");
static ICON_RECORDING: &[u8] = include_bytes!("../icons/tray-recording.png");

pub struct Tray {
    icon: TrayIcon,
    status_item: MenuItem<tauri::Wry>,
    clip_item: MenuItem<tauri::Wry>,
    stop_item: MenuItem<tauri::Wry>,
    last: Mutex<Option<(EngineState, String)>>,
}

pub fn create(app: &AppHandle) -> tauri::Result<Arc<Tray>> {
    let open = MenuItem::with_id(app, "open", "Open Clairvoyance", true, None::<&str>)?;
    let status_item = MenuItem::with_id(app, "status", "Waiting for a game", false, None::<&str>)?;
    let clip_item = MenuItem::with_id(app, "clip", "Save clip", false, None::<&str>)?;
    let marker = MenuItem::with_id(app, "marker", "Add marker", true, None::<&str>)?;
    let stop_item = MenuItem::with_id(app, "stop", "Stop recording", false, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&status_item, &sep1, &open, &clip_item, &marker, &stop_item, &sep2, &quit])?;

    let icon = TrayIconBuilder::with_id("main")
        .icon(Image::from_bytes(ICON_IDLE)?)
        .tooltip("Clairvoyance: waiting for a game")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, ev| {
            let st = app.state::<Arc<AppState>>();
            match ev.id().as_ref() {
                "open" => crate::show_main_window(app),
                "clip" => {
                    let _ = st.cmd.send(EngineCommand::SaveClip);
                }
                "marker" => {
                    let _ = st.cmd.send(EngineCommand::AddMarker);
                }
                "stop" => {
                    let _ = st.cmd.send(EngineCommand::StopSession);
                }
                "quit" => crate::quit(app),
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, ev| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = ev {
                crate::show_main_window(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(Arc::new(Tray { icon, status_item, clip_item, stop_item, last: Mutex::new(None) }))
}

impl Tray {
    /// Updates icon/tooltip only when something visible changed (cheap, no per-second work).
    pub fn update(&self, s: &LiveStatus) {
        let text = match s.state {
            EngineState::Idle => "Waiting for a game".to_string(),
            // e.g. "ARAM: recording off for this mode"
            EngineState::Detected => s.message.clone().unwrap_or_else(|| format!("{} detected (not recording)", s.game_name.as_deref().unwrap_or("Game"))),
            EngineState::Recording => match (s.mode_rule, s.mode_name.as_deref()) {
                (Some(cv_core::modes::ModeRule::ClipsOnly), Some(m)) => format!("{m}: clips only (no full video)"),
                (_, Some(m)) => format!("Recording {m}"),
                _ => format!("Recording {}", s.game_name.as_deref().unwrap_or("game")),
            },
        };
        let mut last = self.last.lock().unwrap();
        if last.as_ref().is_some_and(|(st, t)| *st == s.state && *t == text) {
            return;
        }
        *last = Some((s.state, text.clone()));
        let bytes = match s.state {
            EngineState::Idle => ICON_IDLE,
            EngineState::Detected => ICON_DETECTED,
            EngineState::Recording => ICON_RECORDING,
        };
        if let Ok(img) = Image::from_bytes(bytes) {
            let _ = self.icon.set_icon(Some(img));
        }
        let _ = self.icon.set_tooltip(Some(format!("Clairvoyance: {text}")));
        let _ = self.status_item.set_text(&text);
        let _ = self.clip_item.set_enabled(s.state == EngineState::Recording);
        let _ = self.stop_item.set_enabled(s.state != EngineState::Idle);
    }
}
