//! What one ult *is* per champion (v1.6): several R presses can belong to a single ult.
//!
//! Kinds (data-driven, `ult_rules.json` → `kinds`):
//! - `command`: the first R summons something, later presses command it until it's gone
//!   (Annie's Tibbers, Ivern's Daisy, Shaco's clone, Yorick's Maiden, Viktor's storm).
//! - `multi_cast`: one ult with several activations, an early end or a second part (Ahri, Zed...).
//! - `transform`: every press is a real cast with its own short cooldown (Jayce, Nidalee, Elise,
//!   Udyr): shown as "Form swap", not "Ult used".
//! - `charges_or_reset`: every press is a real cast (ammo, charges, resets: Corki, Kassadin...).
//! - `normal`: everything else (one press = one ult, the v1.3 logic).
//!
//! An "ult episode" (command / multi_cast) starts at the first activation and ends when the ult
//! ends (its cooldown appears, the R icon returns to its ready look, its longest duration passes,
//! or — multi_cast — the champion dies). Within it, only the first activation is "Ult used";
//! later presses are "Ult recast".
//!
//! Also here: the keyword scan of Data Dragon's R texts ([`classify_text`]) used by the developer
//! tool `cargo run -p cv-game-league --example ultscan` (re-run each patch) and, at runtime, for a
//! champion the rules file doesn't know yet (looks like a recast → `multi_cast`).

use crate::verify::{match_up, Outcome};
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, serde::Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum UltKind {
    #[default]
    Normal,
    Command,
    MultiCast,
    Transform,
    ChargesOrReset,
}

impl UltKind {
    pub fn as_str(self) -> &'static str {
        match self {
            UltKind::Normal => "normal",
            UltKind::Command => "command",
            UltKind::MultiCast => "multi_cast",
            UltKind::Transform => "transform",
            UltKind::ChargesOrReset => "charges_or_reset",
        }
    }
    pub fn parse(s: &str) -> Option<UltKind> {
        Some(match s {
            "normal" => UltKind::Normal,
            "command" => UltKind::Command,
            "multi_cast" => UltKind::MultiCast,
            "transform" => UltKind::Transform,
            "charges_or_reset" => UltKind::ChargesOrReset,
            _ => return None,
        })
    }
    /// Several presses can belong to one ult (an episode).
    pub fn has_episodes(self) -> bool {
        matches!(self, UltKind::Command | UltKind::MultiCast)
    }
}

/// What ends an ult episode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum End {
    /// The ult's (long) cooldown appears on the R icon.
    Cooldown,
    /// The R icon returns to its normal "ready" look (after showing a recast/command icon).
    Icon,
    /// The longest duration (seconds per rank) has passed.
    Duration,
}

/// One champion's entry in `ult_rules.json` → `kinds`.
#[derive(Debug, Clone, PartialEq, Deserialize, serde::Serialize)]
pub struct KindRule {
    pub kind: UltKind,
    /// What ends an episode (command / multi_cast). The duration cap always applies.
    #[serde(default = "default_ends")]
    pub ends: Vec<End>,
    /// Longest episode, seconds per rank (the last value for higher ranks).
    #[serde(default)]
    pub duration: Vec<f64>,
    /// Press → the first visible change on the R icon at the first activation (default 1.2 s).
    #[serde(default)]
    pub cast_delay: Option<f64>,
    /// Casts while charges are left don't show a cooldown on the icon (Corki, Teemo): a press
    /// with the icon not on cooldown counts as a cast.
    #[serde(default)]
    pub ammo: bool,
    /// Seconds after the ending cooldown that still belong to the ult (Sylas: a stolen ult's own
    /// recasts).
    #[serde(default)]
    pub grace: f64,
    /// Why (and how it was checked).
    #[serde(default)]
    pub note: String,
}

fn default_ends() -> Vec<End> {
    vec![End::Cooldown, End::Icon, End::Duration]
}

/// Duration cap for champions without one (and unknown champions that look like recasts).
pub const DEFAULT_EPISODE: f64 = 15.0;
/// A cooldown at least this long after it appears is the ult's real cooldown (not a recast lockout).
pub const LONG_COOLDOWN: f64 = 3.0;

impl KindRule {
    pub fn normal() -> KindRule {
        KindRule { kind: UltKind::Normal, ends: default_ends(), duration: Vec::new(), cast_delay: None, ammo: false, grace: 0.0, note: String::new() }
    }
    pub fn guessed(kind: UltKind) -> KindRule {
        KindRule { kind, note: "guessed from Data Dragon's R text (not in the rules file)".into(), ..KindRule::normal() }
    }
    /// Longest episode for `rank` (1-based; 0/unknown = the longest of all ranks).
    pub fn cap(&self, rank: Option<u32>) -> f64 {
        if self.duration.is_empty() {
            return DEFAULT_EPISODE;
        }
        match rank {
            Some(r) if r >= 1 => self.duration[(r as usize - 1).min(self.duration.len() - 1)],
            _ => self.duration.iter().copied().fold(0.0, f64::max),
        }
    }
    pub fn cast_delay(&self) -> f64 {
        self.cast_delay.unwrap_or(1.2)
    }
    pub fn ends_on(&self, e: End) -> bool {
        self.ends.contains(&e)
    }
}

// ---------- Data Dragon scan ----------

fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut depth = 0;
    for c in s.chars() {
        match c {
            '<' => depth += 1,
            '>' if depth > 0 => {
                depth -= 1;
                out.push(' ');
            }
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

/// Lower-case words of a text (letters only).
fn words(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_ascii_alphabetic()).filter(|w| !w.is_empty()).map(|w| w.to_ascii_lowercase()).collect()
}

/// The kind an R's Data Dragon texts (description + tooltip) suggest, with the keywords found.
/// A candidate list for the developer to check, and the runtime guess for champions the rules
/// file doesn't know: `None` = nothing recast-like (normal).
pub fn classify_text(description: &str, tooltip: &str, max_ammo: Option<i64>, cooldown: &[f64]) -> (Option<UltKind>, Vec<&'static str>) {
    let text = format!("{} {}", strip_tags(description), strip_tags(tooltip));
    let w = words(&text);
    let has = |k: &str| w.iter().any(|x| x == k);
    let phrase = |p: &str| text.to_ascii_lowercase().contains(p);
    let mut hits: Vec<&'static str> = Vec::new();
    for (k, label) in [
        ("recast", "recast"),
        ("recasts", "recast"),
        ("recasting", "recast"),
        ("reactivate", "reactivate"),
        ("command", "command"),
        ("instruct", "command"),
        ("transforms", "transform"),
        ("transform", "transform"),
        ("charges", "charges"),
        ("ammo", "charges"),
        ("resets", "reset"),
        ("refresh", "reset"),
    ] {
        if has(k) && !hits.contains(&label) {
            hits.push(label);
        }
    }
    for (p, label) in [("second cast", "second cast"), ("cast a second time", "second cast"), ("subsequent use", "charges"), ("subsequent shots", "charges"), ("form", "form")] {
        if phrase(p) && !hits.contains(&label) {
            hits.push(label);
        }
    }
    // Several shots fired one by one with R ("fire 4 super shots"), or another spell cast with R
    // during the form ("can cast Demonflare while transformed").
    let lw = text.to_ascii_lowercase();
    let toks: Vec<&str> = lw.split_whitespace().collect();
    if toks.windows(3).any(|w| w[0] == "fire" && w[1].chars().all(|c| c.is_ascii_digit()) && w[2].starts_with("super")) && !hits.contains(&"several shots") {
        hits.push("several shots");
    }
    if lw.contains("can cast") && lw.contains("while transformed") && !hits.contains(&"cast while transformed") {
        hits.push("cast while transformed");
    }
    let short_cd = !cooldown.is_empty() && cooldown.iter().all(|c| *c <= 10.0);
    let kind = if max_ammo.is_some_and(|a| a > 0) || hits.contains(&"charges") && short_cd {
        Some(UltKind::ChargesOrReset)
    } else if short_cd && hits.contains(&"transform") {
        Some(UltKind::Transform)
    } else if hits.contains(&"command") {
        Some(UltKind::Command)
    } else if hits.iter().any(|h| matches!(*h, "recast" | "reactivate" | "second cast" | "several shots" | "cast while transformed")) {
        Some(UltKind::MultiCast)
    } else {
        None
    };
    (kind, hits)
}

// ---------- episodes (post-game) ----------

/// A cooldown appearing on the R icon (video seconds).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CastSig {
    pub t: f64,
    /// The cooldown is still there [`LONG_COOLDOWN`] seconds later (the real cooldown, not a lockout).
    pub long: bool,
}

/// What the ability bar showed during the game (all video seconds).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Signals {
    pub casts: Vec<CastSig>,
    /// The R icon showed another picture than its own ready look (a recast / command icon).
    pub alt: Vec<(f64, f64)>,
    /// Samples where the icon had its ready look again (sorted).
    pub ready: Vec<f64>,
    /// The player's deaths.
    pub deaths: Vec<f64>,
}

/// One logged R press: time (video s) and whether the live gates let it through as a possible
/// cast (false = typed in chat, dead, not learned...).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PressIn {
    pub t: f64,
    pub ok: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Label {
    /// "Ult used" at `at` (the first activation), with the press that started it.
    Used { at: f64, press: Option<usize> },
    /// A later press of the same ult (command, second part, early end).
    Recast { press: usize, episode: usize },
    /// A transform champion's cast.
    FormSwap { at: f64, press: Option<usize> },
    /// A press without a cast.
    NoCast { press: usize },
}

/// One ult episode: [start, end] in video seconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Episode {
    pub start: f64,
    pub end: f64,
}

/// The episodes of a command / multi_cast champion from the ability bar alone.
pub fn episodes(rule: &KindRule, sig: &Signals) -> Vec<Episode> {
    let mut acts: Vec<f64> = sig.casts.iter().map(|c| c.t).chain(sig.alt.iter().map(|a| a.0)).collect();
    acts.sort_by(|a, b| a.total_cmp(b));
    let cap = rule.cap(None) + 1.0;
    let mut out: Vec<Episode> = Vec::new();
    for a in acts {
        if out.last().is_some_and(|e| a <= e.end + 0.05) {
            continue;
        }
        let mut end = a + cap;
        if rule.ends_on(End::Cooldown) {
            if let Some(c) = sig.casts.iter().find(|c| c.long && c.t > a + 0.5 && c.t < end) {
                end = (c.t + rule.grace).min(end);
            }
        }
        // Icon back to its ready look, after a recast/command icon that started this episode.
        let alt_here = sig.alt.iter().find(|&&(s, e)| s <= a + rule.cast_delay() + 1.0 && e > a);
        if rule.ends_on(End::Icon) {
            if let Some(&(_, ae)) = alt_here {
                if let Some(&r) = sig.ready.iter().find(|&&r| r >= ae - 0.05 && r > a + 0.5 && !sig.alt.iter().any(|&(s, e)| r >= s && r < e)) {
                    end = end.min(r);
                }
            }
        }
        if rule.kind == UltKind::MultiCast {
            if let Some(&d) = sig.deaths.iter().find(|&&d| d > a && d < end) {
                end = d;
            }
        }
        out.push(Episode { start: a, end });
    }
    out
}

/// Labels the R presses of a game from what the ability bar showed. `delay` = the champion's
/// longest press → cooldown time (rules `cooldown_delay`); `ammo_ready(t)` = the icon wasn't on
/// cooldown at `t` (ammo champions only).
pub fn label(rule: &KindRule, delay: f64, sig: &Signals, presses: &[PressIn], ammo_ready: &dyn Fn(f64) -> bool) -> Vec<Label> {
    let casts: Vec<f64> = sig.casts.iter().map(|c| c.t).collect();
    let times: Vec<f64> = presses.iter().map(|p| p.t).collect();
    match rule.kind {
        UltKind::Normal | UltKind::Transform | UltKind::ChargesOrReset => {
            // One cast per press: the v1.3 matching (closest press before the cast).
            let mut out: Vec<Label> = match_up(&casts, &times, delay, false)
                .into_iter()
                .map(|o| match o {
                    Outcome::Used { cast, press } if rule.kind == UltKind::Transform => Label::FormSwap { at: cast, press },
                    Outcome::Used { cast, press } => Label::Used { at: cast, press },
                    Outcome::NoCast { press } => Label::NoCast { press },
                })
                .collect();
            if rule.kind == UltKind::ChargesOrReset && rule.ammo {
                for l in out.iter_mut() {
                    if let Label::NoCast { press } = *l {
                        if presses[press].ok && ammo_ready(presses[press].t) {
                            *l = Label::Used { at: presses[press].t, press: Some(press) };
                        }
                    }
                }
            }
            out
        }
        UltKind::Command | UltKind::MultiCast => {
            let eps = episodes(rule, sig);
            let mut taken = vec![false; presses.len()];
            let mut out = Vec::new();
            for (k, ep) in eps.iter().enumerate() {
                let lo = ep.start - delay.max(rule.cast_delay()) - 0.1;
                let hi = ep.end + 0.1;
                let inside: Vec<usize> = (0..presses.len()).filter(|&i| !taken[i] && times[i] >= lo && times[i] <= hi).collect();
                // The press that started it: the first possible one before the activation (a
                // hair after it for clock jitter).
                let first = inside.iter().copied().find(|&i| presses[i].ok && times[i] <= ep.start + 0.25);
                let at = match first {
                    // The bar may show the first activation late (a recast window before the
                    // cooldown): then the press is the better moment.
                    Some(i) if ep.start - times[i] > rule.cast_delay() + 0.25 => times[i],
                    _ => ep.start,
                };
                out.push(Label::Used { at, press: first });
                for i in inside {
                    taken[i] = true;
                    if Some(i) == first {
                        continue;
                    }
                    if presses[i].ok && first.is_none_or(|f| times[i] > times[f]) {
                        out.push(Label::Recast { press: i, episode: k });
                    } else {
                        out.push(Label::NoCast { press: i });
                    }
                }
            }
            for (i, t) in taken.iter().enumerate() {
                if !t {
                    out.push(Label::NoCast { press: i });
                }
            }
            out
        }
    }
}

// ---------- episodes (live) ----------

/// Live episode state for one game (cheap: rules + timing, and the R ability's name from
/// `/activeplayer` when it changes during a recast).
#[derive(Debug, Clone, Default)]
pub struct LiveEpisodes {
    /// End of the current episode (game seconds).
    pub until: Option<f64>,
    pub started: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LivePress {
    /// Not a possible cast (filtered) — nothing on the timeline.
    None,
    Used,
    Recast,
    FormSwap,
}

impl LiveEpisodes {
    /// An accepted R press at `t` for `kind`: a new ult, a recast, or a form swap.
    pub fn press(&mut self, kind: UltKind, cap: f64, t: f64) -> LivePress {
        match kind {
            UltKind::Transform => LivePress::FormSwap,
            UltKind::Normal | UltKind::ChargesOrReset => LivePress::Used,
            UltKind::Command | UltKind::MultiCast => {
                if self.until.is_some_and(|u| t <= u) {
                    LivePress::Recast
                } else {
                    self.started = Some(t);
                    self.until = Some(t + cap + 1.0);
                    LivePress::Used
                }
            }
        }
    }
    /// The R ability left its recast state (its name/id from `/activeplayer` is back to normal).
    pub fn ended(&mut self, t: f64) {
        if self.started.is_some_and(|s| t > s + 0.5) {
            self.until = None;
        }
    }
    /// Still in the recast state at `t` (the name says so): keep the episode open.
    pub fn extend(&mut self, t: f64) {
        if let Some(u) = self.until.as_mut() {
            *u = u.max(t + 1.5);
        }
    }
    pub fn reset(&mut self) {
        self.until = None;
    }
}

#[cfg(test)]
mod tests;
