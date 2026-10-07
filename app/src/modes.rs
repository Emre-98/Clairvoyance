//! Settings > Game modes: keeps each game's mode list (League: queues) up to date and applies
//! the user's choices. The list is refreshed from the game module's sources (League: the
//! client's queue list, Riot's queues.json, both cached) by the maintenance pass (never during a
//! game) and when the Game modes page opens. Changes are saved at once and apply to the next game.

use crate::state::AppState;
use cv_core::engine::EngineCommand;
use cv_core::modes::{GameModes, ModeGroupInfo, ModeRule};
use serde::Serialize;
use std::sync::Arc;

#[derive(Serialize)]
pub struct GameModesView {
    pub game_id: String,
    pub game_name: String,
    pub groups: Vec<ModeGroupInfo>,
    pub modes: GameModes,
}

pub fn view(st: &AppState) -> Vec<GameModesView> {
    let s = st.settings();
    crate::games::all()
        .into_iter()
        .filter_map(|mut g| {
            let groups = g.mode_rules()?.mode_groups();
            Some(GameModesView { game_id: g.id().to_string(), game_name: g.name().to_string(), groups, modes: s.modes.get(g.id()).cloned().unwrap_or_default() })
        })
        .collect()
}

/// Saves the settings and hands them to the engine (applies to the next game).
pub fn commit(st: &AppState) -> Result<(), String> {
    let s = st.settings();
    s.save(&st.paths.config_file).map_err(|e| e.to_string())?;
    let _ = st.cmd.send(EngineCommand::ReloadSettings(Box::new(st.engine_settings(&s))));
    Ok(())
}

pub fn edit(st: &AppState, game: &str, f: impl FnOnce(&mut GameModes)) -> Result<(), String> {
    {
        let mut s = st.settings.write().unwrap();
        f(s.modes.entry(game.to_string()).or_default());
    }
    commit(st)
}

/// Pulls the latest mode lists. Returns true if anything changed.
pub async fn refresh_catalog(st: &Arc<AppState>) -> bool {
    let cache = st.paths.data_dir.join("cache");
    let settings = st.settings();
    let mut changed = false;
    for mut g in crate::games::all() {
        if settings.disabled_games.iter().any(|d| d == g.id()) {
            continue;
        }
        g.configure(&settings.game_config(g.id(), g.default_config()));
        let Some(rules) = g.mode_rules() else { continue };
        let (catalog, authoritative) = rules.mode_catalog(&cache).await;
        if catalog.is_empty() {
            continue;
        }
        let mut s = st.settings.write().unwrap();
        let modes = s.modes.entry(g.id().to_string()).or_default();
        let before = modes.clone();
        // The first list, and the first live list from the client (much longer than the
        // built-in one), aren't "new" to the user: only modes that appear later are.
        let first = modes.entries.is_empty() || (authoritative && modes.catalog_updated_at.is_none());
        let added = modes.merge_catalog(&catalog, authoritative);
        if first {
            // The first list isn't "new" to the user.
            modes.clear_new();
        }
        if *modes != before {
            changed = true;
            log::info!("game modes ({}): {} known, {added} new, live list: {authoritative}", g.id(), modes.entries.len());
        }
    }
    if changed {
        if let Err(e) = commit(st) {
            log::warn!("saving game modes: {e}");
        }
    }
    changed
}

pub fn parse_rule(r: &str) -> Result<ModeRule, String> {
    match r {
        "record" => Ok(ModeRule::Record),
        "clips_only" => Ok(ModeRule::ClipsOnly),
        "off" => Ok(ModeRule::Off),
        _ => Err(format!("unknown rule {r}")),
    }
}
