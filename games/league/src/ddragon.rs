//! Data Dragon (Riot's public static data): the ult's base cooldown per rank for the game's
//! patch and its texts (to guess the ult's kind for champions the rules file doesn't know),
//! cached on disk per patch. Fetched in the background at the loading screen; never
//! blocks anything (until it arrives, the cooldown filter is simply off).

use std::path::{Path, PathBuf};
use std::time::Duration;

const VERSIONS_URL: &str = "https://ddragon.leagueoflegends.com/api/versions.json";

/// The Data Dragon version for a patch ("16.19" → "16.19.1"), or the newest one.
pub fn pick_version(versions: &[String], patch: Option<&str>) -> Option<String> {
    if let Some(p) = patch {
        let prefix = format!("{p}.");
        if let Some(v) = versions.iter().find(|v| v.starts_with(&prefix)) {
            return Some(v.clone());
        }
    }
    versions.first().cloned()
}

/// `data.<champ>.spells[3].cooldown` of a champion file.
pub fn parse_ult_cooldown(json: &str, champ: &str) -> Option<Vec<f64>> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let data = v["data"].as_object()?;
    let c = data.get(champ).or_else(|| data.values().next())?;
    let cd: Vec<f64> = c["spells"].get(3)?["cooldown"].as_array()?.iter().filter_map(|x| x.as_f64()).collect();
    (!cd.is_empty()).then_some(cd)
}

/// The ult part of a champion file: cooldowns, charges and texts.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct UltData {
    pub cooldown: Vec<f64>,
    #[serde(default)]
    pub max_ammo: Option<i64>,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub tooltip: String,
}

/// `data.<champ>.spells[3]` of a champion file.
pub fn parse_ult(json: &str, champ: &str) -> Option<UltData> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let data = v["data"].as_object()?;
    let c = data.get(champ).or_else(|| data.values().next())?;
    ult_of(c)
}

/// The R spell of one champion object (`data.<champ>`).
pub fn ult_of(c: &serde_json::Value) -> Option<UltData> {
    let r = c["spells"].get(3)?;
    let cooldown: Vec<f64> = r["cooldown"].as_array()?.iter().filter_map(|x| x.as_f64()).collect();
    if cooldown.is_empty() {
        return None;
    }
    let max_ammo = r["maxammo"].as_str().and_then(|s| s.parse().ok()).or_else(|| r["maxammo"].as_i64());
    let s = |k: &str| r[k].as_str().unwrap_or("").to_string();
    Some(UltData { cooldown, max_ammo, name: s("name"), description: s("description"), tooltip: s("tooltip") })
}

/// What the game needs: the base cooldowns and, from the texts, a guess of the ult's kind.
#[derive(Debug, Clone, PartialEq)]
pub struct UltInfo {
    pub cooldown: Vec<f64>,
    pub kind_guess: Option<crate::ultkind::UltKind>,
}

impl From<UltData> for UltInfo {
    fn from(d: UltData) -> Self {
        let (kind_guess, _) = crate::ultkind::classify_text(&d.description, &d.tooltip, d.max_ammo, &d.cooldown);
        UltInfo { cooldown: d.cooldown, kind_guess }
    }
}

fn cache_file(dir: &Path, version: &str, champ: &str) -> PathBuf {
    let safe: String = champ.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    dir.join("ddragon").join(version).join(format!("{safe}-R2.json"))
}

/// The ult of `champ` for `patch`, from the disk cache or Data Dragon.
pub async fn ult_info(cache_dir: Option<&Path>, patch: Option<&str>, champ: &str) -> Option<UltInfo> {
    let client = reqwest::Client::builder().timeout(Duration::from_secs(8)).user_agent("Clairvoyance").build().ok()?;
    let version = version_for(&client, cache_dir, patch).await?;
    if let Some(d) = cache_dir {
        if let Some(u) = std::fs::read_to_string(cache_file(d, &version, champ)).ok().and_then(|t| serde_json::from_str::<UltData>(&t).ok()) {
            return Some(u.into());
        }
    }
    let url = format!("https://ddragon.leagueoflegends.com/cdn/{version}/data/en_US/champion/{champ}.json");
    let text = client.get(&url).send().await.ok()?.error_for_status().ok()?.text().await.ok()?;
    let u = parse_ult(&text, champ)?;
    if let Some(d) = cache_dir {
        let f = cache_file(d, &version, champ);
        let _ = std::fs::create_dir_all(f.parent().unwrap());
        let _ = std::fs::write(f, serde_json::to_string(&u).unwrap_or_default());
    }
    let info: UltInfo = u.into();
    log::info!("ult of {champ} (Data Dragon {version}): cooldown {:?}, text looks {}", info.cooldown, info.kind_guess.map(|k| k.as_str()).unwrap_or("normal"));
    Some(info)
}

/// One item of Data Dragon's `item.json`, reduced to what the timeline needs.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ItemData {
    pub name: String,
    /// Total cost (with its components).
    pub total: u32,
    /// A finished item: built from components, upgrades into nothing (or, for tier-2 boots,
    /// only into their own tier-3 upgrade), not a consumable or trinket, buyable in the shop.
    pub finished: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub from: Vec<u32>,
}

/// One summoner spell of `summoner.json`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SpellData {
    /// "SummonerFlash" (also the icon's file name).
    pub id: String,
    pub name: String,
    /// Base cooldown in seconds.
    pub cooldown: f64,
}

/// Items and summoner spells of one Data Dragon version.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StaticData {
    pub version: String,
    pub items: std::collections::HashMap<u32, ItemData>,
    pub spells: Vec<SpellData>,
}

impl StaticData {
    pub fn item(&self, id: u32) -> Option<&ItemData> {
        self.items.get(&id)
    }
    pub fn spell(&self, id: &str) -> Option<&SpellData> {
        self.spells.iter().find(|s| s.id.eq_ignore_ascii_case(id))
    }
}

/// Parses `item.json` (see [`ItemData::finished`] for what counts as finished).
pub fn parse_items(json: &str) -> std::collections::HashMap<u32, ItemData> {
    let mut out = std::collections::HashMap::new();
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else { return out };
    let Some(data) = v["data"].as_object() else { return out };
    let ids = |x: &serde_json::Value| -> Vec<u32> { x.as_array().map(|a| a.iter().filter_map(|i| i.as_str()?.parse().ok()).collect()).unwrap_or_default() };
    let tags = |x: &serde_json::Value| -> Vec<String> { x["tags"].as_array().map(|a| a.iter().filter_map(|t| t.as_str().map(str::to_string)).collect()).unwrap_or_default() };
    // An in-place upgrade of `of`: built from it alone (tier-3 boots from tier-2 boots).
    let upgrade_of = |id: &u32, of: u32| data.get(&id.to_string()).is_some_and(|x| ids(&x["from"]) == vec![of]);
    for (k, x) in data {
        let Ok(id) = k.parse::<u32>() else { continue };
        let into = ids(&x["into"]);
        let from = ids(&x["from"]);
        let t = tags(x);
        let boots = t.iter().any(|t| t == "Boots");
        let end_of_tree = into.is_empty() || (boots && into.iter().all(|i| upgrade_of(i, id)));
        let finished = end_of_tree
            && !from.is_empty()
            && !x["consumed"].as_bool().unwrap_or(false)
            && !t.iter().any(|t| t == "Trinket" || t == "Consumable")
            && x["gold"]["purchasable"].as_bool().unwrap_or(true)
            && x["requiredAlly"].as_str().is_none_or(|s| s.is_empty());
        let total = x["gold"]["total"].as_u64().unwrap_or(0) as u32;
        out.insert(id, ItemData { name: x["name"].as_str().unwrap_or("").to_string(), total, finished, from });
    }
    out
}

/// Parses `summoner.json`.
pub fn parse_spells(json: &str) -> Vec<SpellData> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else { return Vec::new() };
    let Some(data) = v["data"].as_object() else { return Vec::new() };
    data.iter()
        .map(|(k, x)| SpellData {
            id: x["id"].as_str().unwrap_or(k).to_string(),
            name: x["name"].as_str().unwrap_or(k).to_string(),
            cooldown: x["cooldown"].get(0).and_then(|c| c.as_f64()).unwrap_or(0.0),
        })
        .collect()
}

/// Items and summoner spells for `patch`, from the disk cache or Data Dragon (two files, ~0.7 MB,
/// once per patch; kept as a small summary).
pub async fn static_data(cache_dir: Option<&Path>, patch: Option<&str>) -> Option<StaticData> {
    let client = reqwest::Client::builder().timeout(Duration::from_secs(15)).user_agent("Clairvoyance").build().ok()?;
    let version = version_for(&client, cache_dir, patch).await?;
    let file = cache_dir.map(|d| d.join("ddragon").join(&version).join("static-v1.json"));
    if let Some(s) = file.as_ref().and_then(|f| std::fs::read_to_string(f).ok()).and_then(|t| serde_json::from_str::<StaticData>(&t).ok()) {
        return Some(s);
    }
    let get = |what: &str| {
        let url = format!("https://ddragon.leagueoflegends.com/cdn/{version}/data/en_US/{what}.json");
        let c = client.clone();
        async move { c.get(&url).send().await.ok()?.error_for_status().ok()?.text().await.ok() }
    };
    let (items, spells) = tokio::join!(get("item"), get("summoner"));
    let s = StaticData { version: version.clone(), items: parse_items(&items?), spells: spells.map(|t| parse_spells(&t)).unwrap_or_default() };
    if s.items.is_empty() {
        return None;
    }
    if let Some(f) = file {
        let _ = std::fs::create_dir_all(f.parent().unwrap());
        let _ = std::fs::write(f, serde_json::to_string(&s).unwrap_or_default());
    }
    log::info!("Data Dragon {version}: {} items ({} finished), {} summoner spells", s.items.len(), s.items.values().filter(|i| i.finished).count(), s.spells.len());
    Some(s)
}

/// Base cooldowns of `champ`'s ult for `patch` (see [`ult_info`]).
pub async fn ult_cooldown(cache_dir: Option<&Path>, patch: Option<&str>, champ: &str) -> Option<Vec<f64>> {
    ult_info(cache_dir, patch, champ).await.map(|i| i.cooldown)
}

/// The Data Dragon version for `patch` (version list cached for a day).
pub async fn version_for(client: &reqwest::Client, cache_dir: Option<&Path>, patch: Option<&str>) -> Option<String> {
    let versions_file = cache_dir.map(|d| d.join("ddragon").join("versions.json"));
    let fresh = versions_file
        .as_ref()
        .and_then(|f| std::fs::metadata(f).ok())
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age < Duration::from_secs(24 * 3600));
    let cached_versions = versions_file.as_ref().and_then(|f| std::fs::read_to_string(f).ok()).and_then(|t| serde_json::from_str::<Vec<String>>(&t).ok());
    let versions = match (&cached_versions, fresh) {
        (Some(v), true) => v.clone(),
        _ => match client.get(VERSIONS_URL).send().await.and_then(|r| r.error_for_status()) {
            Ok(r) => match r.json::<Vec<String>>().await {
                Ok(v) if !v.is_empty() => {
                    if let Some(f) = &versions_file {
                        let _ = std::fs::create_dir_all(f.parent().unwrap());
                        let _ = std::fs::write(f, serde_json::to_string(&v).unwrap_or_default());
                    }
                    v
                }
                _ => cached_versions.clone()?,
            },
            Err(_) => cached_versions.clone()?,
        },
    };
    pick_version(&versions, patch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_for_patch() {
        let v: Vec<String> = ["16.20.1", "16.19.1", "16.18.1"].iter().map(|s| s.to_string()).collect();
        assert_eq!(pick_version(&v, Some("16.19")).as_deref(), Some("16.19.1"));
        assert_eq!(pick_version(&v, Some("15.1")).as_deref(), Some("16.20.1"), "unknown patch: newest");
        assert_eq!(pick_version(&v, None).as_deref(), Some("16.20.1"));
    }

    #[test]
    fn ult_texts_and_kind_guess() {
        let j = r#"{"data":{"Annie":{"spells":[{},{},{},{"name":"Summon: Tibbers","cooldown":[130,115,100],"maxammo":"-1","description":"Annie wills her bear Tibbers to life.","tooltip":"Summons Tibbers.<br><br><recast>Recast:</recast> Manually issue orders to Tibbers."}]}}}"#;
        let u = parse_ult(j, "Annie").unwrap();
        assert_eq!(u.max_ammo, Some(-1));
        let i: UltInfo = u.into();
        assert_eq!(i.cooldown, vec![130.0, 115.0, 100.0]);
        assert_eq!(i.kind_guess, Some(crate::ultkind::UltKind::MultiCast), "a recast; 'orders' isn't the command keyword");
    }

    /// The real 16.20.1 files, trimmed to a few items / spells (`tests/ddragon/items-16.20.json`).
    #[test]
    fn finished_items() {
        let items = parse_items(include_str!("../tests/ddragon/items-16.20.json"));
        let fin = |id: u32| items.get(&id).map(|i| i.finished);
        assert_eq!(fin(3031), Some(true), "Infinity Edge");
        assert_eq!(items[&3031].total, 3500);
        assert_eq!(items[&3031].from, vec![1038, 1037, 1018]);
        assert_eq!(fin(3006), Some(true), "Berserker's Greaves: only upgrades into tier-3 boots");
        assert_eq!(fin(3172), Some(true), "tier-3 boots");
        assert_eq!(fin(1001), Some(false), "Boots (tier 1) upgrade into many items");
        assert_eq!(fin(1055), Some(false), "Doran's Blade: a starter, not built from components");
        assert_eq!(fin(1038), Some(false), "B. F. Sword: a component");
        assert_eq!(fin(2003), Some(false), "Health Potion: consumable");
        assert_eq!(fin(3340), Some(false), "Stealth Ward: trinket");
        assert_eq!(fin(3042), Some(false), "Muramana: not bought (transforms from Manamune)");
        assert_eq!(fin(3004), Some(true), "Manamune");
        let spells = parse_spells(include_str!("../tests/ddragon/summoner-16.20.json"));
        let flash = spells.iter().find(|s| s.id == "SummonerFlash").unwrap();
        assert_eq!((flash.name.as_str(), flash.cooldown), ("Flash", 300.0));
    }

    #[test]
    fn champion_file() {
        let j = r#"{"data":{"Caitlyn":{"spells":[{"cooldown":[10]},{"cooldown":[20]},{"cooldown":[16]},{"cooldown":[90,75,60]}]}}}"#;
        assert_eq!(parse_ult_cooldown(j, "Caitlyn"), Some(vec![90.0, 75.0, 60.0]));
        assert_eq!(parse_ult_cooldown("{}", "Caitlyn"), None);
    }

    /// Real Data Dragon (network): `cargo test -p cv-game-league -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn live_data_dragon() {
        let d = std::env::temp_dir().join(format!("cv-dd-{}", std::process::id()));
        let cd = ult_cooldown(Some(&d), None, "Caitlyn").await.expect("cooldowns");
        assert_eq!(cd.len(), 3);
        assert!(cd[0] > cd[2]);
        // Second time from the cache.
        assert_eq!(ult_cooldown(Some(&d), None, "Caitlyn").await, Some(cd));
    }
}
