//! League of Legends support through Riot's official Live Client Data API
//! (`https://127.0.0.1:2999/liveclientdata/`), which exists only while you're in a match.
//! No memory reading or injection, so it's safe with Vanguard.
//!
//! "Ult pressed" can't come from the API (it doesn't expose ability casts), so the
//! engine forwards key presses made while the game window is focused to [`LeagueIntegration::on_key`].

pub mod events;

use async_trait::async_trait;
use events::{translate, Ctx, EventList};
use cv_core::game::{
    CaptureTarget, ConfigField, GameIntegration, GameResult, KeyPress, MatchPhase, PlayerInfo, PlayerStats, PollUpdate,
};
use cv_core::{EventKind, GameEvent};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

pub const DEFAULT_API: &str = "https://127.0.0.1:2999/liveclientdata";

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct GameStats {
    #[serde(default)]
    game_mode: String,
    #[serde(default)]
    game_time: f64,
}

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase", default)]
struct Scores {
    kills: u32,
    deaths: u32,
    assists: u32,
    creep_score: u32,
    ward_score: f64,
}

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(default)]
struct Item {
    price: u32,
    count: u32,
}

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase", default)]
struct Player {
    riot_id: String,
    riot_id_game_name: String,
    summoner_name: String,
    champion_name: String,
    raw_champion_name: String,
    team: String,
    level: u32,
    scores: Scores,
    #[serde(deserialize_with = "seq_or_map")]
    items: Vec<Item>,
}

/// Riot's `items` has been a list, but some client versions send an object keyed by slot.
/// Accept both (and anything else as "no items") so one field can't break the whole player list.
fn seq_or_map<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<Item>, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    let vals: Vec<serde_json::Value> = match v {
        serde_json::Value::Array(a) => a,
        serde_json::Value::Object(m) => m.into_iter().map(|(_, v)| v).collect(),
        _ => Vec::new(),
    };
    Ok(vals.into_iter().filter_map(|v| serde_json::from_value(v).ok()).collect())
}

impl Player {
    fn names(&self) -> Vec<String> {
        let mut v = Vec::new();
        for n in [&self.riot_id, &self.riot_id_game_name, &self.summoner_name] {
            let l = n.trim().to_lowercase();
            if !l.is_empty() && !v.contains(&l) {
                v.push(l.clone());
            }
            if let Some((game_name, _)) = l.split_once('#') {
                if !v.iter().any(|x| x == game_name) {
                    v.push(game_name.to_string());
                }
            }
        }
        v
    }
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct ActivePlayer {
    current_gold: f64,
}

pub struct LeagueIntegration {
    client: reqwest::Client,
    base: String,
    riot_id: String,
    ult_key: String,
    seen: HashSet<i64>,
    ctx: Ctx,
    players_checked: Option<Instant>,
    me_found: bool,
    player: Option<PlayerInfo>,
    stats: Option<PlayerStats>,
    mode: Option<String>,
    result: Option<GameResult>,
    in_progress: bool,
    chat_open: bool,
    last_ult: Option<f64>,
}

impl Default for LeagueIntegration {
    fn default() -> Self {
        Self::new()
    }
}

impl LeagueIntegration {
    pub fn new() -> Self {
        // The API uses Riot's self-signed certificate. This client only ever talks to
        // 127.0.0.1:2999, so accepting that certificate is limited to localhost.
        let client = reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .no_proxy()
            .timeout(Duration::from_millis(1500))
            .connect_timeout(Duration::from_millis(500))
            .pool_max_idle_per_host(2)
            .build()
            .expect("http client");
        let base = std::env::var("GR_LEAGUE_API").unwrap_or_else(|_| DEFAULT_API.to_string());
        Self {
            client,
            base,
            riot_id: String::new(),
            ult_key: "R".into(),
            seen: HashSet::new(),
            ctx: Ctx::default(),
            players_checked: None,
            me_found: false,
            player: None,
            stats: None,
            mode: None,
            result: None,
            in_progress: false,
            chat_open: false,
            last_ult: None,
        }
    }

    async fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> anyhow::Result<T> {
        let url = format!("{}/{}", self.base, path);
        let resp = self.client.get(&url).send().await?;
        if !resp.status().is_success() {
            anyhow::bail!("{path}: HTTP {}", resp.status());
        }
        Ok(resp.json::<T>().await?)
    }

    fn reset_match(&mut self) {
        self.seen.clear();
        self.ctx = Ctx::default();
        self.players_checked = None;
        self.me_found = false;
        self.player = None;
        self.stats = None;
        self.mode = None;
        self.result = None;
        self.in_progress = false;
        self.chat_open = false;
        self.last_ult = None;
    }

    /// Reads who we are and the scoreboard (every ~10 s, or until we've found ourselves).
    async fn refresh_players(&mut self) {
        let active_name: Option<String> = self.get::<String>("activeplayername").await.ok();
        let players: Vec<Player> = match self.get("playerlist").await {
            Ok(p) => p,
            Err(e) => {
                log::debug!("playerlist: {e:#}");
                return;
            }
        };
        let gold = self.get::<ActivePlayer>("activeplayer").await.ok().map(|a| a.current_gold);
        self.players_checked = Some(Instant::now());

        let mut me: Vec<String> = Vec::new();
        for n in [active_name.clone().unwrap_or_default(), self.riot_id.clone()] {
            let l = n.trim().to_lowercase();
            if l.is_empty() {
                continue;
            }
            if let Some((g, _)) = l.split_once('#') {
                me.push(g.to_string());
            }
            me.push(l);
        }
        let mut champions = HashMap::new();
        for p in &players {
            for n in p.names() {
                champions.insert(n, p.champion_name.clone());
            }
        }
        let mine = players.iter().find(|p| p.names().iter().any(|n| me.contains(n)));
        if let Some(p) = mine {
            // Also match on every name form the API uses for us.
            for n in p.names() {
                if !me.contains(&n) {
                    me.push(n);
                }
            }
            let items: u32 = p.items.iter().map(|i| i.price * i.count.max(1)).sum();
            let character_id = p.raw_champion_name.rsplit('_').next().filter(|s| !s.is_empty()).map(str::to_string);
            self.player = Some(PlayerInfo {
                name: if p.riot_id.is_empty() { p.summoner_name.clone() } else { p.riot_id.clone() },
                character: Some(p.champion_name.clone()),
                character_id,
                team: Some(p.team.clone()),
                mode: self.mode.clone(),
            });
            self.stats = Some(PlayerStats {
                kills: p.scores.kills,
                deaths: p.scores.deaths,
                assists: p.scores.assists,
                cs: Some(p.scores.creep_score),
                gold: gold.map(|g| items + g.max(0.0) as u32),
                level: Some(p.level),
                vision_score: Some(p.scores.ward_score),
                extra: Vec::new(),
            });
            self.ctx.team = Some(p.team.clone());
            self.me_found = true;
        }
        self.ctx.me = me;
        self.ctx.champions = champions;
    }
}

#[async_trait]
impl GameIntegration for LeagueIntegration {
    fn id(&self) -> &'static str {
        "league"
    }
    fn name(&self) -> &'static str {
        "League of Legends"
    }
    fn short_name(&self) -> &'static str {
        "League"
    }
    fn process_names(&self) -> &'static [&'static str] {
        &["league of legends.exe"]
    }
    fn capture(&self) -> CaptureTarget {
        CaptureTarget {
            exe: "League of Legends.exe".into(),
            display_capture_only: false,
        }
    }
    fn supports_events(&self) -> bool {
        true
    }

    fn config_fields(&self) -> Vec<ConfigField> {
        vec![
            ConfigField {
                key: "riot_id",
                label: "Riot ID",
                kind: "text",
                help: "Your Riot ID, e.g. Name#EUW. Used as a fallback; the app normally detects you automatically.",
            },
            ConfigField {
                key: "ult_key",
                label: "Ult key",
                kind: "key",
                help: "The key you cast your ultimate with. Presses are marked as \"Ult pressed\" (the game can't confirm the cast).",
            },
        ]
    }
    fn default_config(&self) -> serde_json::Value {
        serde_json::json!({ "riot_id": "", "ult_key": "R" })
    }
    fn configure(&mut self, config: &serde_json::Value) {
        self.riot_id = config["riot_id"].as_str().unwrap_or("").trim().to_string();
        self.ult_key = cv_core::settings::normalize_key(config["ult_key"].as_str().unwrap_or("R"));
        if self.ult_key.is_empty() {
            self.ult_key = "R".into();
        }
        // Hidden option used by the built-in game simulator.
        self.base = match config["api_base"].as_str().filter(|s| !s.is_empty()) {
            Some(b) => b.trim_end_matches('/').to_string(),
            None => std::env::var("GR_LEAGUE_API").unwrap_or_else(|_| DEFAULT_API.to_string()),
        };
    }

    async fn start(&mut self) -> anyhow::Result<()> {
        self.reset_match();
        Ok(())
    }

    async fn stop(&mut self) {
        self.reset_match();
    }

    async fn poll(&mut self) -> anyhow::Result<PollUpdate> {
        let mut u = PollUpdate::default();
        let stats: GameStats = match self.get("gamestats").await {
            Ok(s) => s,
            Err(e) => {
                // No match data: client, loading screen, or the game just closed.
                u.phase = if self.result.is_some() { MatchPhase::Ended } else { MatchPhase::Waiting };
                u.result = self.result;
                if self.in_progress && self.result.is_none() {
                    return Err(e);
                }
                return Ok(u);
            }
        };
        if self.mode.is_none() && !stats.game_mode.is_empty() {
            self.mode = Some(events::mode_name(&stats.game_mode));
        }
        u.game_time = Some(stats.game_time);
        u.phase = if stats.game_time > 0.0 { MatchPhase::InProgress } else { MatchPhase::Loading };
        if u.phase == MatchPhase::InProgress {
            self.in_progress = true;
        }

        let refresh = match self.players_checked {
            None => true,
            Some(t) => !self.me_found || t.elapsed() >= Duration::from_secs(10),
        };
        if refresh {
            self.refresh_players().await;
        }

        // Handle events only once we know who we are (or tried to), so no kill gets lost.
        if self.players_checked.is_some() {
            if let Ok(list) = self.get::<EventList>("eventdata").await {
                for raw in &list.events {
                    // The API returns ALL events every time: never handle one twice.
                    if !self.seen.insert(raw.event_id) {
                        continue;
                    }
                    if raw.event_name == "GameEnd" {
                        self.result = match raw.result.as_deref() {
                            Some("Win") => Some(GameResult::Win),
                            Some("Lose") => Some(GameResult::Loss),
                            _ => self.result,
                        };
                        // Final scoreboard.
                        self.refresh_players().await;
                    }
                    u.events.extend(translate(raw, &self.ctx));
                }
            }
        }
        if self.result.is_some() {
            u.phase = MatchPhase::Ended;
        }
        u.result = self.result;
        u.player = self.player.clone().map(|mut p| {
            p.mode = self.mode.clone();
            p
        });
        u.stats = self.stats.clone();
        Ok(u)
    }

    fn on_key(&mut self, key: &KeyPress, game_time: f64) -> Option<GameEvent> {
        // Rough chat detection: Enter opens/sends chat, Escape closes it.
        match key.key.as_str() {
            "Enter" => {
                self.chat_open = !self.chat_open;
                return None;
            }
            "Escape" => {
                self.chat_open = false;
                return None;
            }
            _ => {}
        }
        // Ctrl+R levels up the ult instead of casting it.
        if self.chat_open || key.ctrl || !key.key.eq_ignore_ascii_case(&self.ult_key) {
            return None;
        }
        if self.last_ult.is_some_and(|t| (game_time - t).abs() < 0.75) {
            return None;
        }
        self.last_ult = Some(game_time);
        Some(
            GameEvent::new(format!("ult-{:.2}", game_time), EventKind::UltPressed, game_time, "Ult pressed")
                .with_details("Key press. The game can't confirm the ult was cast."),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kp(k: &str, ctrl: bool) -> KeyPress {
        KeyPress { key: k.into(), ctrl, shift: false, alt: false }
    }

    #[test]
    fn ult_detection() {
        let mut l = LeagueIntegration::new();
        l.configure(&serde_json::json!({"ult_key": "r"}));
        assert!(l.on_key(&kp("R", false), 100.0).is_some());
        assert!(l.on_key(&kp("R", false), 100.3).is_none(), "debounced");
        assert!(l.on_key(&kp("R", true), 110.0).is_none(), "ctrl+R levels up");
        assert!(l.on_key(&kp("Enter", false), 120.0).is_none());
        assert!(l.on_key(&kp("R", false), 121.0).is_none(), "typing in chat");
        assert!(l.on_key(&kp("Enter", false), 122.0).is_none());
        assert!(l.on_key(&kp("R", false), 123.0).is_some());
        assert!(l.on_key(&kp("Q", false), 130.0).is_none());
    }

    #[test]
    fn player_names() {
        let p = Player { riot_id: "Faker#KR1".into(), riot_id_game_name: "Faker".into(), ..Default::default() };
        assert_eq!(p.names(), vec!["faker#kr1".to_string(), "faker".to_string()]);
    }
}

#[cfg(test)]
mod items_tests {
    use super::*;

    #[test]
    fn items_as_list_or_object() {
        let list: Player = serde_json::from_str(r#"{"championName":"Ahri","items":[{"price":300,"count":1},{"price":50,"count":2}]}"#).unwrap();
        assert_eq!(list.items.len(), 2);
        let map: Player = serde_json::from_str(r#"{"championName":"Ahri","items":{"0":{"price":300,"count":1},"6":{"price":0,"count":1}}}"#).unwrap();
        assert_eq!(map.items.len(), 2);
        let odd: Player = serde_json::from_str(r#"{"championName":"Ahri","items":null}"#).unwrap();
        assert!(odd.items.is_empty());
    }
}
