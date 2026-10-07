//! Accurate ult tracking, layer 1 (live, during the game, cheap):
//! - the ult keybinds come from League's own `Config\input.ini` (normal, quick, with-indicator
//!   and self-cast variants of spell 4, several binds per action); the manual setting overrides,
//! - presses are filtered while R isn't learned (`abilityLevel` 0), while dead (exceptions in
//!   `ult_rules.json`), and inside the cooldown window (Data Dragon's base cooldown for the rank
//!   × 100 / (100 + ability haste), 85 % of it; champions with recasts/charges/resets skip it),
//! - nothing is thrown away: every press becomes a [`KeyMark`] with the reason it was filtered,
//!   so the check against the recording after the game (layer 2, `verify.rs`) decides.
//!
//! No game memory, no injection: only the Live Client Data API, config files and Data Dragon.

use crate::ultkind::{KindRule, LiveEpisodes, LivePress, UltKind};
use cv_core::game::{KeyMark, KeyPress};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

// ---------- keybinds ----------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bind {
    pub key: String,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Bind {
    pub fn plain(key: &str) -> Bind {
        Bind { key: key.into(), ctrl: false, shift: false, alt: false }
    }
    pub fn matches(&self, k: &KeyPress) -> bool {
        self.key.eq_ignore_ascii_case(&k.key) && self.ctrl == k.ctrl && self.shift == k.shift && self.alt == k.alt
    }
    pub fn label(&self) -> String {
        let mut s = String::new();
        for (on, m) in [(self.ctrl, "Ctrl+"), (self.alt, "Alt+"), (self.shift, "Shift+")] {
            if on {
                s.push_str(m);
            }
        }
        s + &self.key
    }
}

pub fn press_label(k: &KeyPress) -> String {
    Bind { key: k.key.clone(), ctrl: k.ctrl, shift: k.shift, alt: k.alt }.label()
}

/// The ult binds of a League `input.ini`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct UltBinds {
    /// Every bind that casts spell 4 (R).
    pub cast: Vec<Bind>,
    /// Binds that level the ult up instead (Ctrl+R by default).
    pub level_up: Vec<Bind>,
    /// Mouse buttons bound to the ult (can't be seen as key presses; the recording still
    /// finds those casts).
    pub mouse: Vec<String>,
}

/// Actions that cast spell 4, with League's defaults when the file doesn't set them.
const CAST_ACTIONS: [(&str, &str); 7] = [
    ("evtCastSpell4", "[r]"),
    ("evtSmartCastSpell4", "[Shift][r]"),
    ("evtSmartCastWithIndicatorSpell4", ""),
    ("evtSelfCastSpell4", "[Alt][r]"),
    ("evtSmartPlusSelfCastSpell4", ""),
    ("evtSmartPlusSelfCastWithIndicatorSpell4", ""),
    ("evtNormalCastSpell4", ""),
];

/// Maps League's key names (`r`, `Space`, `Num5`, `-`) to the names of [`KeyPress`].
pub(crate) fn key_name(k: &str) -> Option<String> {
    let k = k.trim();
    if k.is_empty() || k.eq_ignore_ascii_case("<Unbound>") {
        return None;
    }
    if k.len() == 1 {
        let c = k.chars().next().unwrap();
        if c.is_ascii_alphanumeric() {
            return Some(c.to_ascii_uppercase().to_string());
        }
        // Virtual-key codes the input thread reports as "Key<n>".
        let vk = match c {
            ';' => 186,
            '=' => 187,
            ',' => 188,
            '-' => 189,
            '.' => 190,
            '/' => 191,
            '`' => return Some("`".into()),
            '[' => 219,
            '\\' => 220,
            ']' => 221,
            '\'' => 222,
            _ => return None,
        };
        return Some(format!("Key{vk}"));
    }
    let lower = k.to_ascii_lowercase();
    Some(match lower.as_str() {
        "space" => "Space".into(),
        "tab" => "Tab".into(),
        "enter" | "return" => "Enter".into(),
        "esc" | "escape" => "Escape".into(),
        "backspace" => "Backspace".into(),
        "insert" | "ins" => "Insert".into(),
        "delete" | "del" => "Delete".into(),
        "home" => "Home".into(),
        "end" => "End".into(),
        "pageup" | "pgup" => "Pageup".into(),
        "pagedown" | "pgdn" => "Pagedown".into(),
        _ if lower.starts_with('f') && lower[1..].parse::<u8>().is_ok() => format!("F{}", &lower[1..]),
        _ if lower.starts_with("num") && lower[3..].parse::<u8>().is_ok() => format!("Num{}", &lower[3..]),
        _ => return None,
    })
}

/// League's mouse bind names: "Button 1" = left, 2 = right, 3 = middle, 4/5 = side buttons.
pub fn mouse_button_number(name: &str) -> Option<u8> {
    let n: u8 = name.trim().to_ascii_lowercase().strip_prefix("button")?.trim().parse().ok()?;
    (1..=5).contains(&n).then_some(n)
}

/// Parses one bind value: `[Alt][r]`, `[r],[Shift][r]`, `null`, `[<Unbound>]`.
/// Returns key binds and mouse-button binds.
fn parse_value(v: &str) -> (Vec<Bind>, Vec<String>) {
    let mut out = Vec::new();
    let mut mouse = Vec::new();
    let v = v.trim();
    if v.is_empty() || v == "null" {
        return (out, mouse);
    }
    // Binds are separated by commas outside brackets.
    let mut parts = Vec::new();
    let mut depth = 0;
    let mut cur = String::new();
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
            ',' if depth == 0 => parts.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    parts.push(cur);
    for p in parts {
        let tokens: Vec<String> = p.split('[').filter_map(|t| t.split(']').next()).map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect();
        if tokens.is_empty() {
            continue;
        }
        let mut b = Bind { key: String::new(), ctrl: false, shift: false, alt: false };
        let mut ok = true;
        for t in &tokens {
            match t.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => b.ctrl = true,
                "shift" => b.shift = true,
                "alt" => b.alt = true,
                "<unbound>" => ok = false,
                l if l.starts_with("button") => {
                    mouse.push(t.clone());
                    ok = false;
                }
                _ => match key_name(t) {
                    Some(k) => b.key = k,
                    None => ok = false,
                },
            }
        }
        if ok && !b.key.is_empty() {
            out.push(b);
        }
    }
    (out, mouse)
}

/// Reads the ult binds from the text of `Config\input.ini` (or `PersistedSettings.json`'s
/// Input.ini section, flattened to `name=value` lines).
pub fn parse_input_ini(text: &str) -> UltBinds {
    let mut values: HashMap<String, String> = HashMap::new();
    for line in text.lines() {
        if let Some((k, v)) = line.split_once('=') {
            values.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    let mut binds = UltBinds::default();
    for (action, default) in CAST_ACTIONS {
        let v = values.get(action).map(|s| s.as_str()).unwrap_or(default);
        let (keys, mouse) = parse_value(v);
        for k in keys {
            if !binds.cast.contains(&k) {
                binds.cast.push(k);
            }
        }
        binds.mouse.extend(mouse);
    }
    let (lv, _) = parse_value(values.get("evtLevelSpell4").map(|s| s.as_str()).unwrap_or("[Ctrl][r]"));
    binds.level_up = lv;
    binds
}

/// The binds League uses when there's no `input.ini`.
pub fn default_binds() -> UltBinds {
    parse_input_ini("")
}

/// The League install folder's `Config\input.ini`; `PersistedSettings.json` is League's
/// newer copy of the same settings (preferred when it has the key binds).
pub fn read_binds(install_dir: &Path) -> Option<UltBinds> {
    read_input_settings(install_dir).map(|t| parse_input_ini(&t))
}

/// League's key bind settings as `name=value` lines: `PersistedSettings.json`'s Input.ini part
/// when it has the binds (it's League's authoritative copy), else `Config\input.ini`. The ult
/// tracking and the replay's ability bubbles both read them through this.
pub fn read_input_settings(install_dir: &Path) -> Option<String> {
    let cfg = install_dir.join("Config");
    if let Ok(t) = std::fs::read_to_string(cfg.join("PersistedSettings.json")) {
        if let Some(flat) = persisted_section(&t, "Input.ini") {
            if !parse_input_ini(&flat).cast.is_empty() {
                return Some(flat);
            }
        }
    }
    std::fs::read_to_string(cfg.join("input.ini")).ok()
}

/// Flattens one file of `PersistedSettings.json` (e.g. "Input.ini", "Game.cfg") into
/// `name=value` lines.
pub fn persisted_section(json: &str, file: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(json.trim_start_matches('\u{feff}')).ok()?;
    let f = v["files"].as_array()?.iter().find(|f| f["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(file)))?;
    let mut out = String::new();
    for sec in f["sections"].as_array()? {
        for s in sec["settings"].as_array().into_iter().flatten() {
            if let (Some(n), Some(val)) = (s["name"].as_str(), s["value"].as_str()) {
                out.push_str(&format!("{n}={val}\n"));
            }
        }
    }
    Some(out)
}

/// Game settings that matter for finding the HUD in the recording and the patch.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct GameCfg {
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// HUD scale slider, 0..1.
    pub hud_scale: Option<f64>,
    /// e.g. "16.19" (from CfgVersion "16.19.823.722").
    pub patch: Option<String>,
}

pub fn parse_game_cfg(text: &str) -> GameCfg {
    let mut c = GameCfg::default();
    let mut section = String::new();
    for line in text.lines() {
        let l = line.trim();
        if l.starts_with('[') && l.ends_with(']') {
            section = l[1..l.len() - 1].to_string();
            continue;
        }
        let Some((k, v)) = l.split_once('=') else { continue };
        let (k, v) = (k.trim(), v.trim());
        match (section.as_str(), k) {
            ("General", "Width") | ("", "Width") => c.width = v.parse().ok(),
            ("General", "Height") | ("", "Height") => c.height = v.parse().ok(),
            ("HUD", "GlobalScale") | ("", "GlobalScale") => c.hud_scale = v.parse().ok(),
            (_, "CfgVersion") => c.patch = patch_of(v),
            _ => {}
        }
    }
    c
}

/// "16.19.823.722" → "16.19".
pub fn patch_of(version: &str) -> Option<String> {
    let mut it = version.split('.');
    let (a, b) = (it.next()?, it.next()?);
    (a.parse::<u32>().is_ok() && b.parse::<u32>().is_ok()).then(|| format!("{a}.{b}"))
}

pub fn read_game_cfg(install_dir: &Path) -> GameCfg {
    let cfg = install_dir.join("Config");
    let mut c = std::fs::read_to_string(cfg.join("game.cfg")).map(|t| parse_game_cfg(&t)).unwrap_or_default();
    if let Ok(t) = std::fs::read_to_string(cfg.join("PersistedSettings.json")) {
        if let Some(flat) = persisted_section(&t, "Game.cfg") {
            let p = parse_game_cfg(&flat);
            c.width = p.width.or(c.width);
            c.height = p.height.or(c.height);
            c.hud_scale = p.hud_scale.or(c.hud_scale);
            c.patch = p.patch.or(c.patch);
        }
    }
    c
}

// ---------- rules ----------

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct UltRules {
    #[serde(default)]
    pub version: u32,
    #[serde(default = "default_delay")]
    pub default_cooldown_delay: f64,
    #[serde(default)]
    pub cooldown_filter_game_modes: Vec<String>,
    #[serde(default)]
    pub skip_cooldown_filter: HashMap<String, String>,
    #[serde(default)]
    pub usable_while_dead: HashMap<String, String>,
    #[serde(default)]
    pub cooldown_delay: HashMap<String, f64>,
    /// v1.6: what one ult is per champion (see `ultkind`). Not listed = normal.
    #[serde(default)]
    pub kinds: HashMap<String, KindRule>,
}

fn default_delay() -> f64 {
    1.2
}

pub const BUILTIN_RULES: &str = include_str!("../ult_rules.json");

impl UltRules {
    pub fn parse(text: &str) -> anyhow::Result<UltRules> {
        Ok(serde_json::from_str(text.trim_start_matches('\u{feff}'))?)
    }
    pub fn builtin() -> UltRules {
        UltRules::parse(BUILTIN_RULES).expect("ult_rules.json")
    }
    /// The built-in rules, or `ult_rules.json` in the app's data folder if there is a valid one
    /// (so the list can be updated without a new version).
    pub fn load(data_dir: Option<&Path>) -> UltRules {
        if let Some(d) = data_dir {
            if let Ok(t) = std::fs::read_to_string(d.join("ult_rules.json")) {
                match UltRules::parse(&t) {
                    Ok(r) => return r,
                    Err(e) => log::warn!("ult_rules.json in {}: {e}; using the built-in rules", d.display()),
                }
            }
        }
        UltRules::builtin()
    }
    fn has(map: &HashMap<String, impl Sized>, champ: &str) -> bool {
        let norm = |s: &str| s.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
        let c = norm(champ);
        map.keys().any(|k| norm(k) == c)
    }
    pub fn skips_cooldown(&self, champ: &str) -> bool {
        Self::has(&self.skip_cooldown_filter, champ) || self.kind_rule(champ).is_some_and(|k| k.kind != UltKind::Normal)
    }
    /// The champion's entry in `kinds` (None = not listed).
    pub fn kind_rule(&self, champ: &str) -> Option<&KindRule> {
        let norm = |s: &str| s.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
        let c = norm(champ);
        self.kinds.iter().find(|(k, _)| norm(k) == c).map(|(_, v)| v)
    }
    /// The rule to use: the rules file, else `guess` (Data Dragon's R text, see
    /// `ultkind::classify_text`), else normal.
    pub fn kind_for(&self, champ: &str, guess: Option<UltKind>) -> KindRule {
        match self.kind_rule(champ) {
            Some(k) => k.clone(),
            None => match guess {
                Some(g) if g != UltKind::Normal => KindRule::guessed(g),
                _ => KindRule::normal(),
            },
        }
    }
    pub fn usable_while_dead(&self, champ: &str) -> bool {
        Self::has(&self.usable_while_dead, champ)
    }
    /// Longest time from the press until the bar shows the cooldown.
    pub fn delay(&self, champ: &str) -> f64 {
        let norm = |s: &str| s.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
        let c = norm(champ);
        self.cooldown_delay.iter().find(|(k, _)| norm(k) == c).map(|(_, v)| *v).unwrap_or(self.default_cooldown_delay)
    }
    pub fn cooldown_mode(&self, game_mode: &str) -> bool {
        self.cooldown_filter_game_modes.iter().any(|m| m.eq_ignore_ascii_case(game_mode))
    }
}

// ---------- cooldown ----------

/// Cooldown in seconds for `rank` (1-based) with `haste` ability haste.
pub fn cooldown(base: &[f64], rank: u32, haste: f64) -> Option<f64> {
    if rank == 0 || base.is_empty() {
        return None;
    }
    let b = base[(rank as usize - 1).min(base.len() - 1)];
    Some(b * 100.0 / (100.0 + haste.max(0.0)))
}

/// Part of the cooldown during which a press is surely not a cast (the rest covers haste the
/// API doesn't report, e.g. ultimate haste, and refunds).
pub const COOLDOWN_WINDOW: f64 = 0.85;

// ---------- the live tracker ----------

/// Live state for one match.
#[derive(Debug, Clone)]
pub struct UltTracker {
    pub binds: UltBinds,
    /// "Ult key" setting (overrides the binds when set).
    pub manual: Option<Bind>,
    pub rules: UltRules,
    /// Data Dragon id, e.g. "MonkeyKing".
    pub champion: Option<String>,
    pub game_mode: Option<String>,
    /// R rank from `/activeplayer` (None until known: no level filter).
    pub r_level: Option<u32>,
    pub haste: f64,
    /// Base cooldown per rank (Data Dragon).
    pub base_cd: Option<Vec<f64>>,
    pub dead: bool,
    pub chat_open: bool,
    /// Game time and window of the last press that counted as a cast.
    last_cast: Option<(f64, f64)>,
    last_press: Option<f64>,
    marks: Vec<KeyMark>,
    /// The kind Data Dragon's R text suggests (champions not in the rules file).
    pub guess: Option<UltKind>,
    /// Live ult episodes (command / multi_cast).
    episodes: LiveEpisodes,
    /// The R ability's id/name from `/activeplayer`: the first one seen, and the current one.
    r_base: Option<String>,
    r_now: Option<String>,
}

impl UltTracker {
    pub fn new(binds: UltBinds, manual: Option<Bind>, rules: UltRules) -> Self {
        Self {
            binds,
            manual,
            rules,
            champion: None,
            game_mode: None,
            r_level: None,
            haste: 0.0,
            base_cd: None,
            dead: false,
            chat_open: false,
            last_cast: None,
            last_press: None,
            marks: Vec::new(),
            guess: None,
            episodes: LiveEpisodes::default(),
            r_base: None,
            r_now: None,
        }
    }

    fn is_cast(&self, k: &KeyPress) -> bool {
        match &self.manual {
            Some(m) => m.key.eq_ignore_ascii_case(&k.key) && !k.ctrl,
            None => self.binds.cast.iter().any(|b| b.matches(k)),
        }
    }

    /// Current cooldown window (seconds) or None when the cooldown filter doesn't apply.
    pub fn window(&self) -> Option<f64> {
        let champ = self.champion.as_deref()?;
        if self.rules.skips_cooldown(champ) || !self.game_mode.as_deref().is_some_and(|m| self.rules.cooldown_mode(m)) {
            return None;
        }
        cooldown(self.base_cd.as_deref()?, self.r_level?, self.haste).map(|c| c * COOLDOWN_WINDOW)
    }

    /// This game's ult kind rule.
    pub fn kind(&self) -> KindRule {
        self.rules.kind_for(self.champion.as_deref().unwrap_or(""), self.guess)
    }

    /// A key press during the game. Returns true if it counts as an ult cast (a timeline
    /// event); every ult press is also kept as a mark (see [`Self::take_marks`]).
    pub fn press(&mut self, k: &KeyPress, game_time: f64) -> bool {
        self.press_live(k, game_time) != LivePress::None
    }

    /// Like [`Self::press`], and says what the press is: a new ult, a recast of the current one
    /// (command / multi_cast champions, within the ult's longest duration or while
    /// `/activeplayer` shows R in its recast state), or a form swap.
    pub fn press_live(&mut self, k: &KeyPress, game_time: f64) -> LivePress {
        if !self.ult_key(k, game_time) {
            return LivePress::None;
        }
        let Some(m) = self.marks.last_mut() else { return LivePress::None };
        if !m.accepted {
            return LivePress::None;
        }
        let rule = self.kind();
        let what = self.episodes.press(rule.kind, rule.cap(self.r_level), game_time);
        let in_episode = rule.kind.has_episodes() && self.episodes.started.is_some_and(|s| s < game_time);
        let m = self.marks.last_mut().unwrap();
        if what == LivePress::Recast || (what == LivePress::None && in_episode) {
            // A later press of the same ult (a mashed one gets no marker of its own).
            m.reason = Some("recast".into());
        }
        what
    }

    /// Gates + mark for one key press; true if it was an ult key press (a mark was added).
    fn ult_key(&mut self, k: &KeyPress, game_time: f64) -> bool {
        match k.key.as_str() {
            // Rough chat detection: Enter opens/sends chat, Escape closes it.
            "Enter" if !k.ctrl && !k.alt => {
                self.chat_open = !self.chat_open;
                return false;
            }
            "Escape" => {
                self.chat_open = false;
                return false;
            }
            _ => {}
        }
        if self.binds.level_up.iter().any(|b| b.matches(k)) && self.manual.is_none() {
            return false;
        }
        if !self.is_cast(k) {
            return false;
        }
        // Contact bounce (auto-repeat is already ignored by the input thread).
        if self.last_press.is_some_and(|t| (game_time - t).abs() < 0.08) {
            return false;
        }
        self.last_press = Some(game_time);
        let champ = self.champion.clone().unwrap_or_default();
        let reason = if self.chat_open {
            Some("chat".to_string())
        } else if self.r_level == Some(0) {
            Some("not_learned".into())
        } else if self.dead && !self.rules.usable_while_dead(&champ) {
            Some("dead".into())
        } else if let (Some((t, w)), Some(_)) = (self.last_cast, self.window()) {
            (game_time - t < w).then(|| format!("cooldown ({:.0} s left)", w / COOLDOWN_WINDOW - (game_time - t)))
        } else {
            None
        };
        let accepted = reason.is_none();
        if accepted {
            self.last_cast = Some((game_time, self.window().unwrap_or(0.0)));
        }
        self.marks.push(KeyMark { game_time, action: "ult".into(), key: press_label(k), accepted, reason });
        true
    }

    /// The R ability's id/name from `/activeplayer` (every poll). Many recast ults swap the R
    /// spell while the recast is available; when this one does, it keeps the live episode open
    /// while the name differs from its normal one and ends it when it's back. Every change is
    /// kept as a mark (action "r_state") for later analysis.
    pub fn update_r_state(&mut self, r: Option<String>, game_time: f64) {
        let Some(r) = r.filter(|s| !s.is_empty()) else { return };
        if self.r_base.is_none() {
            self.r_base = Some(r.clone());
        }
        if self.r_now.as_deref() != Some(r.as_str()) {
            if self.r_now.is_some() {
                log::info!("ult: R ability changed to {r} at {game_time:.1}");
            }
            self.marks.push(KeyMark { game_time, action: "r_state".into(), key: r.clone(), accepted: false, reason: None });
            self.r_now = Some(r.clone());
        }
        if !self.kind().kind.has_episodes() {
            return;
        }
        if self.r_base.as_deref() == Some(r.as_str()) {
            self.episodes.ended(game_time);
        } else {
            self.episodes.extend(game_time);
        }
    }

    /// Death: a multi_cast ult's recasts end with it.
    pub fn set_dead(&mut self, dead: bool) {
        if dead && !self.dead && self.kind().kind == UltKind::MultiCast {
            self.episodes.reset();
        }
        self.dead = dead;
    }

    /// Mouse-button presses (from the input recording, after the game) on a button bound to the
    /// ult: kept as ult presses for the check against the recording, so a mouse-bound ult cast
    /// matches its press instead of showing as "cast without a logged press". No live gates are
    /// known for them (reason "mouse"); the recording decides.
    pub fn mouse_marks(&self, presses: &[(f64, u8)]) -> Vec<KeyMark> {
        if self.manual.is_some() {
            return Vec::new();
        }
        let bound: Vec<u8> = self.binds.mouse.iter().filter_map(|b| mouse_button_number(b)).collect();
        presses
            .iter()
            .filter(|(_, b)| bound.contains(b))
            .map(|&(t, b)| KeyMark { game_time: t, action: "ult".into(), key: format!("Mouse {b}"), accepted: false, reason: Some("mouse".into()) })
            .collect()
    }

    pub fn take_marks(&mut self) -> Vec<KeyMark> {
        std::mem::take(&mut self.marks)
    }

    /// Updates from `/activeplayer`: R rank and ability haste.
    pub fn update_active(&mut self, r_level: Option<u32>, haste: Option<f64>) {
        if let Some(l) = r_level {
            self.r_level = Some(l);
        }
        if let Some(h) = haste {
            self.haste = h;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The owner's real input.ini (League 16.19), trimmed to the lines that matter.
    const OWNER_INI: &str = "[GameEvents]
evtSmartPlusSelfCastWithIndicatorSpell4=null
evtSmartPlusSelfCastSpell4=[<Unbound>]
evtSmartCastWithIndicatorSpell4=[Alt][r]
evtSmartCastSpell4=[r]
evtSelfCastSpell4=[Shift][r]
evtNormalCastSpell4=null
evtLevelSpell4=[Ctrl][r]
evtCastSpell4=[<Unbound>]
evtPlayerMoveClick=[Button 2],[Shift][Button 2]
";

    fn kp(k: &str, ctrl: bool, shift: bool, alt: bool) -> KeyPress {
        KeyPress { key: k.into(), ctrl, shift, alt }
    }

    #[test]
    fn owner_input_ini() {
        let b = parse_input_ini(OWNER_INI);
        let labels: Vec<String> = b.cast.iter().map(|b| b.label()).collect();
        assert_eq!(labels, vec!["R", "Alt+R", "Shift+R"]);
        assert_eq!(b.level_up, vec![Bind { key: "R".into(), ctrl: true, shift: false, alt: false }]);
    }

    #[test]
    fn mouse_bound_ult_presses_from_the_input_recording() {
        let binds = parse_input_ini("evtCastSpell4=[r],[Button 5]\n");
        let t = UltTracker::new(binds.clone(), None, UltRules::builtin());
        let marks = t.mouse_marks(&[(10.0, 5), (11.0, 1), (12.0, 4), (13.5, 5)]);
        assert_eq!(marks.iter().map(|m| (m.game_time, m.key.as_str())).collect::<Vec<_>>(), vec![(10.0, "Mouse 5"), (13.5, "Mouse 5")]);
        assert!(marks.iter().all(|m| m.action == "ult" && !m.accepted && m.reason.as_deref() == Some("mouse")));
        // A manual ult key replaces League's binds: no mouse marks.
        let manual = UltTracker::new(binds, Some(Bind::plain("T")), UltRules::builtin());
        assert!(manual.mouse_marks(&[(10.0, 5)]).is_empty());
        assert_eq!(mouse_button_number("Button 4"), Some(4));
        assert_eq!(mouse_button_number("button 9"), None);
    }

    #[test]
    fn several_binds_mouse_and_defaults() {
        let b = parse_input_ini("evtCastSpell4=[f],[Button 5]\nevtSmartCastSpell4=[<Unbound>]\nevtSelfCastSpell4=[Ctrl][Shift][-]\n");
        let labels: Vec<String> = b.cast.iter().map(|b| b.label()).collect();
        assert_eq!(labels, vec!["F", "Ctrl+Shift+Key189"]);
        assert_eq!(b.mouse, vec!["Button 5"]);
        // No file: League's defaults.
        let d: Vec<String> = default_binds().cast.iter().map(|b| b.label()).collect();
        assert_eq!(d, vec!["R", "Shift+R", "Alt+R"]);
        assert_eq!(parse_value("[Space]").0, vec![Bind::plain("Space")]);
        assert_eq!(parse_value("[F5]").0, vec![Bind::plain("F5")]);
    }

    #[test]
    fn persisted_settings_and_game_cfg() {
        let json = r#"{"files":[{"name":"Input.ini","sections":[{"name":"GameEvents","settings":[{"name":"evtSmartCastSpell4","value":"[t]"}]}]},
            {"name":"Game.cfg","sections":[{"name":"HUD","settings":[{"name":"GlobalScale","value":"0.2500"}]},{"name":"General","settings":[{"name":"Width","value":"2560"},{"name":"Height","value":"1440"},{"name":"CfgVersion","value":"16.19.823.722"}]}]}]}"#;
        let b = parse_input_ini(&persisted_section(json, "Input.ini").unwrap());
        assert!(b.cast.contains(&Bind::plain("T")));
        let c = parse_game_cfg(&persisted_section(json, "Game.cfg").unwrap());
        assert_eq!(c, GameCfg { width: Some(2560), height: Some(1440), hud_scale: Some(0.25), patch: Some("16.19".into()) });
        let c = parse_game_cfg("[General]\nWidth=3840\nHeight=2160\nCfgVersion=16.19.823.722\n[HUD]\nGlobalScale=0.0000\n");
        assert_eq!(c.hud_scale, Some(0.0));
        assert_eq!(c.height, Some(2160));
    }

    #[test]
    fn cooldown_math() {
        // Base 120/100/80, rank 2, 20 haste: 100 * 100/120 = 83.3 s.
        let c = cooldown(&[120.0, 100.0, 80.0], 2, 20.0).unwrap();
        assert!((c - 83.333).abs() < 0.01);
        assert_eq!(cooldown(&[120.0], 0, 0.0), None);
        assert_eq!(cooldown(&[120.0, 100.0, 80.0], 5, 0.0), Some(80.0), "rank past the list uses the last");
    }

    #[test]
    fn rules_file() {
        let r = UltRules::builtin();
        assert!(r.skips_cooldown("Ahri") && r.skips_cooldown("akali") && r.skips_cooldown("TwistedFate"));
        assert!(!r.skips_cooldown("Caitlyn"));
        assert!(r.usable_while_dead("Karthus"));
        assert!(r.cooldown_mode("CLASSIC") && !r.cooldown_mode("URF") && !r.cooldown_mode("CHERRY"));
        assert_eq!(r.delay("Caitlyn"), 1.5);
        assert_eq!(r.delay("Twitch"), r.default_cooldown_delay);
        assert!(UltRules::parse("{ not json").is_err());
    }

    fn tracker(champ: &str) -> UltTracker {
        let mut t = UltTracker::new(parse_input_ini(OWNER_INI), None, UltRules::builtin());
        t.champion = Some(champ.into());
        t.game_mode = Some("CLASSIC".into());
        t.base_cd = Some(vec![90.0, 75.0, 60.0]);
        t
    }

    #[test]
    fn live_filtering() {
        let mut t = tracker("Caitlyn");
        let r = kp("R", false, false, false);
        t.update_active(Some(0), Some(0.0));
        assert!(!t.press(&r, 100.0), "not learned");
        t.update_active(Some(1), Some(0.0));
        assert!(t.press(&r, 400.0), "cast");
        assert!(!t.press(&r, 401.0), "on cooldown");
        assert!(!t.press(&kp("R", true, false, false), 402.0), "Ctrl+R levels up: not even a mark");
        assert!(t.press(&kp("R", false, true, false), 400.0 + 90.0 * 0.86), "Shift+R self-cast, after the window");
        t.dead = true;
        assert!(!t.press(&r, 700.0), "dead");
        t.dead = false;
        t.press(&kp("Enter", false, false, false), 701.0);
        assert!(!t.press(&r, 702.0), "typing in chat");
        t.press(&kp("Enter", false, false, false), 703.0);
        assert!(t.press(&kp("R", false, false, true), 704.0), "Alt+R with indicator");
        let marks = t.take_marks();
        let reasons: Vec<Option<&str>> = marks.iter().map(|m| m.reason.as_deref().map(|r| r.split(' ').next().unwrap())).collect();
        assert_eq!(reasons, vec![Some("not_learned"), None, Some("cooldown"), None, Some("dead"), Some("chat"), None]);
        assert!(t.take_marks().is_empty());
    }

    #[test]
    fn recasts_and_form_swaps_live() {
        use crate::ultkind::LivePress;
        let r = kp("R", false, false, false);
        // Annie: Tibbers, then commands within his 45 s; after that a new ult.
        let mut t = tracker("Annie");
        t.update_active(Some(1), None);
        assert_eq!(t.press_live(&r, 400.0), LivePress::Used);
        assert_eq!(t.press_live(&r, 402.0), LivePress::Recast);
        assert_eq!(t.press_live(&r, 430.0), LivePress::Recast);
        assert_eq!(t.press_live(&r, 520.0), LivePress::Used);
        // Chat and death still filter (no marker, kept as a mark).
        t.press(&kp("Enter", false, false, false), 521.0);
        assert_eq!(t.press_live(&r, 522.0), LivePress::None);
        let marks = t.take_marks();
        assert_eq!(marks.iter().filter(|m| m.reason.as_deref() == Some("recast")).count(), 2);
        assert!(marks.iter().filter(|m| m.reason.as_deref() == Some("recast")).all(|m| m.accepted));
        // Jayce: every press a form swap.
        let mut t = tracker("Jayce");
        t.update_active(Some(1), None);
        assert_eq!(t.press_live(&r, 10.0), LivePress::FormSwap);
        assert_eq!(t.press_live(&r, 17.0), LivePress::FormSwap);
        // Caitlyn: unchanged (one press, one ult, cooldown filter).
        let mut t = tracker("Caitlyn");
        t.update_active(Some(1), None);
        assert_eq!(t.press_live(&r, 400.0), LivePress::Used);
        assert_eq!(t.press_live(&r, 401.0), LivePress::None);
    }

    #[test]
    fn r_state_from_the_api_ends_or_extends_the_episode() {
        use crate::ultkind::LivePress;
        let r = kp("R", false, false, false);
        let mut t = tracker("Zed");
        t.update_active(Some(1), None);
        t.update_r_state(Some("ZedR|Death Mark".into()), 399.0);
        assert_eq!(t.press_live(&r, 400.0), LivePress::Used);
        t.update_r_state(Some("ZedR2|Death Mark".into()), 400.5);
        assert_eq!(t.press_live(&r, 402.0), LivePress::Recast);
        // Back to the normal spell: the ult is over, the next press is a new one.
        t.update_r_state(Some("ZedR|Death Mark".into()), 404.0);
        assert_eq!(t.press_live(&r, 404.5), LivePress::Used);
        let states: Vec<String> = t.take_marks().into_iter().filter(|m| m.action == "r_state").map(|m| m.key).collect();
        assert_eq!(states, vec!["ZedR|Death Mark", "ZedR2|Death Mark", "ZedR|Death Mark"], "every change kept for the owner test analysis");
        // Dying ends a multi-cast ult.
        let mut t = tracker("Zed");
        t.update_active(Some(1), None);
        assert_eq!(t.press_live(&r, 400.0), LivePress::Used);
        t.set_dead(true);
        t.set_dead(false);
        assert_eq!(t.press_live(&r, 401.0), LivePress::Used);
    }

    #[test]
    fn exceptions_and_overrides() {
        // Recasts: no cooldown filter.
        let mut t = tracker("Ahri");
        t.update_active(Some(1), None);
        assert!(t.press(&kp("R", false, false, false), 400.0));
        assert!(t.press(&kp("R", false, false, false), 401.0));
        // Karthus casts while dead.
        let mut t = tracker("Karthus");
        t.update_active(Some(1), None);
        t.dead = true;
        assert!(t.press(&kp("R", false, false, false), 400.0));
        // URF: no cooldown math.
        let mut t = tracker("Caitlyn");
        t.game_mode = Some("URF".into());
        t.update_active(Some(1), None);
        assert!(t.press(&kp("R", false, false, false), 400.0));
        assert!(t.press(&kp("R", false, false, false), 405.0));
        // Manual key overrides the binds.
        let mut t = UltTracker::new(parse_input_ini(OWNER_INI), Some(Bind::plain("G")), UltRules::builtin());
        assert!(!t.press(&kp("R", false, false, false), 10.0));
        assert!(t.press(&kp("G", false, false, false), 11.0));
        // Unknown state (no API data yet): nothing filtered.
        let mut t = UltTracker::new(default_binds(), None, UltRules::builtin());
        assert!(t.press(&kp("R", false, false, false), 10.0));
    }
}
