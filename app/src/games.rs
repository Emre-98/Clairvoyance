//! The list of supported games. To add a game, add its module here (see docs/ADDING_A_GAME.md).

use cv_core::game::{ConfigField, GameIntegration};
use serde::Serialize;

pub fn all() -> Vec<Box<dyn GameIntegration>> {
    vec![Box::new(cv_game_league::LeagueIntegration::new()), Box::new(cv_game_cs2::Cs2Integration::new()), Box::new(cv_game_deadlock::DeadlockIntegration::new())]
}

/// One shared, unconfigured instance of every game, for what doesn't need a running game (the
/// replay's ability bubbles: default binds, categories, press states). Made once.
pub fn registry() -> &'static [Box<dyn GameIntegration>] {
    static GAMES: std::sync::OnceLock<Vec<Box<dyn GameIntegration>>> = std::sync::OnceLock::new();
    GAMES.get_or_init(all)
}

pub fn by_id(id: &str) -> Option<&'static dyn GameIntegration> {
    registry().iter().find(|g| g.id() == id).map(|g| g.as_ref())
}

#[derive(Serialize)]
pub struct GameMeta {
    pub id: &'static str,
    pub name: &'static str,
    pub short_name: &'static str,
    pub supports_events: bool,
    pub input_tracking: bool,
    /// Sub-toggles of the replay overlay's "Ability bubbles" (empty: this game has none).
    pub action_categories: Vec<cv_core::input::actions::ActionCategory>,
    pub config_fields: Vec<ConfigField>,
    pub default_config: serde_json::Value,
}

pub fn meta() -> Vec<GameMeta> {
    all()
        .iter()
        .map(|g| {
            let mut fields = g.config_fields();
            let mut defaults = g.default_config();
            let cursor = g.cursor_input();
            // Cursor-based games get the core's input-recording settings.
            if cursor.is_some() {
                fields.extend(cv_core::input::config_fields());
                if let (Some(d), Some(i)) = (defaults.as_object_mut(), cv_core::input::default_config().as_object()) {
                    for (k, v) in i {
                        d.insert(k.clone(), v.clone());
                    }
                }
            }
            GameMeta {
                id: g.id(),
                name: g.name(),
                short_name: g.short_name(),
                supports_events: g.supports_events(),
                input_tracking: cursor.is_some(),
                action_categories: cursor.map(|c| c.action_categories()).unwrap_or_default(),
                config_fields: fields,
                default_config: defaults,
            }
        })
        .collect()
}
