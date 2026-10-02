//! "Action keys": the game's own actions (League: abilities, summoners, item slots, ward) and the
//! keys or mouse buttons bound to them, for the replay's ability bubbles.
//!
//! Game-agnostic: a game module declares its actions with a label, category, size and colour
//! ([`crate::GameIntegration::action_keys`]); the core turns the key and mouse records of the
//! input file into [`ActionPress`]es: the exact key-down time, the cursor position at that time
//! (interpolated between the two surrounding cursor samples) and the video frame it belongs to.
//! Nothing here runs during a game: only when a replay's overlay is switched on.

use super::stats::{Analysis, Move};
use serde::{Deserialize, Serialize};

/// One key or mouse button (with modifiers) bound to an action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionBind {
    /// Virtual-key code (0 = a mouse button).
    #[serde(default)]
    pub vk: u8,
    /// Mouse button (1 left, 2 right, 3 middle, 4/5 side; 0 = a key).
    #[serde(default)]
    pub button: u8,
    #[serde(default)]
    pub ctrl: bool,
    #[serde(default)]
    pub shift: bool,
    #[serde(default)]
    pub alt: bool,
}

impl ActionBind {
    pub fn key(vk: u8) -> Self {
        ActionBind { vk, button: 0, ctrl: false, shift: false, alt: false }
    }
    pub fn mouse(button: u8) -> Self {
        ActionBind { vk: 0, button, ctrl: false, shift: false, alt: false }
    }
    pub fn with(mut self, ctrl: bool, shift: bool, alt: bool) -> Self {
        self.ctrl = ctrl;
        self.shift = shift;
        self.alt = alt;
        self
    }
    fn matches(&self, vk: u8, button: u8, m: Mods) -> bool {
        self.vk == vk && self.button == button && self.ctrl == m.ctrl && self.shift == m.shift && self.alt == m.alt
    }
    /// The physical key or button, without modifiers ("Q", "5", "M4").
    pub fn physical(&self) -> String {
        if self.button > 0 {
            format!("M{}", self.button)
        } else {
            short_key_label(self.vk)
        }
    }
}

/// An action a key can trigger (League: "spell1" = Q, "item4", "ward", ...).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionKey {
    /// Stable id, e.g. "spell4".
    pub id: String,
    /// What the bubble shows, e.g. "R" or "4" (the action, not the physical key).
    pub label: String,
    /// Drawn instead of the label (the UI knows "ward").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Group for the overlay's sub-toggles, e.g. "ability", "summoner", "item", "ward".
    pub category: String,
    /// Relative bubble size (1 = normal).
    #[serde(default = "one")]
    pub size: f32,
    /// Bubble colour (CSS).
    pub color: String,
    /// The physical key that needs no hint (the game's default key for this action). A press
    /// on any other key that isn't the label itself shows that key as a small hint.
    #[serde(default)]
    pub default_key: String,
    pub binds: Vec<ActionBind>,
}

fn one() -> f32 {
    1.0
}

/// A sub-toggle of the overlay's "Ability bubbles" group.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionCategory {
    pub id: String,
    pub label: String,
}

/// How a press is drawn (games can refine it, e.g. League's ult check).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PressState {
    /// A normal (solid) bubble.
    #[default]
    Normal,
    /// Confirmed by a check of the recording (solid).
    Confirmed,
    /// Pressed, but the recording shows no cast (faded/outlined; hidden with the timeline's
    /// "Unconfirmed presses" filter).
    Unconfirmed,
}

/// One press of an action key during the game.
#[derive(Debug, Clone, PartialEq)]
pub struct ActionPress {
    /// Key-down time (video seconds, the recording's own clock).
    pub t: f64,
    /// Video time of the frame that time belongs to (the frame on screen when it happened): the
    /// bubble appears on exactly this frame.
    pub show: f64,
    /// Cursor position at `t` (0..1 of the game's client area), interpolated.
    pub x: f32,
    pub y: f32,
    /// Index into the action list.
    pub action: usize,
    /// The physical key, when it isn't the action's default key ("A" for a Q rebound to A).
    pub hint: Option<String>,
    /// The mouse button it was bound to (0 = a key).
    pub button: u8,
    pub state: PressState,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Mods {
    ctrl: bool,
    shift: bool,
    alt: bool,
}

fn is_shift(vk: u8) -> bool {
    matches!(vk, 0x10 | 0xA0 | 0xA1)
}
fn is_ctrl(vk: u8) -> bool {
    matches!(vk, 0x11 | 0xA2 | 0xA3)
}
fn is_alt(vk: u8) -> bool {
    matches!(vk, 0x12 | 0xA4 | 0xA5)
}

/// Cursor position at `t` (0..1 of the client area): linear between the two samples around it.
/// The capture stores a sample only when the cursor moved, so a long gap before a sample means
/// the cursor rested at the earlier position until about one sample period before the later
/// one; the movement is interpolated over that last period only. No sample before `t`, or a new
/// stroke starting after it (focus came back, window moved): the nearest sample on `t`'s side.
pub fn cursor_at(moves: &[Move], t: f64, rate: u32) -> Option<(f32, f32)> {
    let i = moves.partition_point(|m| m.t <= t);
    if i == 0 {
        return None;
    }
    let a = moves[i - 1];
    let Some(&b) = moves.get(i) else { return Some((a.x, a.y)) };
    if b.brk || b.t <= a.t {
        return Some((a.x, a.y));
    }
    let period = 1.0 / rate.max(1) as f64;
    // Sampled at `period` while moving; a longer gap = at rest, then moved within one period.
    let start = if b.t - a.t > 1.5 * period { b.t - period } else { a.t };
    if t <= start {
        return Some((a.x, a.y));
    }
    let f = ((t - start) / (b.t - start)).clamp(0.0, 1.0) as f32;
    Some((a.x + (b.x - a.x) * f, a.y + (b.y - a.y) * f))
}

/// The video frame a time belongs to: the last frame starting at or before it (frame times
/// sorted). Without frame times (unknown layout), the time itself.
pub fn frame_of(frames: &[f64], t: f64) -> f64 {
    if frames.is_empty() {
        return t;
    }
    // A hair of tolerance: frame times and input times are both whole 100 ns ticks.
    let i = frames.partition_point(|&f| f <= t + 1e-7);
    if i == 0 {
        frames[0].min(t)
    } else {
        frames[i - 1]
    }
}

/// The action presses of a game: every key-down (and mouse-button press) whose key and
/// modifiers exactly match one of an action's binds. A modifier combination no action has
/// (League: Ctrl+Q levels Q up) gives nothing. Modifier state comes from the recorded key
/// records and is reset when focus changes or the chat closes (keys aren't recorded meanwhile).
/// Presses outside the focused time or before the first cursor sample are skipped (no position).
pub fn presses(an: &Analysis, actions: &[ActionKey], rate: u32, frames: &[f64]) -> Vec<ActionPress> {
    if actions.is_empty() {
        return Vec::new();
    }
    let has_mouse = actions.iter().any(|a| a.binds.iter().any(|b| b.button > 0));
    // (time, vk, button, down), merged in time order (keys first on equal times so a modifier
    // pressed with the click counts).
    let mut evs: Vec<(f64, u8, u8, bool)> = an.keys.iter().map(|k| (k.t, k.vk, 0u8, k.down)).collect();
    if has_mouse {
        evs.extend(an.clicks.iter().filter(|c| c.down).map(|c| (c.t, 0u8, c.button, true)));
        evs.sort_by(|a, b| a.0.total_cmp(&b.0).then((a.2 > 0).cmp(&(b.2 > 0))));
    }
    // Moments when held keys are forgotten: focus gained/lost, chat closed.
    let mut resets: Vec<f64> = an.focus.iter().flat_map(|&(s, e)| [s, e]).chain(an.chat.iter().map(|c| c.1)).collect();
    resets.sort_by(|a, b| a.total_cmp(b));
    let mut ri = 0;
    let mut held = [false; 256];
    let mut out = Vec::new();
    for (t, vk, button, down) in evs {
        while ri < resets.len() && resets[ri] <= t {
            held = [false; 256];
            ri += 1;
        }
        if button == 0 {
            held[vk as usize] = down;
            if !down || is_shift(vk) || is_ctrl(vk) || is_alt(vk) {
                continue;
            }
        }
        let m = Mods {
            ctrl: [0x11, 0xA2, 0xA3].iter().any(|&k| held[k]),
            shift: [0x10, 0xA0, 0xA1].iter().any(|&k| held[k]),
            alt: [0x12, 0xA4, 0xA5].iter().any(|&k| held[k]),
        };
        let found = actions.iter().enumerate().find_map(|(i, a)| a.binds.iter().find(|b| b.matches(vk, button, m)).map(|b| (i, b)));
        let Some((ai, bind)) = found else { continue };
        if !an.focus.is_empty() && !an.focus.iter().any(|&(s, e)| t >= s && t <= e) {
            continue;
        }
        let Some((x, y)) = cursor_at(&an.moves, t, rate) else { continue };
        let phys = bind.physical();
        let a = &actions[ai];
        // A hint only when the key says something the label doesn't: not the action's default key
        // (item slot 4 on 5) and not the label itself (item slot 4 rebound to 4).
        let hint = (!a.default_key.is_empty() && !phys.eq_ignore_ascii_case(&a.default_key) && !phys.eq_ignore_ascii_case(&a.label)).then_some(phys);
        out.push(ActionPress { t, show: frame_of(frames, t), x, y, action: ai, hint, button, state: PressState::Normal });
    }
    out
}

/// Virtual-key code of a key name as the input thread reports them ("Q", "5", "F3", "Num4",
/// "Space", "Key189", "`").
pub fn vk_from_name(name: &str) -> Option<u8> {
    let n = name.trim();
    if n.len() == 1 {
        let c = n.chars().next()?;
        if c.is_ascii_alphanumeric() {
            return Some(c.to_ascii_uppercase() as u8);
        }
        return match c {
            '`' => Some(0xC0),
            ';' => Some(186),
            '=' => Some(187),
            ',' => Some(188),
            '-' => Some(189),
            '.' => Some(190),
            '/' => Some(191),
            '[' => Some(219),
            '\\' => Some(220),
            ']' => Some(221),
            '\'' => Some(222),
            _ => None,
        };
    }
    if let Some(v) = n.strip_prefix("Key").and_then(|v| v.parse::<u16>().ok()) {
        return (1..=255).contains(&v).then_some(v as u8);
    }
    if let Some(v) = n.strip_prefix('F').and_then(|v| v.parse::<u8>().ok()) {
        return (1..=24).contains(&v).then_some(0x6F + v);
    }
    if let Some(v) = n.strip_prefix("Num").and_then(|v| v.parse::<u8>().ok()) {
        return (v <= 9).then_some(0x60 + v);
    }
    Some(match n.to_ascii_lowercase().as_str() {
        "space" => 0x20,
        "tab" => 0x09,
        "enter" => 0x0D,
        "escape" => 0x1B,
        "backspace" => 0x08,
        "insert" => 0x2D,
        "delete" => 0x2E,
        "home" => 0x24,
        "end" => 0x23,
        "pageup" => 0x21,
        "pagedown" => 0x22,
        _ => return None,
    })
}

/// A short label for a key hint ("A", "5", "F3", "Spc", "-").
pub fn short_key_label(vk: u8) -> String {
    match vk {
        0x41..=0x5A | 0x30..=0x39 => (vk as char).to_string(),
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        0x60..=0x69 => format!("N{}", vk - 0x60),
        0x20 => "Spc".into(),
        0x25 => "←".into(),
        0x26 => "↑".into(),
        0x27 => "→".into(),
        0x28 => "↓".into(),
        0x09 => "Tab".into(),
        0xC0 => "`".into(),
        186 => ";".into(),
        187 => "=".into(),
        188 => ",".into(),
        189 => "-".into(),
        190 => ".".into(),
        191 => "/".into(),
        219 => "[".into(),
        220 => "\\".into(),
        221 => "]".into(),
        222 => "'".into(),
        other => super::vk_name(other),
    }
}

/// The bubbles for the replay overlay as flat arrays (JSON).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct PressArrays {
    pub t: Vec<f64>,
    pub show: Vec<f64>,
    pub x: Vec<f32>,
    pub y: Vec<f32>,
    pub action: Vec<u16>,
    /// 0 normal, 1 confirmed, 2 unconfirmed.
    pub state: Vec<u8>,
    /// Index into `hints` + 1 (0 = no hint).
    pub hint: Vec<u16>,
    pub hints: Vec<String>,
}

pub fn to_arrays(p: &[ActionPress]) -> PressArrays {
    let mut a = PressArrays::default();
    for q in p {
        a.t.push(q.t);
        a.show.push(q.show);
        a.x.push(q.x);
        a.y.push(q.y);
        a.action.push(q.action as u16);
        a.state.push(match q.state {
            PressState::Normal => 0,
            PressState::Confirmed => 1,
            PressState::Unconfirmed => 2,
        });
        a.hint.push(match &q.hint {
            None => 0,
            Some(h) => match a.hints.iter().position(|x| x == h) {
                Some(i) => i as u16 + 1,
                None => {
                    a.hints.push(h.clone());
                    a.hints.len() as u16
                }
            },
        });
    }
    a
}

/// The action keys of a recording: the binds saved with it, or (recorded before they were
/// saved) the game's defaults. The bool says whether they were saved with the game.
pub fn session_actions(saved: Option<&[ActionKey]>, game_defaults: impl FnOnce() -> Vec<ActionKey>) -> (Vec<ActionKey>, bool) {
    match saved {
        Some(a) if !a.is_empty() => (a.to_vec(), true),
        _ => (game_defaults(), false),
    }
}

#[cfg(test)]
mod tests;
