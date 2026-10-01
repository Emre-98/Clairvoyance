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

use crate::hud::{self, HudFit, RLook};
use crate::ult::UltRules;
use cv_core::game::{FrameSource, KeyMark, Region};
use cv_core::session::{GameSession, Verification};
use cv_core::{EventKind, GameEvent};
use std::time::Instant;

/// Bump when the checking logic improves: older results are redone by the maintenance pass.
pub const VERSION: u32 = 1;
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
    matches!(k, EventKind::UltPressed | EventKind::UltUsed | EventKind::UltUnconfirmed)
}

/// Keeps the live result, labelled unverified, and records why.
fn keep_live(s: &mut GameSession, status: &str, reason: String, confidence: f64, details: serde_json::Value, started: Instant) {
    let marks = presses_of(s);
    s.events.retain(|e| !is_ult(e.kind));
    for m in marks.iter().filter(|m| m.accepted) {
        s.events.push(
            GameEvent::new(format!("ult-{:.2}", m.game_time), EventKind::UltPressed, m.game_time, "Ult pressed (unverified)")
                .with_details(format!("Ult key pressed ({}). Not checked against the recording: {reason}", m.key)),
        );
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

/// The keyframes to scan: the game itself (after the loading screen).
fn game_keyframes(v: &dyn FrameSource, offset: f64) -> Vec<f64> {
    v.keyframes().iter().copied().filter(|t| *t >= offset.max(0.0) - 0.5).collect()
}

pub fn verify(s: &mut GameSession, v: &mut dyn FrameSource, rules: &UltRules, cancel: &dyn Fn() -> bool) -> anyhow::Result<()> {
    let started = Instant::now();
    let (w, h) = v.size();
    let offset = s.video_offset;
    let champ = s.player.as_ref().and_then(|p| p.character_id.clone()).unwrap_or_default();
    let kfs = game_keyframes(v, offset);
    if kfs.len() < 5 {
        keep_live(s, "skipped", "the recording is too short".into(), 0.0, serde_json::json!({}), started);
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
            keep_live(s, "skipped", "the ability bar wasn't found in the video (another HUD or game mode)".into(), conf, d, started);
            return Ok(());
        }
    };
    let details_v = details(Some(&fit));
    drop(bands);

    // 2. Full scan: R on every keyframe.
    let rreg = fit.r_region(w, h);
    let at = (rreg.x, rreg.y);
    let mut series: Vec<(f64, RLook)> = Vec::with_capacity(kfs.len());
    for &t in &kfs {
        stop()?;
        if let Some((pt, f)) = v.frame_at(t, rreg)? {
            series.push((pt, hud::look(&f, at, &fit)));
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
        let mut tr = CastTracker::default();
        let mut found: Vec<f64> = Vec::new();
        v.frames(a, b, rreg, &mut |pt, f| {
            if tr.push(hud::look(f, at, &fit)) && !found.iter().any(|t| pt - t < 1.0) {
                found.push(pt);
            }
            true
        })?;
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
    let outcomes = match_up(&cast_times, &presses, delay, rules.skips_cooldown(&champ));
    s.events.retain(|e| !is_ult(e.kind));
    let (mut confirmed, mut video_only, mut unconfirmed) = (0, 0, 0);
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
                s.events.push(
                    GameEvent::new(format!("ultp-{:.2}", m.game_time), EventKind::UltUnconfirmed, m.game_time, "Ult pressed, no cast")
                        .with_details(format!("{} pressed. {}", m.key, reason_text(m.reason.as_deref()))),
                );
            }
        }
    }
    s.events.sort_by(|a, b| a.game_time.partial_cmp(&b.game_time).unwrap_or(std::cmp::Ordering::Equal));
    let mut details = details_v;
    details["keyframes"] = serde_json::json!(series.len());
    details["casts_from_scan"] = serde_json::json!(casts.iter().filter(|c| c.from_scan).count());
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

    #[test]
    fn reasons() {
        assert_eq!(reason_text(Some("cooldown (12 s left)")), "The ult was on cooldown.");
        assert_eq!(reason_text(None), "No cast in the recording (cancelled, no target, or not castable then).");
    }
}
