//! The time-synced scoreboard: every player's champion, level, KDA, CS, items and summoner
//! spells over the game, saved with the session as the first full state plus only what changed
//! at each later read (a 35 min League game: ~200 reads, a few KB to a few tens of KB).
//!
//! Game-agnostic: the game module reports full [`PlayerState`]s at a game time
//! ([`crate::game::PollUpdate::scoreboard`]); [`Scoreboard::record`] keeps the changes.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Who is in the game (fixed for the match).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SbPlayer {
    pub name: String,
    /// Display name ("Wukong").
    pub character: String,
    /// Image id ("MonkeyKing").
    #[serde(default)]
    pub character_id: String,
    /// "ORDER" / "CHAOS" (League), or the game's team name.
    pub team: String,
    /// The recording's own player.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub me: bool,
}

/// One player's numbers at one moment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PlayerState {
    pub level: u32,
    pub kills: u32,
    pub deaths: u32,
    pub assists: u32,
    pub cs: u32,
    /// Item ids by inventory slot (0 = empty; League: 6 slots + trinket).
    pub items: Vec<u32>,
    /// Summoner spell ids (League: "SummonerFlash"), D then F.
    pub spells: Vec<String>,
}

/// What changed for one player (absent = unchanged). Short names: this is most of the file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SbDelta {
    /// Player index in [`Scoreboard::players`].
    pub i: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lv: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub k: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub d: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub a: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cs: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub it: Option<Vec<u32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sp: Option<Vec<String>>,
}

impl SbDelta {
    fn between(i: u8, old: &PlayerState, new: &PlayerState) -> Option<SbDelta> {
        let ch = |a: u32, b: u32| (a != b).then_some(b);
        let d = SbDelta {
            i,
            lv: ch(old.level, new.level),
            k: ch(old.kills, new.kills),
            d: ch(old.deaths, new.deaths),
            a: ch(old.assists, new.assists),
            cs: ch(old.cs, new.cs),
            it: (old.items != new.items).then(|| new.items.clone()),
            sp: (old.spells != new.spells).then(|| new.spells.clone()),
        };
        (d != SbDelta { i, ..Default::default() }).then_some(d)
    }

    fn apply(&self, s: &mut PlayerState) {
        if let Some(v) = self.lv {
            s.level = v;
        }
        if let Some(v) = self.k {
            s.kills = v;
        }
        if let Some(v) = self.d {
            s.deaths = v;
        }
        if let Some(v) = self.a {
            s.assists = v;
        }
        if let Some(v) = self.cs {
            s.cs = v;
        }
        if let Some(v) = &self.it {
            s.items = v.clone();
        }
        if let Some(v) = &self.sp {
            s.spells = v.clone();
        }
    }
}

/// The changes found at one read (game seconds).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SbFrame {
    pub t: f64,
    pub d: Vec<SbDelta>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Scoreboard {
    /// Version of the game data the item / spell ids belong to (League: Data Dragon "16.20.1").
    #[serde(default)]
    pub version: String,
    pub players: Vec<SbPlayer>,
    /// Display names of the item / spell ids that appear (for hover text).
    #[serde(default)]
    pub names: BTreeMap<String, String>,
    /// The first frame holds everyone's full state; later frames only changes.
    pub frames: Vec<SbFrame>,
}

/// One read reported by the game module.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ScoreboardRead {
    pub t: f64,
    pub version: String,
    pub players: Vec<SbPlayer>,
    pub states: Vec<PlayerState>,
    /// Names of ids in `states` (items, spells).
    pub names: BTreeMap<String, String>,
}

impl Scoreboard {
    /// Everyone's state at the last frame.
    pub fn last(&self) -> Vec<PlayerState> {
        self.state_at(f64::INFINITY)
    }

    /// Everyone's state at game time `t` (before the first frame: the first frame).
    pub fn state_at(&self, t: f64) -> Vec<PlayerState> {
        let mut s = vec![PlayerState::default(); self.players.len()];
        for (k, f) in self.frames.iter().enumerate() {
            if k > 0 && f.t > t {
                break;
            }
            for d in &f.d {
                if let Some(p) = s.get_mut(d.i as usize) {
                    d.apply(p);
                }
            }
        }
        s
    }

    /// Adds a read: the changes since the last one (nothing if nothing changed). A different
    /// roster (a player joined late, or another match) starts over from this read.
    pub fn record(&mut self, r: ScoreboardRead) {
        if r.players.is_empty() || r.players.len() != r.states.len() || r.players.len() > 255 {
            return;
        }
        if !r.version.is_empty() {
            self.version = r.version;
        }
        self.names.extend(r.names);
        if self.players != r.players {
            let same_people = self.players.len() == r.players.len() && self.players.iter().zip(&r.players).all(|(a, b)| a.name == b.name && a.character == b.character);
            if !same_people {
                self.players = r.players;
                self.frames.clear();
            } else {
                // Only "me" / ids filled in later: keep the history.
                self.players = r.players;
            }
        }
        if self.frames.last().is_some_and(|f| r.t < f.t) {
            return;
        }
        let empty = PlayerState::default();
        let last = if self.frames.is_empty() { Vec::new() } else { self.last() };
        let d: Vec<SbDelta> = r.states.iter().enumerate().filter_map(|(i, s)| SbDelta::between(i as u8, last.get(i).unwrap_or(&empty), s)).collect();
        if !d.is_empty() || self.frames.is_empty() {
            self.frames.push(SbFrame { t: r.t, d });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn players() -> Vec<SbPlayer> {
        (0..10)
            .map(|i| SbPlayer { name: format!("P{i}"), character: format!("C{i}"), character_id: format!("C{i}"), team: if i < 5 { "ORDER" } else { "CHAOS" }.into(), me: i == 0 })
            .collect()
    }

    fn st(level: u32, cs: u32, items: &[u32]) -> PlayerState {
        PlayerState { level, kills: 0, deaths: 0, assists: 0, cs, items: items.to_vec(), spells: vec!["SummonerFlash".into(), "SummonerDot".into()] }
    }

    #[test]
    fn deltas_and_state_at_any_time() {
        let mut sb = Scoreboard::default();
        let read = |t: f64, states: Vec<PlayerState>| ScoreboardRead { t, version: "16.20.1".into(), players: players(), states, names: BTreeMap::new() };
        sb.record(read(10.0, (0..10).map(|_| st(1, 0, &[1056])).collect()));
        // Nothing changed: no frame.
        sb.record(read(20.0, (0..10).map(|_| st(1, 0, &[1056])).collect()));
        assert_eq!(sb.frames.len(), 1);
        // Player 3 levels up and farms; player 0 buys an item.
        let mut s: Vec<PlayerState> = (0..10).map(|_| st(1, 0, &[1056])).collect();
        s[3] = st(2, 12, &[1056]);
        s[0] = st(1, 0, &[1056, 3031]);
        sb.record(read(30.0, s.clone()));
        assert_eq!(sb.frames.len(), 2);
        assert_eq!(sb.frames[1].d.len(), 2, "only the two who changed: {:?}", sb.frames[1].d);
        let d3 = sb.frames[1].d.iter().find(|d| d.i == 3).unwrap();
        assert_eq!((d3.lv, d3.cs, d3.it.is_none(), d3.k), (Some(2), Some(12), true, None));
        s[0].kills = 1;
        sb.record(read(40.0, s.clone()));
        // States in between.
        assert_eq!(sb.state_at(5.0)[3].level, 1, "before the first read: the first read");
        assert_eq!(sb.state_at(29.9)[3].level, 1);
        assert_eq!(sb.state_at(30.0)[3], st(2, 12, &[1056]));
        assert_eq!(sb.state_at(35.0)[0].items, vec![1056, 3031]);
        assert_eq!(sb.state_at(35.0)[0].kills, 0);
        assert_eq!(sb.last()[0].kills, 1);
        // Small on disk: changes only.
        let json = serde_json::to_string(&sb).unwrap();
        assert!(json.contains(r#"{"i":0,"k":1}"#), "{json}");
        // Reads out of order are ignored; a new roster starts over.
        sb.record(read(1.0, s.clone()));
        assert_eq!(sb.frames.len(), 3);
        let mut other = players();
        other[9].character = "Zed".into();
        sb.record(ScoreboardRead { t: 50.0, version: String::new(), players: other, states: s.clone(), names: BTreeMap::new() });
        assert_eq!(sb.frames.len(), 1);
        assert_eq!(sb.version, "16.20.1", "kept when a read has none");
        let back: Scoreboard = serde_json::from_str(&serde_json::to_string(&sb).unwrap()).unwrap();
        assert_eq!(back, sb);
    }

    #[test]
    fn size_of_a_long_game() {
        // 35 minutes, a read every 10 s, everyone farming, levelling and buying now and then.
        let mut sb = Scoreboard::default();
        for r in 0..210u32 {
            let t = r as f64 * 10.0;
            let states = (0..10u32)
                .map(|p| PlayerState {
                    level: (1 + r / 12).min(18),
                    kills: (r + p) / 30,
                    deaths: (r + 2 * p) / 35,
                    assists: (r + 3 * p) / 25,
                    cs: r * (6 + p % 3) / 6 * 10 / 10,
                    items: (0..((r + p) / 30).min(6)).map(|k| 3000 + k).collect(),
                    spells: vec!["SummonerFlash".into(), "SummonerTeleport".into()],
                })
                .collect();
            sb.record(ScoreboardRead { t, version: "16.20.1".into(), players: players(), states, names: BTreeMap::new() });
        }
        let bytes = serde_json::to_string(&sb).unwrap().len();
        let full_every_read = 210 * serde_json::to_string(&sb.last()).unwrap().len();
        eprintln!("35 min scoreboard: {bytes} bytes (full snapshot every read: {full_every_read})");
        assert!(bytes < 60_000 && bytes * 3 < full_every_read, "{bytes} vs {full_every_read}");
    }
}
