//! League of Legends support through Riot's official Live Client Data API
//! (`https://127.0.0.1:2999/liveclientdata/`), which exists only while you're in a match.
//! No memory reading or injection, so it's safe with Vanguard.
//!
//! The queue (Ranked, Normal, ARAM, …) comes from the League client's local API (LCU), see
//! [`queues`], so per-mode recording rules are decided before recording starts.
//!
//! The API doesn't expose ability casts. Ult casts come in two layers: live, the ult key
//! presses (binds from League's own settings) filtered by rank, cooldown and death ([`ult`]);
//! after the game, a check against the recording's ability bar ([`verify`], [`hud`]).

pub mod actions;
pub mod ddragon;
pub mod events;
pub mod hud;
pub mod queues;
pub mod ult;
pub mod ultkind;
pub mod verify;
pub mod watch;

use async_trait::async_trait;
use cv_core::game::{CaptureTarget, ConfigField, GameIntegration, GameResult, KeyPress, MatchPhase, PlayerInfo, PlayerStats, PollUpdate, SessionCheck, WatchKind};
use cv_core::modes::{CatalogMode, MatchMode, ModeGroupInfo};
use cv_core::{EventKind, GameEvent};
use events::{translate, Ctx, EventList};
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
    is_dead: bool,
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
    abilities: HashMap<String, AbilityInfo>,
    champion_stats: ChampionStats,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct AbilityInfo {
    ability_level: Option<u32>,
    /// e.g. "AnnieR"; some recast ults swap it while the recast is available.
    id: Option<String>,
    display_name: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct ChampionStats {
    ability_haste: Option<f64>,
}

/// Where Data Dragon data and the optional `ult_rules.json` override live
/// (`%LOCALAPPDATA%\Clairvoyance`). Set once by the app at start-up.
static DATA_DIR: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

pub fn set_data_dir(dir: std::path::PathBuf) {
    let _ = DATA_DIR.set(dir);
}

fn data_dir() -> Option<&'static std::path::Path> {
    DATA_DIR.get().map(|p| p.as_path())
}

pub struct LeagueIntegration {
    client: reqwest::Client,
    base: String,
    riot_id: String,
    ult_key: String,
    /// League install folder (only needed if it can't be found automatically).
    client_dir: String,
    seen: HashSet<i64>,
    ctx: Ctx,
    players_checked: Option<Instant>,
    me_found: bool,
    player: Option<PlayerInfo>,
    stats: Option<PlayerStats>,
    mode: Option<String>,
    result: Option<GameResult>,
    in_progress: bool,
    /// Live ult filtering (layer 1).
    ult: ult::UltTracker,
    /// Base ult cooldowns from Data Dragon, filled in the background.
    ult_cd: std::sync::Arc<std::sync::Mutex<Option<Vec<f64>>>>,
    ult_cd_started: bool,
    /// The ult kind Data Dragon's R text suggests (filled in the background with the cooldowns).
    ult_guess: std::sync::Arc<std::sync::Mutex<Option<ultkind::UltKind>>>,
    /// A guessed kind (champion not in the rules file) to save with the game once.
    ult_kind_mark: Option<ultkind::UltKind>,
    patch: Option<String>,
    /// Action keys (abilities, summoners, items, ward) with League's binds for this match.
    actions: Vec<cv_core::input::actions::ActionKey>,
    /// Setting "Record games you spectate" (replays are never recorded).
    record_spectating: bool,
    /// What the League client said at the start (replay / spectating), for the in-game check.
    watch_hint: Option<WatchKind>,
    /// The in-game API has said whether this is spectator mode (checked once per match).
    live_decided: bool,
    live_checks: u32,
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
            client_dir: String::new(),
            seen: HashSet::new(),
            ctx: Ctx::default(),
            players_checked: None,
            me_found: false,
            player: None,
            stats: None,
            mode: None,
            result: None,
            in_progress: false,
            ult: ult::UltTracker::new(ult::default_binds(), None, ult::UltRules::builtin()),
            ult_cd: Default::default(),
            ult_cd_started: false,
            ult_guess: Default::default(),
            ult_kind_mark: None,
            patch: None,
            actions: actions::default_actions(),
            record_spectating: false,
            watch_hint: None,
            live_decided: false,
            live_checks: 0,
        }
    }

    /// Status code and body of an in-game API request, errors included.
    async fn get_raw(&self, path: &str) -> anyhow::Result<(u16, String)> {
        let url = format!("{}/{}", self.base, path);
        let resp = self.client.get(&url).send().await?;
        let status = resp.status().as_u16();
        Ok((status, resp.text().await.unwrap_or_default()))
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
        let manual = self.ult.manual.clone();
        self.ult = ult::UltTracker::new(ult::default_binds(), manual, ult::UltRules::load(data_dir()));
        self.ult_cd = Default::default();
        self.ult_cd_started = false;
        self.ult_guess = Default::default();
        self.ult_kind_mark = None;
        self.patch = None;
        self.actions = actions::default_actions();
        self.watch_hint = None;
        self.live_decided = false;
        self.live_checks = 0;
    }

    /// League's own key binds and patch, read at the start of each match (cheap file reads).
    fn read_league_config(&mut self) {
        let dir = queues::install_dirs(&self.client_dir).into_iter().find(|d| d.join("Config").is_dir());
        let Some(dir) = dir else {
            log::info!("ult keys: League's Config folder not found; using League's default binds");
            return;
        };
        // One read of League's key binds for both the ult tracking and the replay's ability
        // bubbles (at the loading screen, before the game starts).
        if let Some(text) = ult::read_input_settings(&dir) {
            let b = ult::parse_input_ini(&text);
            let keys: Vec<String> = b.cast.iter().map(|b| b.label()).collect();
            log::info!("ult keys from League's settings: {}{}", keys.join(", "), if b.mouse.is_empty() { String::new() } else { format!(" (and mouse: {})", b.mouse.join(", ")) });
            self.ult.binds = b;
            self.actions = actions::parse_actions(&text);
        }
        self.patch = ult::read_game_cfg(&dir).patch;
    }

    /// Facts the live ult filter needs: R rank and ability haste (every poll, small request),
    /// whether we're dead (only polled while dead), and the base cooldowns (once, background).
    async fn update_ult_state(&mut self, game_mode: &str, new_events: &[GameEvent], game_time: f64) {
        if self.ult.game_mode.is_none() && !game_mode.is_empty() {
            self.ult.game_mode = Some(game_mode.to_string());
        }
        if let Ok(a) = self.get::<ActivePlayer>("activeplayer").await {
            let r = a.abilities.get("R").and_then(|r| r.ability_level);
            self.ult.update_active(r, a.champion_stats.ability_haste);
            // The R spell's id + name: logged when it changes (a recast state), used to keep
            // a live ult episode open (see ult::update_r_state).
            let rs = a.abilities.get("R").map(|r| format!("{}|{}", r.id.as_deref().unwrap_or(""), r.display_name.as_deref().unwrap_or("")));
            self.ult.update_r_state(rs.filter(|s| s != "|"), game_time);
        }
        if self.ult.champion.is_none() {
            self.ult.champion = self.player.as_ref().and_then(|p| p.character_id.clone());
        }
        if !self.ult_cd_started {
            if let Some(champ) = self.ult.champion.clone() {
                self.ult_cd_started = true;
                let slot = self.ult_cd.clone();
                let patch = self.patch.clone();
                let guess = self.ult_guess.clone();
                tokio::spawn(async move {
                    let info = ddragon::ult_info(data_dir(), patch.as_deref(), &champ).await;
                    *slot.lock().unwrap() = info.as_ref().map(|i| i.cooldown.clone());
                    *guess.lock().unwrap() = info.and_then(|i| i.kind_guess);
                });
            }
        }
        if self.ult.base_cd.is_none() {
            self.ult.base_cd = self.ult_cd.lock().unwrap().clone();
        }
        if self.ult.guess.is_none() {
            if let Some(g) = *self.ult_guess.lock().unwrap() {
                self.ult.guess = Some(g);
                if let Some(c) = self.ult.champion.as_deref() {
                    if self.ult.rules.kind_rule(c).is_none() && g != ultkind::UltKind::Normal {
                        log::info!("ult: {c} isn't in the rules file; Data Dragon's R text looks like {}", g.as_str());
                        // Saved with the game so the check after it uses the same kind.
                        self.ult_kind_mark = Some(g);
                    }
                }
            }
        }
        if new_events.iter().any(|e| e.kind == EventKind::Death) {
            self.ult.set_dead(true);
        }
        if self.ult.dead {
            if let Ok(players) = self.get::<Vec<Player>>("playerlist").await {
                if let Some(p) = players.iter().find(|p| p.names().iter().any(|n| self.ctx.me.contains(n))) {
                    self.ult.set_dead(p.is_dead);
                }
            }
        }
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
        CaptureTarget { exe: "League of Legends.exe".into(), display_capture_only: false }
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
                options: &[],
            },
            ConfigField {
                key: "ult_key",
                label: "Ult key",
                kind: "key",
                help: "Leave empty: your ult keys are read from League's own settings (normal, quick and self cast). Set a key only to override them. After the game, casts are confirmed from the recording.",
                options: &[],
            },
            ConfigField {
                key: "record_spectating",
                label: "Record games you spectate",
                kind: "bool",
                help: "Off: spectating someone's live game isn't recorded (the tray says \"Spectating: not recorded\"). Replays (.rofl, \"Watch\" in match history) are never recorded.",
                options: &[],
            },
            ConfigField {
                key: "client_dir",
                label: "League install folder",
                kind: "text",
                help: "Only if game modes aren't detected: the folder with LeagueClient.exe, e.g. C:\\Riot Games\\League of Legends. Found automatically otherwise.",
                options: &[],
            },
        ]
    }
    fn default_config(&self) -> serde_json::Value {
        serde_json::json!({ "riot_id": "", "ult_key": "", "client_dir": "", "record_spectating": false })
    }
    fn configure(&mut self, config: &serde_json::Value) {
        self.riot_id = config["riot_id"].as_str().unwrap_or("").trim().to_string();
        self.ult_key = cv_core::settings::normalize_key(config["ult_key"].as_str().unwrap_or(""));
        self.client_dir = config["client_dir"].as_str().unwrap_or("").trim().to_string();
        self.record_spectating = config["record_spectating"].as_bool().unwrap_or(false);
        // Empty (or the old default "R"): League's own binds. Anything else overrides them.
        self.ult.manual = match self.ult_key.as_str() {
            "" | "R" | "Auto" => None,
            k => Some(ult::Bind::plain(k)),
        };
        // Hidden option used by the built-in game simulator.
        self.base = match config["api_base"].as_str().filter(|s| !s.is_empty()) {
            Some(b) => b.trim_end_matches('/').to_string(),
            None => std::env::var("GR_LEAGUE_API").unwrap_or_else(|_| DEFAULT_API.to_string()),
        };
    }

    async fn start(&mut self) -> anyhow::Result<()> {
        self.reset_match();
        self.read_league_config();
        Ok(())
    }

    /// How long the victory / defeat screen is still recorded after the match ends. Short, so
    /// the game is ready to watch a few seconds after it ends (v1.7.1; was 6 s).
    fn end_grace(&self) -> Duration {
        Duration::from_secs(2)
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
        // Once the in-game API answers: spectator mode (replay / spectating) or your champion?
        // Asked once per match (until it answers either way).
        if !self.live_decided && self.live_checks < 60 {
            self.live_checks += 1;
            if let Ok((status, body)) = self.get_raw("activeplayer").await {
                match watch::classify_active_player(status, &body) {
                    watch::LiveVerdict::Spectator => {
                        self.live_decided = true;
                        let k = self.watch_hint.unwrap_or(WatchKind::Unknown);
                        log::info!("in-game API: spectator mode ({})", k.label().to_lowercase());
                        u.watching = Some(k);
                        u.game_time = Some(stats.game_time);
                        u.phase = if stats.game_time > 0.0 { MatchPhase::InProgress } else { MatchPhase::Loading };
                        return Ok(u);
                    }
                    watch::LiveVerdict::Playing => {
                        self.live_decided = true;
                        u.playing = true;
                    }
                    watch::LiveVerdict::Undecided => {}
                }
            }
        }
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
        if u.phase == MatchPhase::InProgress && self.result.is_none() {
            self.update_ult_state(&stats.game_mode, &u.events, stats.game_time).await;
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
        use ultkind::LivePress;
        let (kind, title, details) = match self.ult.press_live(key, game_time) {
            LivePress::None => return None,
            LivePress::Used => (EventKind::UltPressed, "Ult pressed", "Ult key pressed. Checked against the recording after the game."),
            LivePress::Recast => (EventKind::UltRecast, "Ult recast", "The same ult pressed again (command, second part or early end). Checked after the game."),
            LivePress::FormSwap => (EventKind::FormSwap, "Form swap", "Form / stance swap. Checked against the recording after the game."),
        };
        Some(GameEvent::new(format!("ult-{:.2}", game_time), kind, game_time, title).with_details(details))
    }

    fn take_key_marks(&mut self) -> Vec<cv_core::game::KeyMark> {
        let mut m = self.ult.take_marks();
        if let Some(g) = self.ult_kind_mark.take() {
            m.push(cv_core::game::KeyMark { game_time: 0.0, action: "ult_kind".into(), key: g.as_str().into(), accepted: false, reason: Some("ddragon".into()) });
        }
        m
    }

    fn watch_detection(&mut self) -> Option<&mut dyn cv_core::game::WatchDetection> {
        Some(self)
    }

    fn mode_rules(&mut self) -> Option<&mut dyn cv_core::game::ModeRules> {
        Some(self)
    }

    fn cursor_input(&self) -> Option<&dyn cv_core::game::CursorInput> {
        Some(self)
    }

    fn recording_check(&self) -> Option<&dyn cv_core::game::RecordingCheck> {
        Some(self)
    }
}

#[async_trait]
impl cv_core::game::WatchDetection for LeagueIntegration {
    /// Replay / spectating / your match, from the League client (before anything is recorded).
    async fn session_check(&mut self) -> SessionCheck {
        self.watch_hint = None;
        self.live_decided = false;
        self.live_checks = 0;
        let Some(lcu) = queues::find_lockfile(&self.client_dir).and_then(|l| queues::Lcu::new(&l)) else {
            log::info!("session check: League client not found; the in-game API decides");
            return SessionCheck::Unknown;
        };
        let mut facts = watch::ClientFacts::default();
        for attempt in 0..2 {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(300)).await;
            }
            let (phase, replays, w) = tokio::join!(lcu.get_raw("/lol-gameflow/v1/gameflow-phase"), lcu.get_raw("/lol-replays/v1/configuration"), lcu.get_raw("/lol-gameflow/v1/watch"));
            if let Ok((200, v)) = phase {
                facts.phase = v.as_str().map(str::to_string);
            }
            if let Ok((200, v)) = replays {
                facts.playing_replay = v.get("isPlayingReplay").and_then(|b| b.as_bool());
            }
            if let Ok((200, v)) = w {
                facts.watch_phase = ["watchPhase", "phase"].iter().find_map(|k| v.get(*k).and_then(|p| p.as_str())).map(str::to_string);
            }
            if facts.phase.is_some() {
                break;
            }
        }
        let mut c = watch::classify_client(&facts);
        log::info!("session check: League client phase {:?}, playing replay {:?}, watch {:?} -> {c:?}", facts.phase, facts.playing_replay, facts.watch_phase);
        if c == SessionCheck::Playing {
            // Spectating a friend's game looks like your own match to the client: are you one
            // of its players?
            let (me, session) = tokio::join!(lcu.get_raw("/lol-summoner/v1/current-summoner"), lcu.get_raw("/lol-gameflow/v1/session"));
            let roster = match (me, session) {
                (Ok((200, me)), Ok((200, session))) => watch::in_roster(&me, &session),
                _ => None,
            };
            c = watch::refine_with_roster(c, roster);
            log::info!("session check: you are one of the match's players: {roster:?} -> {c:?}");
        }
        self.watch_hint = match c {
            SessionCheck::Watching(k) | SessionCheck::Unsure(k) => Some(k),
            _ => None,
        };
        c
    }

    fn record_spectating(&self) -> bool {
        self.record_spectating
    }
}

#[async_trait]
impl cv_core::game::ModeRules for LeagueIntegration {
    fn mode_groups(&self) -> Vec<ModeGroupInfo> {
        vec![
            ModeGroupInfo { id: "ranked", label: "Ranked", help: "Solo/Duo and Flex" },
            ModeGroupInfo { id: "normal", label: "Normal", help: "Draft, Quickplay, Swiftplay" },
            ModeGroupInfo { id: "aram", label: "ARAM", help: "Howling Abyss" },
            ModeGroupInfo { id: "arena", label: "Arena", help: "2v2v2v2" },
            ModeGroupInfo { id: "rotating", label: "Rotating & event modes", help: "URF, One for All and other limited-time modes" },
            ModeGroupInfo { id: "tft", label: "Teamfight Tactics", help: "TFT games (run in the League game client too)" },
            ModeGroupInfo { id: "other", label: "Other", help: "Custom games, Practice Tool, Co-op vs AI, Tutorial" },
        ]
    }

    /// Once per game, when it starts (loading screen): the exact queue from the League client;
    /// if the client can't be reached, the coarse game mode from the Live Client Data API.
    async fn detect_mode(&mut self) -> Option<MatchMode> {
        for attempt in 0..3 {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(700)).await;
            }
            let Some(lock) = queues::find_lockfile(&self.client_dir) else {
                log::debug!("mode: League client lockfile not found");
                continue;
            };
            let Some(lcu) = queues::Lcu::new(&lock) else { continue };
            match lcu.get("/lol-gameflow/v1/session").await {
                Ok(v) => {
                    if let Some(m) = queues::mode_from_session(&v) {
                        return Some(m);
                    }
                }
                Err(e) => log::debug!("mode: LCU gameflow: {e:#}"),
            }
        }
        // Fallback: the in-game API (only knows CLASSIC / ARAM / CHERRY / ...).
        for attempt in 0..3 {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(800)).await;
            }
            if let Ok(stats) = self.get::<GameStats>("gamestats").await {
                if !stats.game_mode.is_empty() {
                    let facts = queues::QueueFacts { id: -1, name: events::mode_name(&stats.game_mode), game_mode: stats.game_mode.clone(), ..Default::default() };
                    let (group, default_rule) = queues::classify(&facts);
                    let practice = stats.game_mode.eq_ignore_ascii_case("PRACTICETOOL");
                    return Some(MatchMode {
                        key: practice.then(|| queues::KEY_PRACTICE.to_string()),
                        queue_id: None,
                        name: facts.name,
                        game_mode: Some(stats.game_mode),
                        group,
                        default_rule,
                        source: "Live Client Data API (game mode only)".into(),
                    });
                }
            }
        }
        None
    }

    async fn mode_catalog(&mut self, cache_dir: &std::path::Path) -> (Vec<CatalogMode>, bool) {
        let mut out = queues::fixed_catalog();
        let mut authoritative = false;
        // 1. The client's own queue list (names in your language + what's playable now).
        let live = match queues::find_lockfile(&self.client_dir).and_then(|l| queues::Lcu::new(&l)) {
            Some(lcu) => lcu.get("/lol-game-queues/v1/queues").await.ok(),
            None => None,
        };
        match live {
            Some(v) => {
                queues::write_cache(cache_dir, "league-client-queues.json", &v.to_string());
                out.extend(queues::catalog_from_lcu(&v));
                authoritative = true;
            }
            None => {
                if let Some(v) = queues::read_cache(cache_dir, "league-client-queues.json").and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()) {
                    // Offline: names from the last time, availability unknown.
                    out.extend(queues::catalog_from_lcu(&v).into_iter().map(|mut c| {
                        c.available = None;
                        c
                    }));
                }
            }
        }
        // 2. Riot's official list (refreshed once a day, cached).
        let cached = queues::read_cache(cache_dir, "league-queues.json");
        let fresh = std::fs::metadata(cache_dir.join("league-queues.json"))
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age < Duration::from_secs(24 * 3600));
        let text = if fresh {
            cached
        } else {
            let fetched = async {
                let c = reqwest::Client::builder().timeout(Duration::from_secs(8)).user_agent("Clairvoyance").build().ok()?;
                let r = c.get(queues::QUEUES_JSON_URL).send().await.ok()?.error_for_status().ok()?;
                r.text().await.ok()
            }
            .await;
            match fetched {
                Some(t) if !queues::parse_static(&t).is_empty() => {
                    queues::write_cache(cache_dir, "league-queues.json", &t);
                    Some(t)
                }
                _ => cached,
            }
        };
        if let Some(t) = text {
            for mut c in queues::static_catalog(&queues::parse_static(&t)) {
                if !out.iter().any(|o| o.key == c.key) {
                    // Not in the client's list of playable queues right now.
                    if authoritative {
                        c.available = Some(false);
                    }
                    out.push(c);
                }
            }
        }
        for mut c in queues::known_catalog() {
            if !out.iter().any(|o| o.key == c.key) {
                if authoritative {
                    c.available = Some(false);
                }
                out.push(c);
            }
        }
        (out, authoritative)
    }
}

impl cv_core::game::CursorInput for LeagueIntegration {
    fn chat_open(&self) -> bool {
        self.ult.chat_open
    }

    fn mouse_marks(&self, presses: &[(f64, u8)]) -> Vec<cv_core::game::KeyMark> {
        self.ult.mouse_marks(presses)
    }

    fn action_keys(&self) -> Vec<cv_core::input::actions::ActionKey> {
        actions::with_manual_ult(self.actions.clone(), self.ult.manual.as_ref())
    }

    fn default_action_keys(&self) -> Vec<cv_core::input::actions::ActionKey> {
        actions::default_actions()
    }

    fn action_categories(&self) -> Vec<cv_core::input::actions::ActionCategory> {
        actions::categories()
    }

    fn action_press_states(&self, session: &cv_core::session::GameSession, keys: &[cv_core::input::actions::ActionKey], presses: &mut [cv_core::input::actions::ActionPress]) {
        actions::press_states(session, keys, presses)
    }
}

impl cv_core::game::RecordingCheck for LeagueIntegration {
    fn verify_recording(&self, session: &mut cv_core::session::GameSession, video: &mut dyn cv_core::game::FrameSource, cancel: &dyn Fn() -> bool) -> anyhow::Result<()> {
        verify::verify(session, video, &ult::UltRules::load(data_dir()), cancel)
    }

    fn verify_version(&self) -> u32 {
        verify::VERSION
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
        // No API data yet (level, cooldown unknown): a second press counts too; the recording decides.
        assert!(l.on_key(&kp("R", false), 100.3).is_some());
        assert!(l.on_key(&kp("R", false), 100.33).is_none(), "key bounce");
        assert!(l.on_key(&kp("R", true), 110.0).is_none(), "ctrl+R levels up");
        assert!(l.on_key(&kp("Enter", false), 120.0).is_none());
        assert!(l.on_key(&kp("R", false), 121.0).is_none(), "typing in chat");
        assert!(l.on_key(&kp("Enter", false), 122.0).is_none());
        assert!(l.on_key(&kp("R", false), 123.0).is_some());
        assert!(l.on_key(&kp("Q", false), 130.0).is_none());
        let marks = l.take_key_marks();
        assert_eq!(marks.iter().filter(|m| m.accepted).count(), 3);
        assert!(marks.iter().any(|m| m.reason.as_deref() == Some("chat")), "{marks:?}");
        assert!(l.take_key_marks().is_empty());
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
