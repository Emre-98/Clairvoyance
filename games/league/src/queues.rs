//! Which queue (game mode) a League match is: Ranked Solo/Duo, Draft, ARAM, Arena, URF, …
//!
//! - The **League Client API (LCU)**, the local API of the League client that tools like
//!   Porofessor and Outplayed use (read-only, no game memory access, Vanguard-safe). Its port
//!   and password are in the client's `lockfile`. `/lol-gameflow/v1/session` gives the exact
//!   queue of the current game; `/lol-game-queues/v1/queues` lists every queue with its name in
//!   the client's language and whether it's playable right now.
//! - Riot's official **queues.json** (static data) names queues when the client isn't running.
//! - The **Live Client Data API** only knows the coarse `gameMode` (CLASSIC, ARAM, CHERRY…); it's
//!   the fallback when the client can't be reached.
//! Both lists are cached on disk, so the settings list works offline. Nothing here hardcodes
//! the list of modes: only a small table of well-known queue ids gives default groups/rules
//! ([`KNOWN`], easy to extend), everything else is classified from the queue's own data.

use cv_core::modes::{CatalogMode, MatchMode, ModeRule};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const QUEUES_JSON_URL: &str = "https://static.developer.riotgames.com/docs/lol/queues.json";

/// Well-known queues: (queue id, group, default rule, fallback name). Only defaults: names come
/// from the client or Riot's list whenever those are available (the fallback name is used only
/// on a PC that has never reached either). Add a line if Riot introduces a new permanent queue.
pub const KNOWN: &[(i64, &str, ModeRule, &str)] = &[
    (420, "ranked", ModeRule::Record, "Ranked Solo/Duo"),
    (440, "ranked", ModeRule::Record, "Ranked Flex"),
    (400, "normal", ModeRule::Record, "Normal Draft"),
    (430, "normal", ModeRule::Record, "Normal Blind"),
    (480, "normal", ModeRule::Record, "Swiftplay"),
    (490, "normal", ModeRule::Record, "Quickplay"),
    (450, "aram", ModeRule::Record, "ARAM"),
    (1700, "arena", ModeRule::Record, "Arena"),
    (1710, "arena", ModeRule::Record, "Arena"),
];

/// Keys for games without a real queue id.
pub const KEY_PRACTICE: &str = "practice";
pub const KEY_CUSTOM: &str = "custom";

pub fn key_for(queue_id: i64) -> String {
    format!("q{queue_id}")
}

/// Everything we may know about a queue, from whichever source.
#[derive(Debug, Clone, Default)]
pub struct QueueFacts {
    pub id: i64,
    pub name: String,
    pub game_mode: String,
    pub queue_type: String,
    pub category: String,
    pub is_ranked: bool,
    pub is_custom: bool,
    pub map_name: String,
}

/// Group id (see `LeagueIntegration::mode_groups`) and default rule for a queue.
pub fn classify(q: &QueueFacts) -> (String, Option<ModeRule>) {
    if q.game_mode.eq_ignore_ascii_case("PRACTICETOOL") || q.name.to_lowercase().contains("practice tool") {
        return ("other".into(), Some(ModeRule::Off));
    }
    if q.is_custom || q.category.eq_ignore_ascii_case("Custom") || (q.id <= 0 && q.game_mode.is_empty()) {
        return ("other".into(), Some(ModeRule::Off));
    }
    if let Some((_, g, r, _)) = KNOWN.iter().find(|(id, _, _, _)| *id == q.id) {
        return (g.to_string(), Some(*r));
    }
    let name = q.name.to_lowercase();
    let t = q.queue_type.to_uppercase();
    if q.game_mode.eq_ignore_ascii_case("TFT") || name.contains("teamfight tactics") || t.contains("TFT") {
        return ("tft".into(), Some(ModeRule::Record));
    }
    // Clash is flagged "ranked" by Riot but it's a tournament event: it goes with the events.
    if t.contains("CLASH") || name.contains("clash") {
        return ("rotating".into(), Some(ModeRule::Record));
    }
    if q.is_ranked || t.starts_with("RANKED") || name.contains("ranked") {
        return ("ranked".into(), Some(ModeRule::Record));
    }
    if q.category.eq_ignore_ascii_case("VersusAi") || t.contains("BOT") || name.contains("co-op") || name.contains("vs. ai") || name.contains("vs ai") {
        return ("other".into(), Some(ModeRule::Record));
    }
    if q.game_mode.to_uppercase().starts_with("TUTORIAL") || name.contains("tutorial") {
        return ("other".into(), Some(ModeRule::Record));
    }
    let gm = q.game_mode.to_uppercase();
    let map = q.map_name.to_lowercase();
    if gm == "ARAM" || name.contains("aram") || (gm.is_empty() && map.contains("howling abyss") && !name.contains("urf") && !name.contains("one for all")) {
        return ("aram".into(), Some(ModeRule::Record));
    }
    if gm == "CHERRY" || map.contains("rings of wrath") || name.contains("arena") {
        return ("arena".into(), Some(ModeRule::Record));
    }
    let normal_words = ["draft", "blind", "quickplay", "swiftplay", "normal"];
    if gm == "CLASSIC" || gm == "SWIFTPLAY" || (gm.is_empty() && map.contains("summoner") && normal_words.iter().any(|w| name.contains(w))) {
        return ("normal".into(), Some(ModeRule::Record));
    }
    // URF, One for All, Nexus Blitz, Swarm, Ultimate Spellbook, event modes...: the "unknown /
    // new modes" rule decides for anything not seen before.
    ("rotating".into(), None)
}

pub fn catalog_entry(q: &QueueFacts, available: Option<bool>) -> CatalogMode {
    let (group, default_rule) = classify(q);
    CatalogMode {
        key: key_for(q.id),
        queue_id: Some(q.id),
        name: clean_name(&q.name, q.id),
        game_mode: (!q.game_mode.is_empty()).then(|| q.game_mode.clone()),
        group,
        default_rule,
        available,
    }
}

fn clean_name(name: &str, id: i64) -> String {
    let n = name.trim().trim_end_matches(" games").trim_end_matches(" Games").trim();
    if n.is_empty() { format!("Queue {id}") } else { n.to_string() }
}

/// Riot's official queue list (static data). `notes` mark deprecated queues.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StaticQueue {
    pub queue_id: i64,
    #[serde(default)]
    pub map: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

pub fn parse_static(json: &str) -> Vec<StaticQueue> {
    serde_json::from_str(json).unwrap_or_default()
}

/// Not-deprecated queues from Riot's list. Riot's list also keeps old event modes, so apart from
/// the well-known queues they're marked "not currently available" until the client lists them
/// or one is played (then they become available and keep the name).
pub fn static_catalog(list: &[StaticQueue]) -> Vec<CatalogMode> {
    list.iter()
        .filter(|q| q.queue_id > 0)
        .filter(|q| !q.notes.as_deref().unwrap_or("").to_lowercase().contains("deprecated"))
        .filter_map(|q| {
            let desc = q.description.clone().unwrap_or_default();
            if desc.is_empty() {
                return None;
            }
            let facts = QueueFacts { id: q.queue_id, name: desc.clone(), map_name: q.map.clone(), ..Default::default() };
            let known = KNOWN.iter().any(|k| k.0 == q.queue_id);
            Some(catalog_entry(&facts, if known { None } else { Some(false) }))
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// League Client API (LCU)
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct Lockfile {
    pub port: u16,
    pub password: String,
    /// "https" (the real client); "http" only for Clairvoyance's own simulator.
    pub protocol: String,
}

/// `LeagueClient:<pid>:<port>:<password>:https`
pub fn parse_lockfile(text: &str) -> Option<Lockfile> {
    let parts: Vec<&str> = text.trim().split(':').collect();
    if parts.len() < 5 {
        return None;
    }
    let protocol = if parts[4].trim() == "http" { "http" } else { "https" };
    Some(Lockfile { port: parts[2].parse().ok()?, password: parts[3].to_string(), protocol: protocol.into() })
}

/// Where League may be installed: the configured folder, what the Riot Client knows, then
/// the usual places.
pub fn install_dirs(configured: &str) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    if !configured.trim().is_empty() {
        out.push(PathBuf::from(configured.trim()));
    }
    let pd = std::env::var_os("PROGRAMDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("C:\\ProgramData"));
    // RiotClientInstalls.json: {"associated_client": {"C:/Riot Games/League of Legends/": "..."}}
    if let Ok(t) = std::fs::read_to_string(pd.join("Riot Games").join("RiotClientInstalls.json")) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&t) {
            if let Some(map) = v.get("associated_client").and_then(|m| m.as_object()) {
                for k in map.keys().filter(|k| k.to_lowercase().contains("league of legends")) {
                    out.push(PathBuf::from(k.trim_end_matches(['/', '\\'])));
                }
            }
        }
    }
    // product_settings.yaml: product_install_full_path: "C:/Riot Games/League of Legends"
    let yaml = pd.join("Riot Games").join("Metadata").join("league_of_legends.live").join("league_of_legends.live.product_settings.yaml");
    if let Ok(t) = std::fs::read_to_string(yaml) {
        for line in t.lines() {
            if let Some(rest) = line.trim().strip_prefix("product_install_full_path:") {
                out.push(PathBuf::from(rest.trim().trim_matches('"').trim_matches('\'')));
            }
        }
    }
    for drive in ["C", "D", "E"] {
        out.push(PathBuf::from(format!("{drive}:\\Riot Games\\League of Legends")));
    }
    out.dedup();
    out
}

pub fn find_lockfile(configured: &str) -> Option<Lockfile> {
    install_dirs(configured).into_iter().find_map(|d| std::fs::read_to_string(d.join("lockfile")).ok().and_then(|t| parse_lockfile(&t)))
}

/// A client for the LCU on 127.0.0.1. It uses Riot's self-signed certificate; this client
/// only ever talks to localhost, so accepting it is limited to that.
pub struct Lcu {
    client: reqwest::Client,
    base: String,
    password: String,
}

impl Lcu {
    pub fn new(lock: &Lockfile) -> Option<Lcu> {
        let client = reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .no_proxy()
            .timeout(Duration::from_millis(2500))
            .connect_timeout(Duration::from_millis(600))
            .build()
            .ok()?;
        Some(Lcu { client, base: format!("{}://127.0.0.1:{}", lock.protocol, lock.port), password: lock.password.clone() })
    }

    pub async fn get(&self, path: &str) -> anyhow::Result<serde_json::Value> {
        let r = self.client.get(format!("{}{}", self.base, path)).basic_auth("riot", Some(&self.password)).send().await?;
        if !r.status().is_success() {
            anyhow::bail!("{path}: HTTP {}", r.status());
        }
        Ok(r.json().await?)
    }
}

/// Facts from an LCU queue object (in `/lol-gameflow/v1/session` → gameData.queue, or the
/// items of `/lol-game-queues/v1/queues`).
pub fn facts_from_lcu(q: &serde_json::Value) -> QueueFacts {
    let s = |k: &str| q.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
    let name = [s("description"), s("name"), s("shortName")].into_iter().find(|n| !n.trim().is_empty()).unwrap_or_default();
    QueueFacts {
        id: q.get("id").and_then(|v| v.as_i64()).unwrap_or(-1),
        name,
        game_mode: s("gameMode"),
        queue_type: s("type"),
        category: s("category"),
        is_ranked: q.get("isRanked").and_then(|v| v.as_bool()).unwrap_or(false),
        is_custom: s("category").eq_ignore_ascii_case("Custom"),
        map_name: String::new(),
    }
}

/// The match mode from a `/lol-gameflow/v1/session` response.
pub fn mode_from_session(session: &serde_json::Value) -> Option<MatchMode> {
    let gd = session.get("gameData")?;
    let q = gd.get("queue")?;
    let mut f = facts_from_lcu(q);
    let custom = gd.get("isCustomGame").and_then(|v| v.as_bool()).unwrap_or(false);
    if f.game_mode.is_empty() {
        f.game_mode = q.get("gameMode").and_then(|v| v.as_str()).unwrap_or("").to_string();
    }
    f.is_custom = f.is_custom || custom;
    if f.id <= 0 && f.game_mode.is_empty() && !custom {
        return None; // not in a game (champ select not started)
    }
    let (group, default_rule) = classify(&f);
    let (key, name) = if f.game_mode.eq_ignore_ascii_case("PRACTICETOOL") {
        (KEY_PRACTICE.to_string(), "Practice Tool".to_string())
    } else if f.is_custom || f.id <= 0 {
        (KEY_CUSTOM.to_string(), "Custom games".to_string())
    } else {
        (key_for(f.id), clean_name(&f.name, f.id))
    };
    Some(MatchMode {
        key: Some(key),
        queue_id: (f.id > 0).then_some(f.id),
        name,
        game_mode: (!f.game_mode.is_empty()).then_some(f.game_mode),
        group,
        default_rule,
        source: "League client (LCU)".into(),
    })
}

/// The client's full queue list → catalog (authoritative for availability).
pub fn catalog_from_lcu(queues: &serde_json::Value) -> Vec<CatalogMode> {
    let Some(arr) = queues.as_array() else { return Vec::new() };
    arr.iter()
        .filter_map(|q| {
            let f = facts_from_lcu(q);
            if f.id <= 0 || f.is_custom {
                return None;
            }
            let avail = q.get("queueAvailability").and_then(|v| v.as_str()).map(|a| a.eq_ignore_ascii_case("Available"));
            Some(catalog_entry(&f, avail))
        })
        .collect()
}

/// Entries that always exist (they have no queue id).
pub fn fixed_catalog() -> Vec<CatalogMode> {
    vec![
        CatalogMode { key: KEY_CUSTOM.into(), queue_id: None, name: "Custom games".into(), game_mode: None, group: "other".into(), default_rule: Some(ModeRule::Off), available: Some(true) },
        CatalogMode { key: KEY_PRACTICE.into(), queue_id: None, name: "Practice Tool".into(), game_mode: Some("PRACTICETOOL".into()), group: "other".into(), default_rule: Some(ModeRule::Off), available: Some(true) },
    ]
}

/// The well-known queues with their fallback names (lowest priority source).
pub fn known_catalog() -> Vec<CatalogMode> {
    KNOWN
        .iter()
        .map(|(id, g, r, name)| CatalogMode { key: key_for(*id), queue_id: Some(*id), name: name.to_string(), game_mode: None, group: g.to_string(), default_rule: Some(*r), available: None })
        .collect()
}

/// Reads a cached file if it's there.
pub fn read_cache(dir: &Path, name: &str) -> Option<String> {
    std::fs::read_to_string(dir.join(name)).ok()
}

pub fn write_cache(dir: &Path, name: &str, text: &str) {
    let _ = std::fs::create_dir_all(dir);
    let tmp = dir.join(format!("{name}.tmp"));
    if std::fs::write(&tmp, text).is_ok() {
        let _ = std::fs::rename(tmp, dir.join(name));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn lockfile() {
        assert_eq!(parse_lockfile("LeagueClient:12345:54321:AbCdEf123:https"), Some(Lockfile { port: 54321, password: "AbCdEf123".into(), protocol: "https".into() }));
        assert_eq!(parse_lockfile("garbage"), None);
    }

    #[test]
    fn session_modes() {
        let ranked = json!({"phase":"InProgress","gameData":{"isCustomGame":false,"queue":{"id":420,"description":"Ranked Solo/Duo","type":"RANKED_SOLO_5x5","isRanked":true,"gameMode":"CLASSIC"}}});
        let m = mode_from_session(&ranked).unwrap();
        assert_eq!((m.key.as_deref(), m.queue_id, m.name.as_str(), m.group.as_str()), (Some("q420"), Some(420), "Ranked Solo/Duo", "ranked"));
        let aram = json!({"gameData":{"queue":{"id":450,"description":"ARAM","gameMode":"ARAM","type":"ARAM_UNRANKED_5x5"}}});
        assert_eq!(mode_from_session(&aram).unwrap().group, "aram");
        let arena = json!({"gameData":{"queue":{"id":1700,"description":"Arena","gameMode":"CHERRY"}}});
        assert_eq!(mode_from_session(&arena).unwrap().group, "arena");
        let practice = json!({"gameData":{"isCustomGame":true,"queue":{"id":-1,"gameMode":"PRACTICETOOL"}}});
        let p = mode_from_session(&practice).unwrap();
        assert_eq!((p.key.as_deref(), p.default_rule), (Some("practice"), Some(ModeRule::Off)));
        let custom = json!({"gameData":{"isCustomGame":true,"queue":{"id":-1,"gameMode":"CLASSIC"}}});
        assert_eq!(mode_from_session(&custom).unwrap().key.as_deref(), Some("custom"));
        let urf = json!({"gameData":{"queue":{"id":1900,"description":"Pick URF","gameMode":"URF","type":"URF"}}});
        let u = mode_from_session(&urf).unwrap();
        assert_eq!((u.group.as_str(), u.default_rule), ("rotating", None));
        let brand_new = json!({"gameData":{"queue":{"id":31337,"description":"Totally New Mode","gameMode":"NEWTHING"}}});
        assert_eq!(mode_from_session(&brand_new).unwrap().group, "rotating");
        assert!(mode_from_session(&json!({"gameData":{"queue":{"id":-1,"gameMode":""}}})).is_none());
    }

    #[test]
    fn client_queue_list() {
        let q = json!([
            {"id":420,"name":"Ranked Solo/Duo","description":"Ranked Solo/Duo","gameMode":"CLASSIC","isRanked":true,"queueAvailability":"Available","category":"PvP"},
            {"id":900,"name":"ARURF","description":"ARURF","gameMode":"URF","queueAvailability":"PlatformDisabled","category":"PvP"},
            {"id":870,"description":"Co-op vs. AI Intro","gameMode":"CLASSIC","category":"VersusAi","type":"BOT","queueAvailability":"Available"}
        ]);
        let c = catalog_from_lcu(&q);
        assert_eq!(c.len(), 3);
        assert_eq!(c[1].available, Some(false));
        assert_eq!(c[2].group, "other");
    }

    #[test]
    fn riot_static_list() {
        let list = parse_static(r#"[{"queueId":0,"map":"Custom games","description":null,"notes":null},
            {"queueId":420,"map":"Summoner's Rift","description":"5v5 Ranked Solo games","notes":null},
            {"queueId":450,"map":"Howling Abyss","description":"5v5 ARAM games","notes":null},
            {"queueId":65,"map":"Howling Abyss","description":"5v5 ARAM games","notes":"Deprecated in patch 7.19 in favor of queueId 450"},
            {"queueId":1700,"map":"Rings of Wrath","description":"Arena","notes":null},
            {"queueId":1020,"map":"Summoner's Rift","description":"One for All games","notes":null}]"#);
        let c = static_catalog(&list);
        let ids: Vec<_> = c.iter().map(|m| (m.queue_id.unwrap(), m.group.as_str(), m.name.as_str())).collect();
        assert_eq!(ids, vec![(420, "ranked", "5v5 Ranked Solo"), (450, "aram", "5v5 ARAM"), (1700, "arena", "Arena"), (1020, "rotating", "One for All")]);
    }

    #[test]
    fn clash_is_an_event_not_ranked() {
        let f = |id, name: &str, t: &str, gm: &str| QueueFacts { id, name: name.into(), queue_type: t.into(), game_mode: gm.into(), category: "PvP".into(), is_ranked: true, ..Default::default() };
        assert_eq!(classify(&f(700, "Clash", "CLASH", "CLASSIC")).0, "rotating");
        assert_eq!(classify(&f(741, "AR Ultra Rapid Fire Clash", "URF_CLASH", "URF")).0, "rotating");
        assert_eq!(classify(&f(420, "Ranked Solo/Duo", "RANKED_SOLO_5x5", "CLASSIC")).0, "ranked");
        assert_eq!(classify(&f(1100, "Teamfight Tactics (Ranked)", "RANKED_TFT", "TFT")), ("tft".to_string(), Some(ModeRule::Record)));
    }
}