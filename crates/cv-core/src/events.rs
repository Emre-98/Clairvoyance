//! Shared event model. Game modules translate their own events into these kinds,
//! so the timeline, storage and UI never need to know which game an event came from.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    Kill,
    Death,
    Assist,
    Multikill,
    FirstBlood,
    Ace,
    /// Ult key pressed (live, not checked against the recording yet / couldn't be checked).
    UltPressed,
    /// Ult cast, confirmed from the recording (the ability bar shows it going on cooldown).
    UltUsed,
    /// Ult key pressed but no cast in the recording (on cooldown, cancelled, dead...). Hidden
    /// by default.
    UltUnconfirmed,
    /// A later press of the same ult (command a summon, second part, early end): not a new ult.
    /// Hidden by default.
    UltRecast,
    /// A form / stance swap (champions whose R swaps forms with a short cooldown). Hidden by
    /// default.
    FormSwap,
    Tower,
    Inhibitor,
    Dragon,
    Herald,
    Baron,
    /// Any other objective the game reports (e.g. Voidgrubs, Atakhan, bomb plant in CS2).
    Objective,
    /// A finished item bought (League: compared between player-list reads, undo-safe).
    ItemCompleted,
    /// A summoner spell cast (League: D / F press checked against the slot's cooldown).
    SummonerSpell,
    /// An ability key pressed, in games that can't tell live whether it was a cast (Deadlock:
    /// abilities 1-3; the ultimate is `UltPressed`). Hidden by default.
    AbilityPressed,
    /// The key of an active item slot pressed (Deadlock: item slots 1-4). Hidden by default.
    ItemPressed,
    /// The melee attack key pressed. Hidden by default.
    Melee,
    /// The parry / block key pressed. Hidden by default.
    Parry,
    /// Round/phase boundaries for games that have them (CS2 rounds).
    Round,
    ManualMarker,
    Clip,
    GameStart,
    GameEnd,
}

impl EventKind {
    pub const ALL: [EventKind; 28] = [
        EventKind::Kill,
        EventKind::Death,
        EventKind::Assist,
        EventKind::Multikill,
        EventKind::FirstBlood,
        EventKind::Ace,
        EventKind::UltPressed,
        EventKind::UltUsed,
        EventKind::UltUnconfirmed,
        EventKind::UltRecast,
        EventKind::FormSwap,
        EventKind::Tower,
        EventKind::Inhibitor,
        EventKind::Dragon,
        EventKind::Herald,
        EventKind::Baron,
        EventKind::Objective,
        EventKind::ItemCompleted,
        EventKind::SummonerSpell,
        EventKind::AbilityPressed,
        EventKind::ItemPressed,
        EventKind::Melee,
        EventKind::Parry,
        EventKind::Round,
        EventKind::ManualMarker,
        EventKind::Clip,
        EventKind::GameStart,
        EventKind::GameEnd,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            EventKind::Kill => "kill",
            EventKind::Death => "death",
            EventKind::Assist => "assist",
            EventKind::Multikill => "multikill",
            EventKind::FirstBlood => "first_blood",
            EventKind::Ace => "ace",
            EventKind::UltPressed => "ult_pressed",
            EventKind::UltUsed => "ult_used",
            EventKind::UltUnconfirmed => "ult_unconfirmed",
            EventKind::UltRecast => "ult_recast",
            EventKind::FormSwap => "form_swap",
            EventKind::Tower => "tower",
            EventKind::Inhibitor => "inhibitor",
            EventKind::Dragon => "dragon",
            EventKind::Herald => "herald",
            EventKind::Baron => "baron",
            EventKind::Objective => "objective",
            EventKind::ItemCompleted => "item_completed",
            EventKind::SummonerSpell => "summoner_spell",
            EventKind::AbilityPressed => "ability_pressed",
            EventKind::ItemPressed => "item_pressed",
            EventKind::Melee => "melee",
            EventKind::Parry => "parry",
            EventKind::Round => "round",
            EventKind::ManualMarker => "manual_marker",
            EventKind::Clip => "clip",
            EventKind::GameStart => "game_start",
            EventKind::GameEnd => "game_end",
        }
    }

    /// Default spoken callout for text-to-speech.
    pub fn callout(self) -> &'static str {
        match self {
            EventKind::Kill => "Kill",
            EventKind::Death => "Death",
            EventKind::Assist => "Assist",
            EventKind::Multikill => "Multikill",
            EventKind::FirstBlood => "First blood",
            EventKind::Ace => "Ace",
            EventKind::UltPressed => "Ult",
            EventKind::UltUsed => "Ult",
            EventKind::UltUnconfirmed => "Ult pressed",
            EventKind::UltRecast => "Recast",
            EventKind::FormSwap => "Form swap",
            EventKind::Tower => "Tower",
            EventKind::Inhibitor => "Inhibitor",
            EventKind::Dragon => "Dragon",
            EventKind::Herald => "Herald",
            EventKind::Baron => "Baron",
            EventKind::Objective => "Objective",
            EventKind::ItemCompleted => "Item",
            EventKind::SummonerSpell => "Summoner",
            EventKind::AbilityPressed => "Ability",
            EventKind::ItemPressed => "Item",
            EventKind::Melee => "Melee",
            EventKind::Parry => "Parry",
            EventKind::Round => "Round",
            EventKind::ManualMarker => "Marked",
            EventKind::Clip => "Clip saved",
            EventKind::GameStart => "Game started",
            EventKind::GameEnd => "Game over",
        }
    }
}

/// One moment in a game, e.g. "Killed Ahri at 12:34".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GameEvent {
    /// Unique id. For API events it is derived from the game's own event id,
    /// so the same event is never handled twice.
    pub id: String,
    pub kind: EventKind,
    /// Seconds on the in-game clock.
    pub game_time: f64,
    /// Short label for the marker, e.g. "Killed Ahri".
    pub title: String,
    /// Extra hover info (assisters, dragon type, ...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
    /// True if an objective was stolen.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub steal: bool,
    /// A picture for the hover card (League: the item / summoner spell from Data Dragon).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<EventIcon>,
    /// Labelled details for the hover card, e.g. ("Lane", "Mid"), ("Gold", "≈ +250").
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub facts: Vec<(String, String)>,
    /// Characters involved, first the main one (League: champion ids, for portraits).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub who: Vec<String>,
}

/// An image the UI can show for an event: `kind` "item" / "spell" / "champion", `id` the game's
/// id ("3031", "SummonerFlash", "Ahri"), `version` the data version it belongs to (League: Data
/// Dragon "16.19.1"; empty = the newest).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventIcon {
    pub kind: String,
    pub id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub version: String,
}

impl GameEvent {
    pub fn new(id: impl Into<String>, kind: EventKind, game_time: f64, title: impl Into<String>) -> Self {
        Self { id: id.into(), kind, game_time, title: title.into(), details: None, steal: false, icon: None, facts: Vec::new(), who: Vec::new() }
    }
    pub fn with_icon(mut self, kind: &str, id: impl Into<String>, version: impl Into<String>) -> Self {
        self.icon = Some(EventIcon { kind: kind.into(), id: id.into(), version: version.into() });
        self
    }
    pub fn fact(mut self, label: impl Into<String>, value: impl Into<String>) -> Self {
        let v = value.into();
        if !v.is_empty() {
            self.facts.push((label.into(), v));
        }
        self
    }
    pub fn with_who(mut self, who: Vec<String>) -> Self {
        self.who = who;
        self
    }
    pub fn with_details(mut self, d: impl Into<String>) -> Self {
        let d = d.into();
        if !d.is_empty() {
            self.details = Some(d);
        }
        self
    }
    pub fn stolen(mut self, steal: bool) -> Self {
        self.steal = steal;
        self
    }
}

/// Formats seconds as m:ss (or h:mm:ss).
pub fn fmt_clock(secs: f64) -> String {
    let s = secs.max(0.0).round() as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clock() {
        assert_eq!(fmt_clock(754.4), "12:34");
        assert_eq!(fmt_clock(3725.0), "1:02:05");
        assert_eq!(fmt_clock(-3.0), "0:00");
    }
    #[test]
    fn kind_serde_matches_as_str() {
        for k in EventKind::ALL {
            let s = serde_json::to_string(&k).unwrap();
            assert_eq!(s, format!("\"{}\"", k.as_str()));
        }
    }
}
