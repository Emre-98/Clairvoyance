//! The own player's gold, read every poll (`/activeplayer`, ~1 s): it tells when the shop was
//! used (a drop = a purchase, a jump = a sale / undo or a reward; the player list is then read
//! at once, see [`crate::items`]) and how much gold an event gave you ("≈ +300"): the gold just
//! after the event minus the gold just before it, without the passive income.

use cv_core::{EventKind, GameEvent};
use std::collections::VecDeque;

/// Gold changes beyond the passive income smaller than this are noise.
const JUMP: f64 = 40.0;
/// The gold is read this long after an event before its gain is known.
const SETTLE: f64 = 1.5;

#[derive(Debug, Default)]
pub struct GoldLog {
    /// (game time, gold), oldest first; the last ~2 minutes.
    samples: VecDeque<(f64, f64)>,
    /// Game time of the last drop (a purchase).
    pub spent_at: Option<f64>,
    /// Events waiting for the gold read after them.
    waiting: Vec<GameEvent>,
}

/// Kinds whose gold is shown: what the player earned gold for.
pub fn earns_gold(k: EventKind) -> bool {
    matches!(k, EventKind::Kill | EventKind::Assist | EventKind::Tower | EventKind::Inhibitor | EventKind::Dragon | EventKind::Herald | EventKind::Baron | EventKind::Objective)
}

impl GoldLog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Passive income per second: the median of recent small steps (default 2).
    fn rate(&self) -> f64 {
        let mut r: Vec<f64> = self
            .samples
            .iter()
            .zip(self.samples.iter().skip(1))
            .filter(|(a, b)| b.0 - a.0 > 0.3)
            .map(|(a, b)| (b.1 - a.1) / (b.0 - a.0))
            .filter(|r| (0.0..=6.0).contains(r))
            .collect();
        if r.len() < 5 {
            return 2.0;
        }
        r.sort_by(|a, b| a.partial_cmp(b).unwrap());
        r[r.len() / 2]
    }

    /// A new reading. Returns true if the gold jumped (up or down) beyond the passive income:
    /// the shop was probably used, so the inventory should be read now.
    pub fn push(&mut self, gt: f64, gold: f64) -> bool {
        let jump = match self.samples.back() {
            Some(&(t0, g0)) if gt > t0 => {
                let d = gold - g0 - self.rate() * (gt - t0);
                if d < -JUMP {
                    self.spent_at = Some(gt);
                }
                d.abs() > JUMP
            }
            Some(&(t0, _)) if gt <= t0 => return false,
            _ => false,
        };
        self.samples.push_back((gt, gold));
        while self.samples.front().is_some_and(|(t, _)| gt - t > 120.0) {
            self.samples.pop_front();
        }
        jump
    }

    /// Keeps a copy of a new event to add its gold to once it's known.
    pub fn watch(&mut self, e: &GameEvent) {
        if earns_gold(e.kind) {
            self.waiting.push(e.clone());
        }
    }

    /// Events whose gold is known now, with a "Gold" fact added (send them as updates). Events
    /// whose gain can't be told (the shop was used meanwhile, two rewards at once, no reading
    /// before them) are dropped without one.
    pub fn settled(&mut self) -> Vec<GameEvent> {
        let Some(&(now, _)) = self.samples.back() else { return Vec::new() };
        let rate = self.rate();
        let (ready, still): (Vec<GameEvent>, Vec<GameEvent>) = std::mem::take(&mut self.waiting).into_iter().partition(|e| now >= e.game_time + SETTLE);
        self.waiting = still;
        let mut out = Vec::new();
        for e in &ready {
            let te = e.game_time;
            // Another reward within 3 s: the gain can't be split between them.
            if ready.iter().chain(self.waiting.iter()).any(|o| o.id != e.id && (o.game_time - te).abs() < 3.0) {
                continue;
            }
            let before = self.samples.iter().rev().find(|(t, _)| *t < te - 0.05).copied();
            let after = self.samples.iter().find(|(t, _)| *t >= te + SETTLE).copied();
            let (Some(b), Some(a)) = (before, after) else { continue };
            if a.0 - b.0 > 6.0 {
                continue;
            }
            let spent = self.samples.iter().zip(self.samples.iter().skip(1)).any(|(x, y)| x.0 >= b.0 && y.0 <= a.0 && y.1 - x.1 < -JUMP / 2.0);
            if spent {
                continue;
            }
            let gain = a.1 - b.1 - rate * (a.0 - b.0);
            if gain < 15.0 {
                continue;
            }
            let rounded = ((gain / 5.0).round() * 5.0) as i64;
            let mut u = e.clone();
            u.facts.retain(|(k, _)| k != "Your gold");
            out.push(u.fact("Your gold", format!("≈ +{rounded}")));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tower(t: f64) -> GameEvent {
        GameEvent::new(format!("lol-{t}"), EventKind::Tower, t, "Destroyed a tower")
    }

    #[test]
    fn purchase_and_reward() {
        let mut g = GoldLog::new();
        let mut gold = 500.0;
        for i in 0..20 {
            gold += 2.1;
            assert!(!g.push(100.0 + i as f64, gold), "passive income is no jump");
        }
        // Purchase at 120.
        gold -= 1100.0;
        assert!(g.push(120.0, gold));
        assert_eq!(g.spent_at, Some(120.0));
        // A tower at 125.4 gives 250.
        for t in [121.0, 122.0, 123.0, 124.0, 125.0] {
            gold += 2.1;
            g.push(t, gold);
        }
        g.watch(&tower(125.4));
        gold += 2.1 + 250.0;
        assert!(g.push(126.0, gold), "a reward jumps too");
        assert!(g.settled().is_empty(), "not settled yet");
        gold += 2.1;
        g.push(127.0, gold);
        let s = g.settled();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].facts, vec![("Your gold".to_string(), "≈ +250".to_string())]);
        assert!(g.settled().is_empty(), "once");
    }

    #[test]
    fn no_gold_shown_when_unclear() {
        let mut g = GoldLog::new();
        let mut gold = 1000.0;
        for i in 0..10 {
            gold += 2.0;
            g.push(i as f64, gold);
        }
        // Two rewards a second apart: can't be split.
        g.watch(&tower(10.2));
        g.watch(&GameEvent::new("k", EventKind::Kill, 11.0, "Killed Zed"));
        gold += 600.0;
        g.push(11.0, gold);
        g.push(13.0, gold + 4.0);
        assert!(g.settled().is_empty());
        // A reward while shopping: unclear.
        g.watch(&tower(20.5));
        g.push(20.0, gold + 20.0);
        g.push(21.0, gold - 800.0);
        g.push(22.5, gold - 500.0);
        assert!(g.settled().is_empty());
        // Deaths earn nothing (not watched).
        g.watch(&GameEvent::new("d", EventKind::Death, 30.0, "Killed by Zed"));
        g.push(32.0, gold);
        assert!(g.settled().is_empty());
    }
}
