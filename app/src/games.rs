//! The list of supported games. To add a game, add its module here (see docs/ADDING_A_GAME.md).

use gr_core::game::{ConfigField, GameIntegration};
use serde::Serialize;

pub fn all() -> Vec<Box<dyn GameIntegration>> {
    vec![Box::new(gr_game_league::LeagueIntegration::new()), Box::new(gr_game_cs2::Cs2Integration::new())]
}

#[derive(Serialize)]
pub struct GameMeta {
    pub id: &'static str,
    pub name: &'static str,
    pub short_name: &'static str,
    pub supports_events: bool,
    pub config_fields: Vec<ConfigField>,
    pub default_config: serde_json::Value,
}

pub fn meta() -> Vec<GameMeta> {
    all()
        .iter()
        .map(|g| GameMeta {
            id: g.id(),
            name: g.name(),
            short_name: g.short_name(),
            supports_events: g.supports_events(),
            config_fields: g.config_fields(),
            default_config: g.default_config(),
        })
        .collect()
}
