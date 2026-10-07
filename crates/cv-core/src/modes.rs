//! Per-mode recording rules ("record Ranked and Normal, not ARAM").
//!
//! A game module that knows its modes (League: queues) reports the mode of the match that's
//! starting ([`MatchMode`]) and a catalog of the modes that exist ([`CatalogMode`]). This module
//! keeps the user's choice per mode ([`GameModes`], stored in the settings) and decides before
//! recording starts. Nothing here is hardcoded per game: modes are identified by a key the game
//! module chooses (League: "q420"), named from live data, and grouped by the module.
//! New modes are added automatically with the "unknown / new modes" rule; modes that disappear
//! stay, marked unavailable, so the choice comes back with them.

use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ModeRule {
    /// Full video + events + clips.
    #[default]
    Record,
    /// Events, hotkey clips and automatic event clips from the replay buffer; no full video.
    ClipsOnly,
    /// Not recorded at all.
    Off,
}

impl ModeRule {
    pub fn label(self) -> &'static str {
        match self {
            ModeRule::Record => "recording",
            ModeRule::ClipsOnly => "clips only",
            ModeRule::Off => "recording off",
        }
    }
    /// Higher = records more. Used when only a coarse game type is known.
    fn rank(self) -> u8 {
        match self {
            ModeRule::Off => 0,
            ModeRule::ClipsOnly => 1,
            ModeRule::Record => 2,
        }
    }
}

/// A group of modes in the settings page (ids and labels come from the game module).
#[derive(Debug, Clone, Serialize)]
pub struct ModeGroupInfo {
    pub id: &'static str,
    pub label: &'static str,
    pub help: &'static str,
}

/// The mode of the match that is starting.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MatchMode {
    /// Stable key, e.g. "q420". `None` when only the coarse game type is known.
    pub key: Option<String>,
    pub queue_id: Option<i64>,
    /// Display name, e.g. "Ranked Solo/Duo".
    pub name: String,
    /// Coarse game type, e.g. "CLASSIC", "ARAM", "CHERRY".
    pub game_mode: Option<String>,
    pub group: String,
    /// Rule a newly seen mode of this kind gets by default (before the "unknown" rule).
    #[serde(default)]
    pub default_rule: Option<ModeRule>,
    /// Where it came from, e.g. "League client (LCU)", "Live Client Data API", "unknown".
    pub source: String,
}

/// A mode that exists (client queue list, official list, or seen in a game).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CatalogMode {
    pub key: String,
    pub queue_id: Option<i64>,
    pub name: String,
    pub game_mode: Option<String>,
    pub group: String,
    /// Rule used the first time this mode appears (`None` = the "unknown / new modes" rule).
    pub default_rule: Option<ModeRule>,
    /// `Some(true/false)` when the source knows whether it can be played right now.
    pub available: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModeEntry {
    pub name: String,
    #[serde(default)]
    pub queue_id: Option<i64>,
    #[serde(default)]
    pub game_mode: Option<String>,
    pub group: String,
    pub rule: ModeRule,
    /// `Some(false)` = not currently available (kept so the choice returns with the mode).
    #[serde(default)]
    pub available: Option<bool>,
    /// Added automatically and not looked at yet ("New mode detected").
    #[serde(default)]
    pub is_new: bool,
    #[serde(default)]
    pub first_seen: Option<DateTime<Local>>,
}

/// The user's per-mode choices for one game.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct GameModes {
    /// Rule for modes seen for the first time (and when the mode can't be detected).
    pub unknown_rule: ModeRule,
    pub entries: BTreeMap<String, ModeEntry>,
    /// When the catalog was last refreshed from a live source.
    pub catalog_updated_at: Option<DateTime<Local>>,
}

impl Default for GameModes {
    fn default() -> Self {
        Self { unknown_rule: ModeRule::Record, entries: BTreeMap::new(), catalog_updated_at: None }
    }
}

/// What to do with a match, and why (shown in the tray / status).
#[derive(Debug, Clone, PartialEq)]
pub struct ModeDecision {
    pub rule: ModeRule,
    /// e.g. "ARAM: recording off for this mode".
    pub reason: String,
    /// A mode seen for the first time was added to the settings.
    pub added: bool,
}

impl GameModes {
    fn insert_new(&mut self, key: &str, name: &str, queue_id: Option<i64>, game_mode: Option<String>, group: &str, default_rule: Option<ModeRule>, available: Option<bool>) {
        let rule = default_rule.unwrap_or(self.unknown_rule);
        self.entries
            .insert(key.to_string(), ModeEntry { name: name.to_string(), queue_id, game_mode, group: group.to_string(), rule, available, is_new: true, first_seen: Some(Local::now()) });
    }

    /// Decides for a match that is starting. A mode seen for the first time is added (with the
    /// "unknown / new modes" rule, or its group's default for the built-in groups).
    pub fn decide(&mut self, m: Option<&MatchMode>) -> ModeDecision {
        let Some(m) = m else {
            return ModeDecision { rule: self.unknown_rule, reason: format!("Unknown mode: {}", self.unknown_rule.label()), added: false };
        };
        let mut added = false;
        let rule = match &m.key {
            Some(key) => {
                if let Some(e) = self.entries.get_mut(key) {
                    // Keep names fresh (Riot renames queues) and note it's playable.
                    if !m.name.is_empty() && m.source != "unknown" {
                        e.name = m.name.clone();
                    }
                    e.available = Some(true);
                    e.rule
                } else {
                    // New modes follow the "unknown / new modes" rule until the user changes it
                    // (modes the game module knows, like Ranked, get their built-in default).
                    self.insert_new(key, &m.name, m.queue_id, m.game_mode.clone(), &m.group, m.default_rule, Some(true));
                    if m.default_rule.is_some() {
                        self.entries.get_mut(key).unwrap().is_new = false;
                    }
                    added = true;
                    self.entries[key].rule
                }
            }
            // Only the coarse game type is known (e.g. ARAM without a queue id): use the most
            // permissive choice among modes of that type, so nothing wanted is missed.
            None => {
                let same: Vec<ModeRule> = self.entries.values().filter(|e| e.game_mode.is_some() && e.game_mode == m.game_mode).map(|e| e.rule).collect();
                same.into_iter().max_by_key(|r| r.rank()).unwrap_or(self.unknown_rule)
            }
        };
        let name = if m.name.is_empty() { "Unknown mode".to_string() } else { m.name.clone() };
        ModeDecision { rule, reason: format!("{name}: {} for this mode", rule.label()), added }
    }

    /// Adds modes from a catalog. `authoritative` = the source lists everything that's playable
    /// right now (the client's queue list): modes it doesn't list are marked unavailable.
    /// Returns how many modes were added.
    pub fn merge_catalog(&mut self, catalog: &[CatalogMode], authoritative: bool) -> usize {
        let mut added = 0;
        for c in catalog {
            match self.entries.get_mut(&c.key) {
                Some(e) => {
                    if !c.name.is_empty() {
                        e.name = c.name.clone();
                    }
                    // Grouping is derived data (it may improve with better sources); the
                    // user's rule is kept.
                    e.group = c.group.clone();
                    if c.available.is_some() {
                        e.available = c.available;
                    }
                    if e.game_mode.is_none() {
                        e.game_mode = c.game_mode.clone();
                    }
                }
                None => {
                    self.insert_new(&c.key, &c.name, c.queue_id, c.game_mode.clone(), &c.group, c.default_rule, c.available);
                    // Built-in defaults (Ranked, Normal, …) aren't "new" to the user.
                    if c.default_rule.is_some() {
                        if let Some(e) = self.entries.get_mut(&c.key) {
                            e.is_new = false;
                        }
                    }
                    added += 1;
                }
            }
        }
        if authoritative {
            let listed: std::collections::HashSet<&str> = catalog.iter().map(|c| c.key.as_str()).collect();
            for (k, e) in self.entries.iter_mut() {
                if !listed.contains(k.as_str()) {
                    e.available = Some(false);
                }
            }
            self.catalog_updated_at = Some(Local::now());
        }
        added
    }

    pub fn set_rule(&mut self, key: &str, rule: ModeRule) -> bool {
        match self.entries.get_mut(key) {
            Some(e) => {
                e.rule = rule;
                e.is_new = false;
                true
            }
            None => false,
        }
    }

    pub fn set_group(&mut self, group: &str, rule: ModeRule) {
        for e in self.entries.values_mut().filter(|e| e.group == group) {
            e.rule = rule;
            e.is_new = false;
        }
    }

    /// "everything", "ranked", "ranked_normal". Groups are the game module's ids.
    pub fn apply_preset(&mut self, preset: &str) {
        let on = |g: &str| match preset {
            "ranked" => g == "ranked",
            "ranked_normal" => g == "ranked" || g == "normal",
            _ => true,
        };
        for e in self.entries.values_mut() {
            e.rule = if on(&e.group) { ModeRule::Record } else { ModeRule::Off };
            e.is_new = false;
        }
        self.unknown_rule = if preset == "everything" { ModeRule::Record } else { ModeRule::Off };
    }

    pub fn clear_new(&mut self) {
        for e in self.entries.values_mut() {
            e.is_new = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cat(key: &str, name: &str, gm: &str, group: &str, def: Option<ModeRule>, avail: Option<bool>) -> CatalogMode {
        CatalogMode { key: key.into(), queue_id: key.trim_start_matches('q').parse().ok(), name: name.into(), game_mode: Some(gm.into()), group: group.into(), default_rule: def, available: avail }
    }
    fn mm(key: Option<&str>, name: &str, gm: &str) -> MatchMode {
        MatchMode { key: key.map(Into::into), queue_id: None, name: name.into(), game_mode: Some(gm.into()), group: "other".into(), default_rule: None, source: "test".into() }
    }

    #[test]
    fn decides_by_mode_and_adds_new_ones_with_the_unknown_rule() {
        let mut g = GameModes::default();
        g.merge_catalog(&[cat("q420", "Ranked Solo/Duo", "CLASSIC", "ranked", Some(ModeRule::Record), Some(true)), cat("q450", "ARAM", "ARAM", "aram", Some(ModeRule::Record), Some(true))], true);
        assert!(!g.entries["q420"].is_new, "built-in defaults aren't flagged new");
        g.set_rule("q450", ModeRule::Off);
        let d = g.decide(Some(&mm(Some("q450"), "ARAM", "ARAM")));
        assert_eq!(d.rule, ModeRule::Off);
        assert_eq!(d.reason, "ARAM: recording off for this mode");

        g.unknown_rule = ModeRule::ClipsOnly;
        let d = g.decide(Some(&mm(Some("q9999"), "Brand New Mode", "NEWMODE")));
        assert_eq!(d.rule, ModeRule::ClipsOnly);
        assert!(d.added && g.entries["q9999"].is_new);
        // Next time it's known.
        assert!(!g.decide(Some(&mm(Some("q9999"), "Brand New Mode", "NEWMODE"))).added);
    }

    #[test]
    fn coarse_fallback_uses_most_permissive_rule_of_that_type() {
        let mut g = GameModes::default();
        g.merge_catalog(
            &[
                cat("q420", "Ranked", "CLASSIC", "ranked", Some(ModeRule::Record), None),
                cat("q400", "Draft", "CLASSIC", "normal", Some(ModeRule::Off), None),
                cat("q450", "ARAM", "ARAM", "aram", Some(ModeRule::Off), None),
            ],
            false,
        );
        assert_eq!(g.decide(Some(&mm(None, "ARAM", "ARAM"))).rule, ModeRule::Off);
        assert_eq!(g.decide(Some(&mm(None, "Summoner's Rift", "CLASSIC"))).rule, ModeRule::Record);
        g.unknown_rule = ModeRule::Off;
        assert_eq!(g.decide(Some(&mm(None, "?", "WEIRD"))).rule, ModeRule::Off);
        assert_eq!(g.decide(None).rule, ModeRule::Off);
    }

    #[test]
    fn gone_modes_stay_but_are_marked_unavailable_and_come_back() {
        let mut g = GameModes::default();
        g.merge_catalog(&[cat("q900", "URF", "URF", "rotating", None, Some(true)), cat("q420", "Ranked", "CLASSIC", "ranked", Some(ModeRule::Record), Some(true))], true);
        g.set_rule("q900", ModeRule::ClipsOnly);
        g.merge_catalog(&[cat("q420", "Ranked Solo/Duo", "CLASSIC", "ranked", Some(ModeRule::Record), Some(true))], true);
        assert_eq!(g.entries["q900"].available, Some(false));
        assert_eq!(g.entries["q900"].rule, ModeRule::ClipsOnly, "choice remembered");
        assert_eq!(g.entries["q420"].name, "Ranked Solo/Duo", "renamed by Riot");
        g.merge_catalog(&[cat("q900", "URF", "URF", "rotating", None, Some(true))], false);
        assert_eq!(g.entries["q900"].available, Some(true));
        assert_eq!(g.entries["q900"].rule, ModeRule::ClipsOnly);
    }

    #[test]
    fn presets_and_groups() {
        let mut g = GameModes::default();
        g.merge_catalog(
            &[
                cat("q420", "R", "CLASSIC", "ranked", Some(ModeRule::Record), None),
                cat("q400", "D", "CLASSIC", "normal", Some(ModeRule::Record), None),
                cat("q450", "A", "ARAM", "aram", Some(ModeRule::Record), None),
            ],
            false,
        );
        g.apply_preset("ranked");
        assert_eq!((g.entries["q420"].rule, g.entries["q400"].rule, g.entries["q450"].rule, g.unknown_rule), (ModeRule::Record, ModeRule::Off, ModeRule::Off, ModeRule::Off));
        g.apply_preset("ranked_normal");
        assert_eq!(g.entries["q400"].rule, ModeRule::Record);
        g.set_group("aram", ModeRule::ClipsOnly);
        assert_eq!(g.entries["q450"].rule, ModeRule::ClipsOnly);
        g.apply_preset("everything");
        assert!(g.entries.values().all(|e| e.rule == ModeRule::Record) && g.unknown_rule == ModeRule::Record);
    }
}
