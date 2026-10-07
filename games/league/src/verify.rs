//! Ult tracking, layer 2: after the game (maintenance pass, low priority), the live ult presses
//! are checked against the ability bar in the recording.
//!
//! 1. Find the game image (letterbox) and the ability bar ([`hud::locate`]) in ~24 frames spread
//!    over the game. Not found / low confidence (other HUD, other mode) → the live result stays,
//!    labelled "Ult pressed (unverified)".
//! 2. Full scan: the R icon on every keyframe (1 per second; decoding a keyframe needs no other
//!    frame). A cast = the cooldown overlay appearing between two keyframes.
//! 3. Refine: decode the frames between those two keyframes for the exact cast frame; and around
//!    every press without a cast nearby (−0.3 s .. +delay), which finds casts in ready windows
//!    shorter than a second.
//! 4. Match casts with presses (the press comes up to the champion's cast delay before the bar
//!    shows the cooldown): "Ult used" at the cast frame; casts without a logged press are "Ult
//!    used" too (other key / mouse bind); presses without a cast become "Ult pressed, no cast"
//!    (hidden by default on the timeline).

use crate::hud::{self, HudFit, IconSig, RLook};
use crate::ult::UltRules;
use crate::ultkind::{self, CastSig, KindRule, Label, PressIn, Signals, UltKind};
use cv_core::game::{FrameSource, KeyMark, Region};
use cv_core::session::{GameSession, Verification};
use cv_core::{EventKind, GameEvent};
use std::time::Instant;

/// Bump when the checking logic improves: older results are redone by the maintenance pass.
/// 3-4 (v1.6): ult kinds — recasts / commands of one ult, form swaps, charges; one recast per
/// burst of mashed presses (4; the v1.6 test builds wrote 3).
/// 5: summoner spells (D / F slot cooldowns) checked too.
pub const VERSION: u32 = 5;
/// Below this the ability bar isn't trusted.
pub const MIN_CONFIDENCE: f64 = 0.35;
const CALIBRATION_FRAMES: usize = 24;

/// A cast found in the video (video seconds).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cast {
    pub t: f64,
    /// Found by the keyframe scan (true) or only around a press (false).
    pub from_scan: bool,
}

/// One press in video seconds, with what layer 1 said.
#[derive(Debug, Clone, PartialEq)]
pub struct Press {
    pub t: f64,
    pub mark: KeyMark,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Cast confirmed; `press` is the matching press (if any).
    Used { cast: f64, press: Option<usize> },
    /// Press without a cast.
    NoCast { press: usize },
}

/// Finds casts in a sequence of R icon looks: the cooldown overlay appearing, compared with the
/// last look that wasn't dimmed (stun, silence, death...).
#[derive(Default)]
pub struct CastTracker {
    prev: Option<RLook>,
}

impl CastTracker {
    pub fn seeded(l: RLook) -> Self {
        CastTracker { prev: Some(l) }
    }
    /// True if this look starts a cast.
    pub fn push(&mut self, l: RLook) -> bool {
        if hud::dimmed(&l) {
            return false;
        }
        let cast = self.prev.is_some_and(|p| hud::is_cast(&p, &l));
        self.prev = Some(l);
        cast
    }
}

/// Matches casts and presses (both in video seconds). `delay` = the longest time from a press to
/// the cooldown appearing; `first_press` = champions with recasts (the first press of a burst
/// counts) instead of the press closest to the cast.
pub fn match_up(casts: &[f64], presses: &[f64], delay: f64, first_press: bool) -> Vec<Outcome> {
    let mut used = vec![false; presses.len()];
    let mut out = Vec::new();
    for &c in casts {
        let window: Vec<usize> = (0..presses.len()).filter(|&i| !used[i] && presses[i] >= c - delay - 0.1 && presses[i] <= c + 0.25).collect();
        let pick = if first_press {
            window.first().copied()
        } else {
            // The last press before the cast, else the first one just after it (clock jitter).
            window.iter().rev().find(|&&i| presses[i] <= c).copied().or_else(|| window.first().copied())
        };
        if let Some(i) = pick {
            used[i] = true;
        }
        out.push(Outcome::Used { cast: c, press: pick });
    }
    for (i, u) in used.iter().enumerate() {
        if !u {
            out.push(Outcome::NoCast { press: i });
        }
    }
    out
}

fn reason_text(r: Option<&str>) -> &'static str {
    match r.map(|r| r.split(' ').next().unwrap_or("")) {
        Some("cooldown") => "The ult was on cooldown.",
        Some("not_learned") => "The ult wasn't learned yet.",
        Some("dead") => "You were dead.",
        Some("chat") => "Typed in chat.",
        _ => "No cast in the recording (cancelled, no target, or not castable then).",
    }
}

/// The ult presses of a session: the key log, or (older recordings) the "Ult pressed" events.
fn presses_of(s: &mut GameSession) -> Vec<KeyMark> {
    let marks: Vec<KeyMark> = s.key_presses.iter().filter(|m| m.action == "ult").cloned().collect();
    if !marks.is_empty() || s.key_presses.iter().any(|m| m.action == "ult") {
        return marks;
    }
    // Older recordings: only accepted presses were kept, as events. Keep them as marks so a
    // later re-check still has them after the events are rewritten.
    let from_events: Vec<KeyMark> = s
        .events
        .iter()
        .filter(|e| e.kind == EventKind::UltPressed)
        .map(|e| KeyMark { game_time: e.game_time, action: "ult".into(), key: "R".into(), accepted: true, reason: None })
        .collect();
    s.key_presses.extend(from_events.iter().cloned());
    from_events
}

fn is_ult(k: EventKind) -> bool {
    matches!(k, EventKind::UltPressed | EventKind::UltUsed | EventKind::UltUnconfirmed | EventKind::UltRecast | EventKind::FormSwap)
}

/// The kind rule for this game: the rules file, else the kind saved with the game (Data
/// Dragon's guess for a champion the rules didn't know), else normal.
pub fn kind_of(s: &GameSession, rules: &UltRules) -> KindRule {
    let champ = s.player.as_ref().and_then(|p| p.character_id.clone()).unwrap_or_default();
    let guess = s.key_presses.iter().find(|m| m.action == "ult_kind").and_then(|m| UltKind::parse(&m.key));
    rules.kind_for(&champ, guess)
}

/// Keeps the live result, labelled unverified, and records why.
#[cfg(test)]
fn keep_live(s: &mut GameSession, status: &str, reason: String, confidence: f64, details: serde_json::Value, started: Instant) {
    keep_live_with(s, status, reason, confidence, details, started, UltKind::Normal)
}

fn keep_live_with(s: &mut GameSession, status: &str, reason: String, confidence: f64, details: serde_json::Value, started: Instant, kind: UltKind) {
    let marks = presses_of(s);
    s.events.retain(|e| !is_ult(e.kind));
    for m in marks.iter().filter(|m| m.accepted) {
        let (k, title) = if m.reason.as_deref() == Some("recast") {
            (EventKind::UltRecast, "Ult recast (unverified)")
        } else if kind == UltKind::Transform {
            (EventKind::FormSwap, "Form swap (unverified)")
        } else {
            (EventKind::UltPressed, "Ult pressed (unverified)")
        };
        s.events
            .push(GameEvent::new(format!("ult-{:.2}", m.game_time), k, m.game_time, title).with_details(format!("Ult key pressed ({}). Not checked against the recording: {reason}", m.key)));
    }
    s.events.sort_by(|a, b| a.game_time.partial_cmp(&b.game_time).unwrap_or(std::cmp::Ordering::Equal));
    s.verification = Some(Verification {
        version: VERSION,
        at: chrono::Local::now(),
        status: status.into(),
        reason: Some(reason),
        confidence,
        casts: 0,
        confirmed: 0,
        video_only: 0,
        unconfirmed: 0,
        analysis_ms: started.elapsed().as_millis() as u64,
        details,
    });
}

/// The spell in summoner slot `slot` at video time `t`, from the session's summoner chips (the
/// live ones carry the name and icon from the player list); later spell changes (Smite,
/// Teleport upgrades) are picked up by the nearest earlier chip.
fn spell_at(chips: &[GameEvent], slot: usize, t: f64) -> (crate::summoners::Spell, String) {
    let slot_name = ["D", "F"][slot];
    let mine: Vec<&GameEvent> = chips.iter().filter(|e| e.facts.iter().any(|(k, v)| k == "Slot" && v == slot_name) && e.icon.is_some()).collect();
    let pick = mine.iter().rev().find(|e| e.game_time <= t + 1.0).or(mine.first());
    match pick {
        Some(e) => {
            let ic = e.icon.as_ref().unwrap();
            (crate::summoners::Spell { id: ic.id.clone(), name: e.title.clone() }, ic.version.clone())
        }
        None => (crate::summoners::Spell::default(), String::new()),
    }
}

/// Summoner spells: a cast = the D / F slot's cooldown overlay appearing between two keyframes
/// (summoner cooldowns are minutes long, so 1 s keyframes see every one), refined to the exact
/// frame. Presses name the key; presses without a cast are dropped; casts without a press
/// (mouse click on the icon, unfocused window) still get their chip.
fn summoner_pass(
    s: &mut GameSession,
    series: &[Vec<(f64, RLook)>; 2],
    v: &mut dyn FrameSource,
    fit: &HudFit,
    (w, h): (u32, u32),
    stop: &dyn Fn() -> anyhow::Result<()>,
) -> anyhow::Result<serde_json::Value> {
    let offset = s.video_offset;
    let game_end = s.events.iter().filter(|e| e.kind == EventKind::GameEnd).map(|e| e.game_time + offset).fold(f64::INFINITY, f64::min);
    let live: Vec<GameEvent> = s.events.iter().filter(|e| e.kind == EventKind::SummonerSpell).cloned().collect();
    let marks: Vec<KeyMark> = s.key_presses.iter().filter(|m| crate::summoners::SLOTS.contains(&m.action.as_str())).cloned().collect();
    let mut out = Vec::new();
    let mut counts = [0usize; 2];
    let mut matched = 0;
    for slot in 0..2 {
        let (x, y, size) = fit.summoner(slot);
        let reg = Region { x: (x - 2.0).max(0.0) as u32, y: (y - 2.0).max(0.0) as u32, w: ((size + 4.0).ceil() as u32).min(w), h: ((size + 4.0).ceil() as u32).min(h) };
        let at = (reg.x, reg.y);
        let ser = &series[slot];
        let mut last_clear: Option<usize> = None;
        for k in 0..ser.len() {
            let (t1, b) = ser[k];
            if hud::dimmed(&b) {
                continue;
            }
            let Some(pk) = last_clear.replace(k) else { continue };
            let (t0, a) = ser[pk];
            if !hud::is_cast(&a, &b) || t1 >= game_end - 0.5 {
                continue;
            }
            let mut tr = CastTracker::seeded(a);
            let mut exact = None;
            stop()?;
            v.frames(t0, t1, reg, &mut |pt, f| {
                if pt <= t0 {
                    return true;
                }
                if tr.push(hud::look_summoner(f, at, fit, slot)) {
                    exact = Some(pt);
                    return false;
                }
                true
            })?;
            let cast = exact.unwrap_or(t1);
            let gt = cast - offset;
            // The press of this slot just before the cast (the cooldown shows within ~0.3 s;
            // channelled spells like Teleport show it when the channel starts).
            let press = marks.iter().rfind(|m| m.action == crate::summoners::SLOTS[slot] && m.game_time <= gt + 0.25 && m.game_time >= gt - 1.5);
            let (spell, version) = spell_at(&live, slot, gt);
            let mut e = crate::summoners::event(slot, &spell, gt, &version, press.map(|m| m.key.as_str()).unwrap_or(""), true);
            if press.is_some() {
                matched += 1;
            } else {
                e.details = Some("Seen in the recording (the spell went on cooldown); no key press was logged (clicked, another bind, or the game wasn't in focus).".into());
            }
            counts[slot] += 1;
            out.push(e);
        }
    }
    s.events.retain(|e| e.kind != EventKind::SummonerSpell);
    s.events.extend(out);
    s.events.sort_by(|a, b| a.game_time.partial_cmp(&b.game_time).unwrap_or(std::cmp::Ordering::Equal));
    Ok(serde_json::json!({ "casts_d": counts[0], "casts_f": counts[1], "matched_presses": matched, "presses": marks.iter().filter(|m| m.accepted).count(), "live_chips": live.len() }))
}

/// The keyframes to scan: the game itself (after the loading screen).
fn game_keyframes(v: &dyn FrameSource, offset: f64) -> Vec<f64> {
    v.keyframes().iter().copied().filter(|t| *t >= offset.max(0.0) - 0.5).collect()
}

pub fn verify(s: &mut GameSession, v: &mut dyn FrameSource, rules: &UltRules, cancel: &dyn Fn() -> bool) -> anyhow::Result<()> {
    let started = Instant::now();
    let (w, h) = v.size();
    let offset = s.video_offset;
    let champ = s.player.as_ref().and_then(|p| p.character_id.clone()).unwrap_or_default();
    let kind = kind_of(s, rules);
    let kfs = game_keyframes(v, offset);
    if kfs.len() < 5 {
        keep_live_with(s, "skipped", "the recording is too short".into(), 0.0, serde_json::json!({}), started, kind.kind);
        return Ok(());
    }
    let stop = || -> anyhow::Result<()> {
        if cancel() {
            anyhow::bail!("stopped: a game started")
        }
        Ok(())
    };

    // 1. Letterbox + ability bar.
    let picks: Vec<f64> = (0..CALIBRATION_FRAMES).map(|i| kfs[(kfs.len() - 1) * (2 * i + 1) / (2 * CALIBRATION_FRAMES)]).collect();
    let mut full = Vec::new();
    for &t in picks.iter().step_by(4) {
        stop()?;
        if let Some((_, f)) = v.frame_at(t, Region { x: 0, y: 0, w, h })? {
            full.push(f);
        }
    }
    let content = hud::find_content(&full).unwrap_or(hud::Content::full(w, h));
    drop(full);
    let creg = hud::calibration_region(w, h);
    let mut bands = Vec::new();
    for &t in &picks {
        stop()?;
        if let Some((_, f)) = v.frame_at(t, creg)? {
            bands.push(f);
        }
    }
    let fit = hud::locate(&bands, (creg.x, creg.y), content);
    let details = |fit: Option<&HudFit>| serde_json::json!({ "content": content, "fit": fit, "frames": bands.len(), "champion": champ });
    let fit = match fit {
        Some(f) if f.confidence >= MIN_CONFIDENCE => f,
        other => {
            let conf = other.map(|f| f.confidence).unwrap_or(0.0);
            let d = details(other.as_ref());
            keep_live_with(s, "skipped", "the ability bar wasn't found in the video (another HUD or game mode)".into(), conf, d, started, kind.kind);
            return Ok(());
        }
    };
    let details_v = details(Some(&fit));
    drop(bands);

    // 2. Full scan: R (and the summoner slots D / F, same crop) on every keyframe.
    let rreg = fit.r_region(w, h);
    let at = (rreg.x, rreg.y);
    let breg = fit.bar_region(w, h);
    let bat = (breg.x, breg.y);
    let mut series: Vec<(f64, RLook)> = Vec::with_capacity(kfs.len());
    let mut summ: [Vec<(f64, RLook)>; 2] = [Vec::with_capacity(kfs.len()), Vec::with_capacity(kfs.len())];
    // The icon's picture (only needed for champions with recast / command states).
    let mut sigs: Vec<IconSig> = Vec::new();
    let want_sig = kind.kind.has_episodes();
    for &t in &kfs {
        stop()?;
        if let Some((pt, f)) = v.frame_at(t, breg)? {
            series.push((pt, hud::look(&f, bat, &fit)));
            for (i, sv) in summ.iter_mut().enumerate() {
                sv.push((pt, hud::look_summoner(&f, bat, &fit, i)));
            }
            if want_sig {
                sigs.push(hud::signature(&f, bat, &fit));
            }
        }
    }
    let mut casts: Vec<Cast> = Vec::new();
    let mut last_clear: Option<usize> = None;
    for k in 0..series.len() {
        let (t1, b) = series[k];
        if hud::dimmed(&b) {
            continue;
        }
        let prev = last_clear.replace(k);
        let Some(pk) = prev else { continue };
        let (t0, a) = series[pk];
        if hud::is_cast(&a, &b) {
            // 3a. The exact frame between the two keyframes.
            let mut tr = CastTracker::seeded(a);
            let mut exact = None;
            stop()?;
            v.frames(t0, t1, rreg, &mut |pt, f| {
                if pt <= t0 {
                    return true;
                }
                if tr.push(hud::look(f, at, &fit)) {
                    exact = Some(pt);
                    return false;
                }
                true
            })?;
            casts.push(Cast { t: exact.unwrap_or(t1), from_scan: true });
        }
    }

    // 3b. Around presses without a cast nearby (ready windows shorter than a keyframe gap).
    let marks = presses_of(s);
    let delay = rules.delay(&champ);
    let dur = v.duration();
    // Presses outside the video (app started mid-game, or after the recording stopped) can't be
    // checked: they stay as live presses, labelled unverified.
    let (inside, outside): (Vec<KeyMark>, Vec<KeyMark>) = marks.into_iter().partition(|m| {
        let t = m.game_time + offset;
        t >= 0.5 && t <= dur - 0.5
    });
    let marks = inside;
    let presses: Vec<f64> = marks.iter().map(|m| m.game_time + offset).collect();
    let mut windows: Vec<(f64, f64)> = presses
        .iter()
        .filter(|&&p| !casts.iter().any(|c| c.t >= p - 0.35 && c.t <= p + delay + 0.25))
        .map(|&p| (p - 0.3, p + delay + 0.2))
        .collect();
    windows.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    let mut merged: Vec<(f64, f64)> = Vec::new();
    for w in windows {
        match merged.last_mut() {
            Some(m) if w.0 <= m.1 => m.1 = m.1.max(w.1),
            _ => merged.push(w),
        }
    }
    for (a, b) in merged {
        stop()?;
        let (a, b) = (a.max(0.0), b.min(dur));
        if b <= a {
            continue;
        }
        let mut tr = CastTracker::default();
        let mut found: Vec<f64> = Vec::new();
        let r = v.frames(a, b, rreg, &mut |pt, f| {
            if tr.push(hud::look(f, at, &fit)) && !found.iter().any(|t| pt - t < 1.0) {
                found.push(pt);
            }
            true
        });
        if let Err(e) = r {
            // One unreadable stretch (e.g. a damaged end of file) doesn't spoil the rest.
            log::warn!("ult check: frames {a:.1}-{b:.1} s unreadable: {e:#}");
            continue;
        }
        for t in found {
            if !casts.iter().any(|c| (c.t - t).abs() < 1.0) {
                casts.push(Cast { t, from_scan: false });
            }
        }
    }
    casts.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap());
    log::debug!("ult casts {casts:?}");

    // 4. Match and rewrite the ult events.
    let cast_times: Vec<f64> = casts.iter().map(|c| c.t).collect();
    s.events.retain(|e| !is_ult(e.kind));
    let (mut confirmed, mut video_only, mut unconfirmed) = (0, 0, 0);
    let mut kind_details = serde_json::json!({ "kind": kind.kind.as_str() });
    if kind.kind == UltKind::Normal {
        // One press = one ult: the v1.3 logic, unchanged.
        let outcomes = match_up(&cast_times, &presses, delay, rules.skips_cooldown(&champ));
        for o in &outcomes {
            match *o {
                Outcome::Used { cast, press } => {
                    let gt = cast - offset;
                    let details = match press {
                        Some(i) => {
                            confirmed += 1;
                            format!("Confirmed from the recording ({} pressed {:.2} s before the cast).", marks[i].key, (cast - presses[i]).max(0.0))
                        }
                        None => {
                            video_only += 1;
                            "Seen in the recording; no ult key press was logged (mouse or another bind, or the game wasn't in focus).".to_string()
                        }
                    };
                    s.events.push(GameEvent::new(format!("ult-{gt:.2}"), EventKind::UltUsed, gt, "Ult used").with_details(details));
                }
                Outcome::NoCast { press } => {
                    unconfirmed += 1;
                    let m = &marks[press];
                    s.events
                        .push(GameEvent::new(format!("ultp-{:.2}", m.game_time), EventKind::UltUnconfirmed, m.game_time, "Ult pressed, no cast").with_details(format!(
                            "{} pressed. {}",
                            m.key,
                            reason_text(m.reason.as_deref())
                        )));
                }
            }
        }
    } else {
        // v1.6: ult kinds. What the ability bar showed: casts (cooldown appearing, long or a
        // short lockout), the icon's other picture (recast / command state), its ready look.
        let look_at = |t: f64| -> Option<RLook> {
            let i = series.partition_point(|(pt, _)| *pt <= t + 1e-6);
            (i > 0).then(|| series[i - 1].1)
        };
        let cast_sigs: Vec<CastSig> = casts
            .iter()
            .map(|c| CastSig { t: c.t, long: look_at(c.t + ultkind::LONG_COOLDOWN).is_some_and(|l| l.state() == hud::RState::Cooldown) })
            .collect();
        let mut sig = Signals { casts: cast_sigs, ..Default::default() };
        if want_sig && sigs.len() == series.len() {
            let ready_sigs: Vec<IconSig> = series.iter().zip(&sigs).filter(|((_, l), _)| !hud::dimmed(l) && l.state() == hud::RState::Ready).map(|(_, g)| *g).collect();
            if let Some(reference) = hud::ready_reference(&ready_sigs) {
                let mut cur: Option<(f64, usize)> = None;
                for (k, ((pt, l), g)) in series.iter().zip(&sigs).enumerate() {
                    if hud::dimmed(l) {
                        continue;
                    }
                    let alt = l.state() == hud::RState::Ready && hud::sig_dist(g, &reference) > hud::ALT_DIST;
                    if l.state() == hud::RState::Ready && !alt {
                        sig.ready.push(*pt);
                    }
                    match (alt, cur) {
                        (true, None) => cur = Some((*pt, k)),
                        (false, Some((st, sk))) => {
                            // The exact frame the icon changed: decode from the keyframe before.
                            let from = if sk > 0 { series[sk - 1].0 } else { st };
                            let mut exact = st;
                            if from < st {
                                stop()?;
                                let _ = v.frames(from, st, rreg, &mut |ft, f| {
                                    if ft > from && hud::sig_dist(&hud::signature(f, at, &fit), &reference) > hud::ALT_DIST {
                                        exact = ft;
                                        return false;
                                    }
                                    true
                                });
                            }
                            sig.alt.push((exact, *pt));
                            cur = None;
                        }
                        _ => {}
                    }
                }
                if let Some((st, _)) = cur {
                    sig.alt.push((st, dur));
                }
            }
        }
        sig.deaths = s.events.iter().filter(|e| e.kind == EventKind::Death).map(|e| e.game_time + offset).collect();
        let pin: Vec<PressIn> = marks
            .iter()
            .zip(&presses)
            .map(|(m, &t)| PressIn { t, ok: m.accepted || matches!(m.reason.as_deref().map(|r| r.split(' ').next().unwrap_or("")), Some("cooldown") | Some("mouse") | Some("recast")) })
            .collect();
        // An icon picture change only counts as a recast / command state with an R press that
        // could have caused it, and not after the game (victory screen) or running into the end
        // of the video (the HUD fades out): a lone change is a misread, not a video-only ult.
        let game_end = s.events.iter().filter(|e| e.kind == EventKind::GameEnd).map(|e| e.game_time + offset).fold(f64::INFINITY, f64::min);
        let alt_all = sig.alt.len();
        sig.alt
            .retain(|&(st, en)| st < game_end - 1.0 && en < dur - 0.5 && pin.iter().any(|p| p.ok && p.t >= st - kind.cast_delay().max(delay) - 0.3 && p.t <= st + 0.5));
        sig.ready.retain(|&t| t < game_end);
        let ammo_ready = |t: f64| look_at(t).is_some_and(|l| !hud::dimmed(&l) && l.state() != hud::RState::Cooldown);
        let labels = ultkind::label(&kind, delay, &sig, &pin, &ammo_ready);
        let (mut recasts, mut swaps) = (0, 0);
        for l in &labels {
            match *l {
                Label::Used { at: cast, press } | Label::FormSwap { at: cast, press } => {
                    let gt = cast - offset;
                    let swap = matches!(l, Label::FormSwap { .. });
                    let details = match press {
                        Some(i) => {
                            confirmed += 1;
                            format!("Confirmed from the recording ({} pressed {:.2} s before).", marks[i].key, (cast - presses[i]).max(0.0))
                        }
                        None => {
                            video_only += 1;
                            "Seen in the recording; no ult key press was logged (mouse or another bind, or the game wasn't in focus).".to_string()
                        }
                    };
                    if swap {
                        swaps += 1;
                        s.events.push(GameEvent::new(format!("form-{gt:.2}"), EventKind::FormSwap, gt, "Form swap").with_details(details));
                    } else {
                        s.events.push(GameEvent::new(format!("ult-{gt:.2}"), EventKind::UltUsed, gt, "Ult used").with_details(details));
                    }
                }
                Label::Recast { press, .. } => {
                    recasts += 1;
                    let m = &marks[press];
                    s.events.push(
                        GameEvent::new(format!("ultr-{:.2}", m.game_time), EventKind::UltRecast, m.game_time, "Ult recast")
                            .with_details(format!("{} pressed again during the same ult (command, second part or early end): not a new ult.", m.key)),
                    );
                }
                Label::NoCast { press } => {
                    unconfirmed += 1;
                    let m = &marks[press];
                    s.events
                        .push(GameEvent::new(format!("ultp-{:.2}", m.game_time), EventKind::UltUnconfirmed, m.game_time, "Ult pressed, no cast").with_details(format!(
                            "{} pressed. {}",
                            m.key,
                            reason_text(m.reason.as_deref())
                        )));
                }
            }
        }
        kind_details = serde_json::json!({
            "kind": kind.kind.as_str(),
            "recasts": recasts,
            "form_swaps": swaps,
            "episodes": ultkind::episodes(&kind, &sig).len(),
            "alt_states": sig.alt.len(),
            "alt_states_dropped": alt_all - sig.alt.len(),
            "long_casts": sig.casts.iter().filter(|c| c.long).count(),
        });
    }
    let summoner_details = summoner_pass(s, &summ, v, &fit, (w, h), &stop)?;
    for m in outside.iter().filter(|m| m.accepted) {
        s.events.push(
            GameEvent::new(format!("ult-{:.2}", m.game_time), EventKind::UltPressed, m.game_time, "Ult pressed (unverified)")
                .with_details(format!("Ult key pressed ({}). Not checked: this moment isn't in the recording.", m.key)),
        );
    }
    s.events.sort_by(|a, b| a.game_time.partial_cmp(&b.game_time).unwrap_or(std::cmp::Ordering::Equal));
    let mut details = details_v;
    details["presses_outside_video"] = serde_json::json!(outside.len());
    details["keyframes"] = serde_json::json!(series.len());
    details["casts_from_scan"] = serde_json::json!(casts.iter().filter(|c| c.from_scan).count());
    details["ult_kind"] = kind_details;
    details["summoners"] = summoner_details;
    s.verification = Some(Verification {
        version: VERSION,
        at: chrono::Local::now(),
        status: "verified".into(),
        reason: None,
        confidence: fit.confidence,
        casts: casts.len(),
        confirmed,
        video_only,
        unconfirmed,
        analysis_ms: started.elapsed().as_millis() as u64,
        details,
    });
    log::info!(
        "ult check of {}: {} casts ({} matched a press, {} from the video only), {} presses without a cast, confidence {:.2}, {} ms",
        s.id,
        casts.len(),
        confirmed,
        video_only,
        unconfirmed,
        fit.confidence,
        started.elapsed().as_millis()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cv_core::game::{Region, Rgb};

    #[test]
    fn casts_and_presses() {
        // Presses at 10 (cast at 10.1), 30 and 30.5 (cast at 31.0, Caitlyn-like delay), 50 (no
        // cast: on cooldown); a cast at 70 without a press.
        let presses = [10.0, 30.0, 30.5, 50.0];
        let o = match_up(&[10.1, 31.0, 70.0], &presses, 1.5, false);
        assert_eq!(
            o,
            vec![
                Outcome::Used { cast: 10.1, press: Some(0) },
                Outcome::Used { cast: 31.0, press: Some(2) },
                Outcome::Used { cast: 70.0, press: None },
                Outcome::NoCast { press: 1 },
                Outcome::NoCast { press: 3 },
            ]
        );
        // Recast champions: the first press of the burst.
        let o = match_up(&[31.0], &presses, 1.5, true);
        assert_eq!(o[0], Outcome::Used { cast: 31.0, press: Some(1) });
        // A press logged a hair after the HUD changed (clock jitter) still matches.
        assert_eq!(match_up(&[10.0], &[10.2], 1.2, false)[0], Outcome::Used { cast: 10.0, press: Some(0) });
        // Too early to be this cast.
        assert_eq!(match_up(&[10.0], &[8.0], 1.2, false)[1], Outcome::NoCast { press: 0 });
    }

    #[test]
    fn old_recordings_keep_their_presses() {
        let mut s = GameSession::new("x".into(), "league", "League of Legends", chrono::Local::now());
        s.events.push(GameEvent::new("ult-1", EventKind::UltPressed, 100.0, "Ult pressed"));
        let p = presses_of(&mut s);
        assert_eq!(p.len(), 1);
        assert_eq!(s.key_presses.len(), 1, "kept for later re-checks");
        keep_live(&mut s, "skipped", "test".into(), 0.0, serde_json::json!({}), Instant::now());
        assert_eq!(s.events.len(), 1);
        assert_eq!(s.events[0].title, "Ult pressed (unverified)");
        assert_eq!(presses_of(&mut s).len(), 1);
    }

    #[test]
    fn mouse_presses_match_casts_and_stay_hidden_otherwise() {
        // A mouse-bound ult press (from the input recording) next to a cast: "Ult used" with a
        // press instead of "cast without a logged press".
        let o = match_up(&[40.2], &[40.0], 0.5, false);
        assert_eq!(o, vec![Outcome::Used { cast: 40.2, press: Some(0) }]);
        // Unchecked recording: mouse marks (not accepted live) never become "Ult pressed".
        let mut s = GameSession::new("x".into(), "league", "League of Legends", chrono::Local::now());
        s.key_presses.push(KeyMark { game_time: 40.0, action: "ult".into(), key: "Mouse 5".into(), accepted: false, reason: Some("mouse".into()) });
        keep_live(&mut s, "skipped", "test".into(), 0.0, serde_json::json!({}), Instant::now());
        assert!(s.events.is_empty());
    }

    #[test]
    fn stun_is_not_a_cast() {
        let ready = RLook { blue: 0.16, dark: 0.23, digits: 0.01 };
        let cd = |b| RLook { blue: b, dark: 0.15, digits: 0.15 };
        let dim = RLook { blue: 0.16, dark: 0.64, digits: 0.0 };
        let mut t = CastTracker::default();
        let seq = [ready, ready, cd(0.76), cd(0.7), dim, dim, cd(0.69), dim, cd(0.68), cd(0.3), ready, dim, cd(0.8)];
        let casts: Vec<usize> = seq.iter().enumerate().filter(|(_, l)| t.push(**l)).map(|(i, _)| i).collect();
        // The cast at 2; the cooldown back after stuns isn't one; ready → stunned → cast at 12 is.
        assert_eq!(casts, vec![2, 12]);
    }

    /// A synthetic recording: a 1920×1080 game with the owner's real ability bar (band crop) and
    /// the R icon crop of `state(t)` pasted in, 30 fps, keyframes every second.
    struct Synth {
        dur: f64,
        kfs: Vec<f64>,
        band: Rgb,
        icons: Vec<Rgb>,
        state: Box<dyn Fn(f64) -> usize>,
        /// The D slot on cooldown (pixels from another real frame) while this says so.
        d_cooldown: Box<dyn Fn(f64) -> bool>,
        d_band: Rgb,
    }
    fn ppm(n: &str) -> Rgb {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/hud").join(n);
        Rgb::from_ppm(&std::fs::read(p).unwrap()).unwrap()
    }
    impl Synth {
        fn new(dur: f64, icons: &[&str], state: impl Fn(f64) -> usize + 'static) -> Synth {
            Synth {
                dur,
                kfs: (0..dur as usize).map(|s| s as f64).collect(),
                band: ppm("band-caitlyn-544-1.ppm"),
                icons: icons.iter().map(|n| ppm(n)).collect(),
                state: Box::new(state),
                d_cooldown: Box::new(|_| false),
                d_band: ppm("band-caitlyn-200-3.ppm"),
            }
        }
        fn render(&self, t: f64, r: Region) -> Rgb {
            let icon = &self.icons[(self.state)(t)];
            let mut data = Vec::with_capacity((r.w * r.h * 3) as usize);
            for y in r.y..r.y + r.h {
                for x in r.x..r.x + r.w {
                    let d_cd = (self.d_cooldown)(t);
                    let p = if (934..984).contains(&x) && (987..1037).contains(&y) {
                        icon.px(x - 934, y - 987)
                    } else if d_cd && (986..1022).contains(&x) && (990..1026).contains(&y) {
                        self.d_band.px(x - 780, y - 980)
                    } else if (780..1140).contains(&x) && (980..1080).contains(&y) {
                        self.band.px(x - 780, y - 980)
                    } else {
                        [70, 80, 60]
                    };
                    data.extend_from_slice(&p);
                }
            }
            Rgb { w: r.w, h: r.h, data }
        }
    }
    impl FrameSource for Synth {
        fn size(&self) -> (u32, u32) {
            (1920, 1080)
        }
        fn duration(&self) -> f64 {
            self.dur
        }
        fn keyframes(&self) -> &[f64] {
            &self.kfs
        }
        fn frame_at(&mut self, t: f64, region: Region) -> anyhow::Result<Option<(f64, Rgb)>> {
            let ft = (t * 30.0 - 1e-6).ceil() / 30.0;
            Ok(Some((ft, self.render(ft, region))))
        }
        fn frames(&mut self, t0: f64, t1: f64, region: Region, f: &mut dyn FnMut(f64, &Rgb) -> bool) -> anyhow::Result<()> {
            let mut k = (t0 * 30.0 - 1e-6).ceil() as i64;
            while (k as f64) / 30.0 <= t1 {
                let t = k as f64 / 30.0;
                if !f(t, &self.render(t, region)) {
                    break;
                }
                k += 1;
            }
            Ok(())
        }
    }
    fn session(champ: &str, presses: &[(f64, Option<&str>)]) -> GameSession {
        let mut s = GameSession::new("x".into(), "league", "League of Legends", chrono::Local::now());
        s.player = Some(cv_core::game::PlayerInfo { name: "me".into(), character: Some(champ.into()), character_id: Some(champ.into()), team: None, mode: None });
        for &(t, reason) in presses {
            s.key_presses
                .push(KeyMark { game_time: t, action: "ult".into(), key: "R".into(), accepted: reason.is_none() || reason == Some("recast"), reason: reason.map(|r| r.to_string()) });
        }
        s
    }
    fn kinds(s: &GameSession) -> (usize, usize, usize, usize) {
        let n = |k| s.events.iter().filter(|e| e.kind == k).count();
        (n(EventKind::UltUsed), n(EventKind::UltRecast), n(EventKind::FormSwap), n(EventKind::UltUnconfirmed))
    }

    #[test]
    fn annie_in_a_synthetic_recording() {
        // Ready; Tibbers 100.2-145 (command icon), cooldown 145-245, ready; Tibbers again at 260.2.
        let icons = ["r-caitlyn-ready.ppm", "r-caitlyn-alt-synthetic.ppm", "r-caitlyn-cooldown.ppm"];
        let mut v = Synth::new(300.0, &icons, |t| {
            if (100.2..145.0).contains(&t) || (260.2..290.0).contains(&t) {
                1
            } else if (145.0..245.0).contains(&t) {
                2
            } else {
                0
            }
        });
        let mut presses: Vec<(f64, Option<&str>)> = vec![(100.0, None)];
        presses.extend((0..10).map(|i| (102.0 + i as f64 * 2.0, Some("recast"))));
        presses.push((150.0, None));
        presses.push((260.0, None));
        let mut s = session("Annie", &presses);
        verify(&mut s, &mut v, &UltRules::builtin(), &|| false).unwrap();
        assert_eq!(s.verification.as_ref().unwrap().status, "verified");
        assert_eq!(kinds(&s), (2, 10, 0, 1), "{:#?}", s.events);
        let used: Vec<f64> = s.events.iter().filter(|e| e.kind == EventKind::UltUsed).map(|e| e.game_time).collect();
        assert!((used[0] - 100.2).abs() < 0.04 && (used[1] - 260.2).abs() < 0.04, "the exact frame the icon changed: {used:?}");
        let d = &s.verification.as_ref().unwrap().details["ult_kind"];
        assert_eq!(d["kind"], "command");
        assert_eq!(d["recasts"], 10);
    }

    #[test]
    fn lone_icon_changes_are_not_ults() {
        // Riven-like: the icon's picture changes without any R press (a HUD effect at 50 s) and
        // again into the end of the video (victory screen): no "Ult used" (seen in the owner's
        // Riven Practice Tool recording before this rule).
        let icons = ["r-caitlyn-ready.ppm", "r-caitlyn-alt-synthetic.ppm"];
        let mut v = Synth::new(120.0, &icons, |t| ((50.0..53.0).contains(&t) || t >= 110.0) as usize);
        let mut s = session("Riven", &[]);
        s.events.push(GameEvent::new("end", EventKind::GameEnd, 109.0, "Victory"));
        verify(&mut s, &mut v, &UltRules::builtin(), &|| false).unwrap();
        assert_eq!(kinds(&s), (0, 0, 0, 0), "{:#?}", s.events);
        assert_eq!(s.verification.as_ref().unwrap().details["ult_kind"]["alt_states_dropped"], 2);
    }

    #[test]
    fn caitlyn_unchanged_and_jayce_form_swaps() {
        // Normal: a cast at 50 (cooldown until 140), a press on cooldown at 60.
        let icons = ["r-caitlyn-ready.ppm", "r-caitlyn-cooldown.ppm"];
        let mut v = Synth::new(200.0, &icons, |t| if (50.0..140.0).contains(&t) { 1 } else { 0 });
        let mut s = session("Caitlyn", &[(49.9, None), (60.0, Some("cooldown (70 s left)"))]);
        verify(&mut s, &mut v, &UltRules::builtin(), &|| false).unwrap();
        assert_eq!(kinds(&s), (1, 0, 0, 1));
        // Jayce: a 6 s cooldown after each swap.
        let mut v = Synth::new(200.0, &icons, |t| [30.0, 60.0, 90.0].iter().any(|&c| t >= c && t < c + 6.0) as usize);
        let mut s = session("Jayce", &[(29.95, None), (59.95, None), (89.95, None), (92.0, None)]);
        verify(&mut s, &mut v, &UltRules::builtin(), &|| false).unwrap();
        assert_eq!(kinds(&s), (0, 0, 3, 1));
    }

    #[test]
    fn summoner_casts_from_the_recording() {
        // Flash on D: casts at 40.5 and 200.25 (cooldown 300 s, cut short here), a D press on
        // cooldown at 60 (no cast), a cast at 150.1 without a press (clicked). F (Ignite) stays
        // on cooldown the whole time: no casts. Plus the victory screen: nothing after the end.
        let icons = ["r-caitlyn-ready.ppm"];
        let mut v = Synth::new(260.0, &icons, |_| 0);
        v.d_cooldown = Box::new(|t| (40.5..120.0).contains(&t) || (150.1..180.0).contains(&t) || (200.25..240.0).contains(&t) || t >= 251.0);
        let mut s = session("Caitlyn", &[]);
        s.events.push(GameEvent::new("end", EventKind::GameEnd, 250.0, "Victory"));
        let flash = crate::summoners::Spell { id: "SummonerFlash".into(), name: "Flash".into() };
        for (t, ok) in [(40.4, true), (60.0, false), (200.2, true)] {
            s.key_presses
                .push(KeyMark { game_time: t, action: "summoner1".into(), key: "D".into(), accepted: ok, reason: (!ok).then(|| "cooldown".into()) });
            if ok {
                s.events.push(crate::summoners::event(0, &flash, t, "16.20.1", "D", false));
            }
        }
        verify(&mut s, &mut v, &UltRules::builtin(), &|| false).unwrap();
        let chips: Vec<&GameEvent> = s.events.iter().filter(|e| e.kind == EventKind::SummonerSpell).collect();
        let times: Vec<f64> = chips.iter().map(|e| e.game_time).collect();
        assert_eq!(chips.len(), 3, "{times:?}");
        for (got, want) in times.iter().zip([40.5, 150.1, 200.25]) {
            assert!((got - want).abs() < 0.04, "the exact frame: {times:?}");
        }
        assert!(chips.iter().all(|e| e.title == "Flash" && e.icon.as_ref().is_some_and(|i| i.id == "SummonerFlash")), "named from the live chips");
        assert!(chips[0].details.as_deref().unwrap().starts_with("Confirmed"));
        assert!(chips[1].details.as_deref().unwrap().contains("no key press"), "clicked: still a chip");
        assert_eq!(chips[1].facts.iter().find(|(k, _)| k == "Key").map(|(_, v)| v.as_str()), None);
        let d = &s.verification.as_ref().unwrap().details["summoners"];
        assert_eq!((d["casts_d"].as_u64(), d["casts_f"].as_u64(), d["matched_presses"].as_u64()), (Some(3), Some(0), Some(2)));
        // The ult result is untouched by the summoner pass.
        assert_eq!(kinds(&s), (0, 0, 0, 0));
    }

    #[test]
    fn reasons() {
        assert_eq!(reason_text(Some("cooldown (12 s left)")), "The ult was on cooldown.");
        assert_eq!(reason_text(None), "No cast in the recording (cancelled, no target, or not castable then).");
    }
}
