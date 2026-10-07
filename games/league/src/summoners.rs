//! Summoner spell chips ("Flash", "Ignite"...), own champion only. The API exposes neither
//! summoner casts nor their cooldowns (Overwolf's League events don't have them either), so,
//! like the ult ([`crate::ult`], [`crate::verify`]), two layers:
//! 1. Live: a press of a summoner key (D / F with League's own binds: normal, quick and self
//!    cast variants) while not typing in chat or dead, and not within the spell's cooldown since
//!    the last accepted press, gives a "Flash" chip at once. Every press is kept as a key mark
//!    (action "summoner1" / "summoner2").
//! 2. After the game: the D / F slot's cooldown overlay in the recording decides (the same
//!    reading as the R icon, [`crate::hud::look`]): a chip at the frame the cooldown appears,
//!    presses without one are dropped.
//!
//! The spell's name comes from the player list (`summonerSpells.summonerSpellOne` = D slot),
//! read again during the game (Smite and Teleport upgrade, Arena swaps them).

use crate::ddragon::StaticData;
use cv_core::game::{KeyMark, KeyPress};
use cv_core::input::actions::{vk_from_name, ActionKey};
use cv_core::{EventKind, GameEvent};

/// A summoner spell of a slot: Data Dragon id ("SummonerFlash") and display name ("Flash").
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct Spell {
    pub id: String,
    pub name: String,
}

/// Id from the player list's `rawDescription`: "GeneratedTip_SummonerSpell_SummonerFlash_Description".
pub fn spell_id(raw_description: &str, display_name: &str) -> String {
    let parts: Vec<&str> = raw_description.split('_').collect();
    if parts.len() >= 4 && parts[1] == "SummonerSpell" {
        // Ids can contain underscores ("SummonerSnowURFSnowball_Mark"): everything between.
        return parts[2..parts.len() - 1].join("_");
    }
    match display_name {
        "" => String::new(),
        n => format!("Summoner{}", n.replace(' ', "")),
    }
}

/// Slot ids as in the action keys.
pub const SLOTS: [&str; 2] = ["summoner1", "summoner2"];

#[derive(Debug, Default)]
pub struct SummonerTracker {
    /// D and F spells (from the player list).
    pub spells: [Option<Spell>; 2],
    /// Game time of the last accepted press per slot.
    last: [Option<f64>; 2],
    last_any: Option<f64>,
    marks: Vec<KeyMark>,
}

impl SummonerTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Which summoner slot a key press casts (0 = D, 1 = F) with these binds.
    pub fn slot_of(actions: &[ActionKey], k: &KeyPress) -> Option<usize> {
        let vk = vk_from_name(&k.key)?;
        SLOTS.iter().position(|id| {
            actions
                .iter()
                .find(|a| a.id == *id)
                .is_some_and(|a| a.binds.iter().any(|b| b.button == 0 && b.vk == vk && b.ctrl == k.ctrl && b.shift == k.shift && b.alt == k.alt))
        })
    }

    /// A key press: an event for an accepted summoner cast (live, checked after the game).
    pub fn press(&mut self, actions: &[ActionKey], k: &KeyPress, gt: f64, chat: bool, dead: bool, data: Option<&StaticData>) -> Option<GameEvent> {
        let slot = Self::slot_of(actions, k)?;
        if self.last_any.is_some_and(|t| (gt - t).abs() < 0.08) {
            return None;
        }
        self.last_any = Some(gt);
        let spell = self.spells[slot].clone().unwrap_or_default();
        // Summoner haste (boots, runes) can take up to ~40 % off; a bit less to be safe.
        let cd = data.and_then(|d| d.spell(&spell.id)).map(|s| s.cooldown).filter(|c| *c > 0.0);
        let reason = if chat {
            Some("chat".to_string())
        } else if dead {
            Some("dead".to_string())
        } else {
            match (self.last[slot], cd) {
                (Some(t), Some(cd)) if gt - t < cd * 0.55 => Some(format!("cooldown ({:.0} s left)", cd - (gt - t))),
                (Some(t), None) if gt - t < 1.0 => Some("repeat".to_string()),
                _ => None,
            }
        };
        let key = crate::ult::press_label(k);
        let accepted = reason.is_none();
        self.marks.push(KeyMark { game_time: gt, action: SLOTS[slot].into(), key: key.clone(), accepted, reason: reason.clone() });
        if !accepted {
            return None;
        }
        self.last[slot] = Some(gt);
        Some(event(slot, &spell, gt, data.map(|d| d.version.as_str()).unwrap_or(""), &key, false))
    }

    pub fn take_marks(&mut self) -> Vec<KeyMark> {
        std::mem::take(&mut self.marks)
    }
}

/// A summoner chip: live (`checked` false) or confirmed from the recording.
pub fn event(slot: usize, spell: &Spell, gt: f64, version: &str, key: &str, checked: bool) -> GameEvent {
    let name = if spell.name.is_empty() { format!("Summoner spell {}", ["D", "F"][slot]) } else { spell.name.clone() };
    let mut e = GameEvent::new(format!("sum{}-{gt:.2}", slot + 1), EventKind::SummonerSpell, gt, name)
        .fact("Slot", ["D", "F"][slot])
        .fact("Key", key)
        .with_details(if checked { "Confirmed from the recording (the spell went on cooldown)." } else { "Key pressed. Checked against the recording after the game." });
    if !spell.id.is_empty() {
        e = e.with_icon("spell", spell.id.clone(), version);
    }
    e
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ddragon::{parse_spells, StaticData};

    fn kp(k: &str) -> KeyPress {
        KeyPress { key: k.into(), ctrl: false, shift: false, alt: false }
    }

    #[test]
    fn ids_from_the_player_list() {
        assert_eq!(spell_id("GeneratedTip_SummonerSpell_SummonerFlash_Description", "Flash"), "SummonerFlash");
        assert_eq!(spell_id("GeneratedTip_SummonerSpell_SummonerSnowURFSnowball_Mark_Description", "Mark"), "SummonerSnowURFSnowball_Mark");
        assert_eq!(spell_id("", "Ignite"), "SummonerIgnite");
    }

    #[test]
    fn presses_with_cooldown_chat_and_binds() {
        let actions = crate::actions::default_actions();
        let data = StaticData { version: "16.20.1".into(), spells: parse_spells(include_str!("../tests/ddragon/summoner-16.20.json")), ..Default::default() };
        let mut t = SummonerTracker::new();
        t.spells = [Some(Spell { id: "SummonerFlash".into(), name: "Flash".into() }), Some(Spell { id: "SummonerDot".into(), name: "Ignite".into() })];
        let e = t.press(&actions, &kp("D"), 100.0, false, false, Some(&data)).expect("flash");
        assert_eq!(e.title, "Flash");
        assert_eq!(e.kind, EventKind::SummonerSpell);
        assert_eq!(e.icon.as_ref().map(|i| i.id.as_str()), Some("SummonerFlash"));
        assert!(t.press(&actions, &kp("D"), 101.0, false, false, Some(&data)).is_none(), "on cooldown");
        assert!(t.press(&actions, &kp("F"), 102.0, true, false, Some(&data)).is_none(), "chat");
        assert!(t.press(&actions, &kp("F"), 103.0, false, true, Some(&data)).is_none(), "dead");
        assert_eq!(t.press(&actions, &kp("F"), 104.0, false, false, Some(&data)).map(|e| e.title), Some("Ignite".into()));
        assert!(t.press(&actions, &kp("Q"), 105.0, false, false, Some(&data)).is_none());
        let ctrl_d = KeyPress { ctrl: true, ..kp("D") };
        assert!(t.press(&actions, &ctrl_d, 106.0, false, false, Some(&data)).is_none(), "not a summoner bind");
        // Flash again after its cooldown (with summoner haste it can be < 300 s).
        assert!(t.press(&actions, &kp("D"), 100.0 + 250.0, false, false, Some(&data)).is_some());
        let m = t.take_marks();
        assert_eq!(m.iter().filter(|m| m.accepted).count(), 3);
        assert_eq!(m.iter().filter(|m| !m.accepted).count(), 3);
        assert!(m.iter().any(|m| m.reason.as_deref().is_some_and(|r| r.starts_with("cooldown"))));
        // Quick cast (Shift+D by default) counts too.
        let mut t = SummonerTracker::new();
        assert!(t.press(&actions, &KeyPress { shift: true, ..kp("D") }, 10.0, false, false, None).is_some(), "no data: still a chip");
    }
}
