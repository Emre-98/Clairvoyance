//! League's "action keys" for the replay's ability bubbles: spells Q W E R, summoners D F, item
//! slots 1-6 and the trinket (ward), each with every bind League has for it (normal, quick,
//! quick-with-indicator, self and normal-cast variants, several binds per action, modifiers,
//! mouse buttons), read from League's own settings with the same code as the ult tracking
//! ([`crate::ult::read_input_settings`]: `PersistedSettings.json` first, `input.ini` fallback).
//!
//! Only exact binds count: Ctrl+Q (level up) or any other combination League doesn't cast with
//! gives no bubble. The bubble shows the action ("Q", "4", a ward), never the physical key; a
//! small hint shows the key when it isn't League's default for that action.
//!
//! Ult tie-in ([`press_states`]): R presses take the result of the post-game ult check (v1.3;
//! v1.6: recasts of the same ult are smaller outlined bubbles):
//! "Ult used" → solid, "Ult pressed, no cast" → faded (hidden with "Unconfirmed presses").

use crate::ult::{self, key_name};
use cv_core::game::KeyMark;
use cv_core::input::actions::{vk_from_name, ActionBind, ActionCategory, ActionKey, ActionPress, PressState};
use cv_core::session::GameSession;
use cv_core::EventKind;
use std::collections::HashMap;
use std::path::Path;

/// Variants of a cast action: (prefix, default modifiers: None = no default bind).
/// League's defaults: plain key = cast, Shift = quick cast, Alt = self cast.
const VARIANTS: [(&str, Option<(bool, bool, bool)>); 7] = [
    ("evtCast", Some((false, false, false))),
    ("evtSmartCast", Some((false, true, false))),
    ("evtSmartCastWithIndicator", None),
    ("evtSelfCast", Some((false, false, true))),
    ("evtSmartPlusSelfCast", None),
    ("evtSmartPlusSelfCastWithIndicator", None),
    ("evtNormalCast", None),
];

struct Slot {
    id: &'static str,
    label: &'static str,
    icon: Option<&'static str>,
    category: &'static str,
    size: f32,
    color: &'static str,
    /// League's default key (lower case, as in input.ini).
    key: &'static str,
    /// Action name suffixes, e.g. "Spell1"; `use_names` = the plain "use" action (items).
    suffixes: &'static [&'static str],
    use_names: &'static [&'static str],
}

const SLOTS: [Slot; 13] = [
    Slot { id: "spell1", label: "Q", icon: None, category: "ability", size: 1.0, color: "#3b82f6", key: "q", suffixes: &["Spell1"], use_names: &[] },
    Slot { id: "spell2", label: "W", icon: None, category: "ability", size: 1.0, color: "#22c55e", key: "w", suffixes: &["Spell2"], use_names: &[] },
    Slot { id: "spell3", label: "E", icon: None, category: "ability", size: 1.0, color: "#f59e0b", key: "e", suffixes: &["Spell3"], use_names: &[] },
    Slot { id: "spell4", label: "R", icon: None, category: "ability", size: 1.18, color: "#a855f7", key: "r", suffixes: &["Spell4"], use_names: &[] },
    Slot { id: "summoner1", label: "D", icon: None, category: "summoner", size: 1.18, color: "#f43f5e", key: "d", suffixes: &["AvatarSpell1"], use_names: &[] },
    Slot { id: "summoner2", label: "F", icon: None, category: "summoner", size: 1.18, color: "#14b8a6", key: "f", suffixes: &["AvatarSpell2"], use_names: &[] },
    Slot { id: "item1", label: "1", icon: None, category: "item", size: 0.84, color: "#64748b", key: "1", suffixes: &["Item1"], use_names: &["evtUseItem1"] },
    Slot { id: "item2", label: "2", icon: None, category: "item", size: 0.84, color: "#64748b", key: "2", suffixes: &["Item2"], use_names: &["evtUseItem2"] },
    Slot { id: "item3", label: "3", icon: None, category: "item", size: 0.84, color: "#64748b", key: "3", suffixes: &["Item3"], use_names: &["evtUseItem3"] },
    Slot { id: "item4", label: "4", icon: None, category: "item", size: 0.84, color: "#64748b", key: "5", suffixes: &["Item4"], use_names: &["evtUseItem4"] },
    Slot { id: "item5", label: "5", icon: None, category: "item", size: 0.84, color: "#64748b", key: "6", suffixes: &["Item5"], use_names: &["evtUseItem5"] },
    Slot { id: "item6", label: "6", icon: None, category: "item", size: 0.84, color: "#64748b", key: "7", suffixes: &["Item6"], use_names: &["evtUseItem6"] },
    // Not "Item7": League 16.x uses that for another slot (the owner's file has it on Right Arrow
    // and "=" next to the trinket on C).
    Slot { id: "ward", label: "Ward", icon: Some("ward"), category: "ward", size: 0.84, color: "#eab308", key: "4", suffixes: &["VisionItem"], use_names: &["evtUseVisionItem"] },
];

pub fn categories() -> Vec<ActionCategory> {
    [("ability", "Abilities"), ("summoner", "Summoners"), ("item", "Items"), ("ward", "Ward")]
        .into_iter()
        .map(|(id, label)| ActionCategory { id: id.into(), label: label.into() })
        .collect()
}

/// Parses one bind value into binds with modifiers: `[Alt][q]`, `[q],[Shift][q]`, `[Button 4]`,
/// `[Ctrl][Button 5]`, `null`, `[<Unbound>]`.
pub fn parse_binds(v: &str) -> Vec<ActionBind> {
    let v = v.trim();
    let mut out = Vec::new();
    if v.is_empty() || v == "null" {
        return out;
    }
    let mut parts = Vec::new();
    let (mut depth, mut cur) = (0i32, String::new());
    for c in v.chars() {
        match c {
            '[' => {
                depth += 1;
                cur.push(c);
            }
            ']' => {
                depth -= 1;
                cur.push(c);
            }
            ',' if depth <= 0 => parts.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    parts.push(cur);
    for p in parts {
        let tokens: Vec<String> = p.split('[').filter_map(|t| t.split(']').next()).map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect();
        let (mut ctrl, mut shift, mut alt) = (false, false, false);
        let mut target: Option<ActionBind> = None;
        let mut ok = !tokens.is_empty();
        for t in &tokens {
            match t.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => ctrl = true,
                "shift" => shift = true,
                "alt" => alt = true,
                "<unbound>" => ok = false,
                _ => {
                    let arrow = match t.to_ascii_lowercase().as_str() {
                        "left arrow" | "left" => Some(0x25),
                        "up arrow" | "up" => Some(0x26),
                        "right arrow" | "right" => Some(0x27),
                        "down arrow" | "down" => Some(0x28),
                        _ => None,
                    };
                    if let Some(n) = ult::mouse_button_number(t) {
                        target = Some(ActionBind::mouse(n));
                    } else if let Some(vk) = arrow {
                        target = Some(ActionBind::key(vk));
                    } else {
                        match key_name(t).and_then(|k| vk_from_name(&k)) {
                            Some(vk) => target = Some(ActionBind::key(vk)),
                            None => ok = false,
                        }
                    }
                }
            }
        }
        if let (true, Some(b)) = (ok, target) {
            let b = b.with(ctrl, shift, alt);
            if !out.contains(&b) {
                out.push(b);
            }
        }
    }
    out
}

fn default_bind(key: &str, mods: (bool, bool, bool)) -> String {
    let mut s = String::new();
    for (on, m) in [(mods.0, "[Ctrl]"), (mods.1, "[Shift]"), (mods.2, "[Alt]")] {
        if on {
            s.push_str(m);
        }
    }
    format!("{s}[{key}]")
}

/// The action keys from League's key bind settings (`name=value` lines of input.ini or
/// PersistedSettings.json's Input.ini). Missing settings use League's defaults.
pub fn parse_actions(text: &str) -> Vec<ActionKey> {
    let mut values: HashMap<String, String> = HashMap::new();
    for line in text.lines() {
        if let Some((k, v)) = line.split_once('=') {
            values.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
    let get = |name: &str| values.get(&name.to_ascii_lowercase()).cloned();
    SLOTS
        .iter()
        .map(|s| {
            let mut binds: Vec<ActionBind> = Vec::new();
            let mut add = |v: &str| {
                for b in parse_binds(v) {
                    if !binds.contains(&b) {
                        binds.push(b);
                    }
                }
            };
            // Items/ward: the plain bind is "evtUseItemN" / "evtUseVisionItem"; the first suffix is
            // the main one (defaults apply to it only).
            for (k, (prefix, def)) in VARIANTS.iter().enumerate() {
                for (i, suffix) in s.suffixes.iter().enumerate() {
                    let names: Vec<String> =
                        if k == 0 && !s.use_names.is_empty() { vec![s.use_names[i.min(s.use_names.len() - 1)].to_string(), format!("{prefix}{suffix}")] } else { vec![format!("{prefix}{suffix}")] };
                    let mut any = false;
                    for n in &names {
                        if let Some(v) = get(n) {
                            any = true;
                            add(&v);
                        }
                    }
                    if !any && i == 0 {
                        if let Some(m) = def {
                            add(&default_bind(s.key, *m));
                        }
                    }
                }
            }
            ActionKey {
                id: s.id.into(),
                label: s.label.into(),
                icon: s.icon.map(str::to_string),
                category: s.category.into(),
                size: s.size,
                color: s.color.into(),
                default_key: s.key.to_ascii_uppercase(),
                binds,
            }
        })
        .collect()
}

/// League's defaults (no settings file, or recordings made before the binds were saved).
pub fn default_actions() -> Vec<ActionKey> {
    parse_actions("")
}

/// The action keys of the League install (`None` when its settings can't be read).
pub fn read_actions(install_dir: &Path) -> Option<Vec<ActionKey>> {
    ult::read_input_settings(install_dir).map(|t| parse_actions(&t))
}

/// Adds the "Ult key" setting's manual key (when set) to R.
pub fn with_manual_ult(mut actions: Vec<ActionKey>, manual: Option<&ult::Bind>) -> Vec<ActionKey> {
    if let Some(m) = manual {
        if let (Some(r), Some(vk)) = (actions.iter_mut().find(|a| a.id == "spell4"), vk_from_name(&m.key)) {
            let b = ActionBind::key(vk);
            if !r.binds.contains(&b) {
                r.binds.push(b);
            }
        }
    }
    actions
}

/// R presses follow the ult check of the recording, so the bubbles never contradict the ult
/// markers: each R press is paired with its logged ult press (`key_presses`); after a check
/// ("verified"), a press the check marked "Ult pressed, no cast" is faded, the others are
/// solid ("Ult used"); without a check, live-accepted presses are solid and filtered ones
/// faded (they have no marker either). R presses that were never logged as ult presses
/// (loading screen, key bounce) are faded.
pub fn press_states(s: &GameSession, actions: &[ActionKey], presses: &mut [ActionPress]) {
    let Some(r) = actions.iter().position(|a| a.id == "spell4") else { return };
    let marks: Vec<&KeyMark> = s.key_presses.iter().filter(|m| m.action == "ult" && !m.key.starts_with("Mouse")).collect();
    let mouse_marks: Vec<&KeyMark> = s.key_presses.iter().filter(|m| m.action == "ult" && m.key.starts_with("Mouse")).collect();
    let r_idx: Vec<usize> = (0..presses.len()).filter(|&i| presses[i].action == r).collect();
    if r_idx.is_empty() || (marks.is_empty() && mouse_marks.is_empty()) {
        return;
    }
    let verified = s.verification.as_ref().is_some_and(|v| v.status == "verified");
    let no_cast: Vec<f64> = s.events.iter().filter(|e| e.kind == EventKind::UltUnconfirmed).map(|e| e.game_time).collect();
    // v1.6: later presses of the same ult (recast / command) are small outlined bubbles.
    let recast: Vec<f64> = s.events.iter().filter(|e| e.kind == EventKind::UltRecast).map(|e| e.game_time).collect();
    let offset = s.video_offset;
    let state_of = |m: &KeyMark| -> PressState {
        if verified {
            if no_cast.iter().any(|&g| (g - m.game_time).abs() < 0.005) {
                PressState::Unconfirmed
            } else if recast.iter().any(|&g| (g - m.game_time).abs() < 0.005) {
                PressState::Recast
            } else {
                PressState::Confirmed
            }
        } else if m.accepted && m.reason.as_deref() == Some("recast") {
            PressState::Recast
        } else if m.accepted {
            PressState::Normal
        } else {
            PressState::Unconfirmed
        }
    };
    // Key presses: the logged game times come from the game clock estimate, the input times
    // from the recording's clock; correct the (constant) difference first, then pair nearest.
    for (list, is_mouse) in [(&marks, false), (&mouse_marks, true)] {
        let idx: Vec<usize> = r_idx.iter().copied().filter(|&i| (presses[i].button > 0) == is_mouse).collect();
        if idx.is_empty() {
            continue;
        }
        let times: Vec<f64> = list.iter().map(|m| m.game_time + offset).collect();
        let mut diffs: Vec<f64> = idx
            .iter()
            .filter_map(|&i| times.iter().map(|&t| presses[i].t - t).min_by(|a, b| a.abs().total_cmp(&b.abs())))
            .filter(|d| d.abs() < 0.3)
            .collect();
        diffs.sort_by(|a, b| a.total_cmp(b));
        let shift = if diffs.is_empty() { 0.0 } else { diffs[diffs.len() / 2] };
        let mut used = vec![false; list.len()];
        for &i in &idx {
            let t = presses[i].t - shift;
            let best = (0..list.len()).filter(|&k| !used[k] && (times[k] - t).abs() < 0.12).min_by(|&a, &b| (times[a] - t).abs().total_cmp(&(times[b] - t).abs()));
            presses[i].state = match best {
                Some(k) => {
                    used[k] = true;
                    state_of(list[k])
                }
                None => PressState::Unconfirmed,
            };
        }
    }
}

#[cfg(test)]
mod tests;
