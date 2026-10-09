//! Developer options that belong to one game (Settings > General > Developer tools): each is a
//! checkbox stored in that game's settings, with a log file of its own in the logs folder.

use crate::state::AppState;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::State;

#[derive(Serialize)]
pub struct DevOption {
    /// Game id: the option is `settings.games[game][key]`.
    pub game: &'static str,
    pub key: &'static str,
    pub label: &'static str,
    pub help: &'static str,
}

pub fn all() -> Vec<DevOption> {
    use cv_game_deadlock::diag;
    vec![DevOption { game: cv_game_deadlock::ID, key: diag::OPTION_KEY, label: diag::OPTION_LABEL, help: diag::OPTION_HELP }]
}

fn logs_dir(st: &AppState) -> PathBuf {
    st.paths.log_file.parent().map(PathBuf::from).unwrap_or_else(|| st.paths.data_dir.join("logs"))
}

/// On only with Developer tools on.
fn enabled(st: &AppState, game: &str, key: &str) -> bool {
    let s = st.settings.read().unwrap();
    s.dev_tools && s.games.get(game).and_then(|g| g.get(key)).and_then(serde_json::Value::as_bool).unwrap_or(false)
}

/// Starts the options' background threads (idle until their option is switched on).
pub fn spawn(st: Arc<AppState>) {
    use cv_game_deadlock::diag;
    let dir = logs_dir(&st);
    let run = move || diag::run(dir, move || enabled(&st, cv_game_deadlock::ID, diag::OPTION_KEY));
    if let Err(e) = std::thread::Builder::new().name("deadlock-signals".into()).spawn(run) {
        log::warn!("Deadlock match signals: {e}");
    }
}

/// The option's newest log file, or the logs folder while there is none yet.
#[tauri::command]
pub fn dev_option_log_path(st: State<'_, Arc<AppState>>, game: String, key: String) -> Result<String, String> {
    use cv_game_deadlock::diag;
    if (game.as_str(), key.as_str()) != (cv_game_deadlock::ID, diag::OPTION_KEY) {
        return Err("unknown option".into());
    }
    let dir = logs_dir(&st);
    Ok(diag::latest_log(&dir).unwrap_or(dir).to_string_lossy().into())
}
