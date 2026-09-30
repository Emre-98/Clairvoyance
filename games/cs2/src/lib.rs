//! Counter-Strike 2 support through Valve's official Game State Integration (GSI).
//!
//! CS2 reads `gamestate_integration_*.cfg` files from its cfg folder at launch and then
//! POSTs its state (map, round, the local player's stats) to a local HTTP endpoint.
//! No memory reading, no injection. The module installs the cfg file itself.
//!
//! Recording: CS2 is captured as the whole monitor (its exclusive-fullscreen window can come
//! out black with window capture).

mod gsi;

use async_trait::async_trait;
use cv_core::game::{CaptureTarget, ConfigField, GameIntegration, GameResult, MatchPhase, PlayerInfo, PlayerStats, PollUpdate};
use cv_core::{EventKind, GameEvent};
use serde_json::Value;
use std::time::{Duration, Instant};

pub use gsi::{cfg_contents, find_cfg_dirs, CFG_NAME};

pub const DEFAULT_PORT: u16 = 3380;
pub const TOKEN: &str = "clairvoyance-gsi";

/// What we remember between two GSI snapshots to turn counters into events.
#[derive(Debug, Default, Clone)]
pub struct Tracker {
    pub kills: Option<i64>,
    pub deaths: Option<i64>,
    pub assists: Option<i64>,
    pub round_phase: String,
    pub bomb: String,
    pub multikill_round: Option<(i64, i64)>,
    pub team: Option<String>,
    pub seq: u64,
}

fn s<'a>(v: &'a Value, path: &[&str]) -> &'a Value {
    path.iter().fold(v, |acc, k| &acc[*k])
}

/// Is the `player` block us (and not the player we're spectating after dying)?
pub fn is_local(v: &Value) -> bool {
    let me = s(v, &["provider", "steamid"]).as_str();
    let p = s(v, &["player", "steamid"]).as_str();
    me.is_some() && me == p
}

pub fn map_phase(v: &Value) -> MatchPhase {
    match s(v, &["map", "phase"]).as_str() {
        Some("warmup") => MatchPhase::Loading,
        Some("live") | Some("intermission") => MatchPhase::InProgress,
        Some("gameover") => MatchPhase::Ended,
        _ => MatchPhase::Waiting,
    }
}

/// Events between the previous snapshot (in `t`) and `v`. `now` = timeline time.
pub fn diff(t: &mut Tracker, v: &Value, now: f64) -> Vec<GameEvent> {
    let mut out = Vec::new();
    let id = |t: &mut Tracker, what: &str| {
        t.seq += 1;
        format!("cs2-{}-{what}", t.seq)
    };
    if is_local(v) {
        let st = s(v, &["player", "match_stats"]);
        let (k, d, a) = (st["kills"].as_i64(), st["deaths"].as_i64(), st["assists"].as_i64());
        if let Some(team) = s(v, &["player", "team"]).as_str() {
            t.team = Some(team.to_string());
        }
        if let (Some(prev), Some(cur)) = (t.kills, k) {
            for _ in prev..cur.max(prev) {
                let i = id(t, "k");
                out.push(GameEvent::new(i, EventKind::Kill, now, "Kill"));
            }
        }
        if let (Some(prev), Some(cur)) = (t.deaths, d) {
            if cur > prev {
                let i = id(t, "d");
                out.push(GameEvent::new(i, EventKind::Death, now, "Died"));
            }
        }
        if let (Some(prev), Some(cur)) = (t.assists, a) {
            for _ in prev..cur.max(prev) {
                let i = id(t, "a");
                out.push(GameEvent::new(i, EventKind::Assist, now, "Assist"));
            }
        }
        t.kills = k.or(t.kills);
        t.deaths = d.or(t.deaths);
        t.assists = a.or(t.assists);

        // 3K / 4K / Ace within one round.
        let rk = s(v, &["player", "state", "round_kills"]).as_i64().unwrap_or(0);
        let round = s(v, &["map", "round"]).as_i64().unwrap_or(-1);
        if rk >= 3 && t.multikill_round.is_none_or(|(r, n)| r != round || n < rk) {
            t.multikill_round = Some((round, rk));
            let (kind, title) = match rk {
                3 => (EventKind::Multikill, "3K".to_string()),
                4 => (EventKind::Multikill, "4K".to_string()),
                _ => (EventKind::Ace, "Ace (5K)".to_string()),
            };
            let i = id(t, "mk");
            out.push(GameEvent::new(i, kind, now, title));
        }
    }

    let phase = s(v, &["round", "phase"]).as_str().unwrap_or("").to_string();
    if phase == "over" && t.round_phase != "over" && !t.round_phase.is_empty() {
        // map.round is 0-based while the round is being played.
        let n = s(v, &["map", "round"]).as_i64().unwrap_or(0) + 1;
        let win = s(v, &["round", "win_team"]).as_str();
        let title = match (win, t.team.as_deref()) {
            (Some(w), Some(me)) if w == me => format!("Round {n} won"),
            (Some(_), Some(_)) => format!("Round {n} lost"),
            _ => format!("Round {n} over"),
        };
        let i = id(t, "r");
        out.push(GameEvent::new(i, EventKind::Round, now, title));
    }
    if !phase.is_empty() {
        t.round_phase = phase;
    }

    let bomb = s(v, &["round", "bomb"]).as_str().unwrap_or("").to_string();
    if bomb != t.bomb {
        let title = match bomb.as_str() {
            "planted" => Some("Bomb planted"),
            "defused" => Some("Bomb defused"),
            "exploded" => Some("Bomb exploded"),
            _ => None,
        };
        if let Some(title) = title {
            let i = id(t, "b");
            out.push(GameEvent::new(i, EventKind::Objective, now, title));
        }
        t.bomb = bomb;
    }
    out
}

pub fn result(v: &Value, team: Option<&str>) -> Option<GameResult> {
    let ct = s(v, &["map", "team_ct", "score"]).as_i64()?;
    let tt = s(v, &["map", "team_t", "score"]).as_i64()?;
    let (mine, theirs) = match team? {
        "CT" => (ct, tt),
        "T" => (tt, ct),
        _ => return None,
    };
    Some(if mine > theirs {
        GameResult::Win
    } else if mine < theirs {
        GameResult::Loss
    } else {
        GameResult::Draw
    })
}

fn map_title(name: &str) -> String {
    let base = name.rsplit('/').next().unwrap_or(name);
    let base = base.split_once('_').map(|(_, m)| m).unwrap_or(base);
    let mut c = base.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

pub struct Cs2Integration {
    port: u16,
    cfg_folder: String,
    server: Option<gsi::Server>,
    tracker: Tracker,
    t0: Instant,
    ended: Option<GameResult>,
}

impl Default for Cs2Integration {
    fn default() -> Self {
        Self::new()
    }
}

impl Cs2Integration {
    pub fn new() -> Self {
        Self { port: DEFAULT_PORT, cfg_folder: String::new(), server: None, tracker: Tracker::default(), t0: Instant::now(), ended: None }
    }

    fn ensure_server(&mut self) -> anyhow::Result<&gsi::Server> {
        if self.server.as_ref().is_some_and(|s| s.port != self.port) {
            self.server = None;
        }
        if self.server.is_none() {
            self.server = Some(gsi::Server::start(self.port, TOKEN)?);
        }
        Ok(self.server.as_ref().unwrap())
    }
}

#[async_trait]
impl GameIntegration for Cs2Integration {
    fn id(&self) -> &'static str {
        "cs2"
    }
    fn name(&self) -> &'static str {
        "Counter-Strike 2"
    }
    fn short_name(&self) -> &'static str {
        "CS2"
    }
    fn process_names(&self) -> &'static [&'static str] {
        &["cs2.exe"]
    }
    fn capture(&self) -> CaptureTarget {
        CaptureTarget { exe: "cs2.exe".into(), display_capture_only: true }
    }
    fn supports_events(&self) -> bool {
        true
    }
    fn match_only(&self) -> bool {
        true
    }
    fn end_grace(&self) -> Duration {
        Duration::from_secs(8)
    }

    fn config_fields(&self) -> Vec<ConfigField> {
        vec![
            ConfigField {
                key: "cfg_folder",
                label: "CS2 cfg folder",
                kind: "text",
                help: "Found automatically through Steam. Clairvoyance puts its Game State Integration file there; restart CS2 once after the first install.",
            },
            ConfigField { key: "gsi_port", label: "Local port", kind: "text", help: "Port CS2 sends its game state to (default 3380)." },
        ]
    }
    fn default_config(&self) -> serde_json::Value {
        serde_json::json!({ "cfg_folder": "", "gsi_port": DEFAULT_PORT.to_string() })
    }
    fn configure(&mut self, config: &serde_json::Value) {
        self.port = config["gsi_port"].as_str().and_then(|p| p.trim().parse().ok()).or(config["gsi_port"].as_u64().map(|p| p as u16)).unwrap_or(DEFAULT_PORT);
        self.cfg_folder = config["cfg_folder"].as_str().unwrap_or("").trim().to_string();
        // Install/refresh the GSI cfg so CS2 knows where to send its state.
        let dirs = if self.cfg_folder.is_empty() { gsi::find_cfg_dirs() } else { vec![self.cfg_folder.clone().into()] };
        for d in dirs {
            if let Err(e) = gsi::install_cfg(&d, self.port, TOKEN) {
                log::warn!("CS2 GSI cfg in {}: {e}", d.display());
            }
        }
    }

    async fn start(&mut self) -> anyhow::Result<()> {
        self.ensure_server()?;
        self.tracker = Tracker::default();
        self.t0 = Instant::now();
        self.ended = None;
        Ok(())
    }

    async fn stop(&mut self) {
        // Keep the tiny HTTP server running (CS2 keeps posting while it's open).
        self.tracker = Tracker::default();
        self.ended = None;
    }

    async fn poll(&mut self) -> anyhow::Result<PollUpdate> {
        let server = self.ensure_server()?;
        let mut u = PollUpdate::default();
        let Some((v, age)) = server.latest() else { return Ok(u) };
        if age > Duration::from_secs(35) {
            return Ok(u); // CS2 stopped talking (closed or crashed).
        }
        u.phase = map_phase(&v);
        let now = self.t0.elapsed().as_secs_f64();
        u.game_time = Some(now);
        if u.phase == MatchPhase::Waiting {
            return Ok(u);
        }
        u.events = diff(&mut self.tracker, &v, now);
        if u.phase == MatchPhase::Ended && self.ended.is_none() {
            self.ended = result(&v, self.tracker.team.as_deref());
            u.events.push(GameEvent::new("cs2-end", EventKind::GameEnd, now, match self.ended {
                Some(GameResult::Win) => "Victory",
                Some(GameResult::Loss) => "Defeat",
                Some(GameResult::Draw) => "Draw",
                None => "Match over",
            }));
        }
        u.result = self.ended;
        let map = s(&v, &["map", "name"]).as_str().unwrap_or("");
        u.player = Some(PlayerInfo {
            name: s(&v, &["player", "name"]).as_str().unwrap_or("").to_string(),
            character: (!map.is_empty()).then(|| map_title(map)),
            character_id: None,
            team: self.tracker.team.clone(),
            mode: s(&v, &["map", "mode"]).as_str().map(|m| {
                let mut c = m.chars();
                c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
            }),
        });
        if let (Some(k), Some(d), Some(a)) = (self.tracker.kills, self.tracker.deaths, self.tracker.assists) {
            let ms = s(&v, &["player", "match_stats"]);
            let mut extra = Vec::new();
            if is_local(&v) {
                if let Some(m) = ms["mvps"].as_i64() {
                    extra.push(("MVPs".into(), m.to_string()));
                }
                if let Some(sc) = ms["score"].as_i64() {
                    extra.push(("Score".into(), sc.to_string()));
                }
            }
            if let (Some(ct), Some(tt)) = (s(&v, &["map", "team_ct", "score"]).as_i64(), s(&v, &["map", "team_t", "score"]).as_i64()) {
                let (mine, theirs) = if self.tracker.team.as_deref() == Some("T") { (tt, ct) } else { (ct, tt) };
                extra.push(("Rounds".into(), format!("{mine} – {theirs}")));
            }
            u.stats = Some(PlayerStats { kills: k as u32, deaths: d as u32, assists: a as u32, extra, ..Default::default() });
        }
        Ok(u)
    }
}

#[cfg(test)]
mod tests;
