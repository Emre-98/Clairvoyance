//! Data Dragon (Riot's public static data): the ult's base cooldown per rank for the game's
//! patch, cached on disk per patch. Fetched in the background at the loading screen; never
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

fn cache_file(dir: &Path, version: &str, champ: &str) -> PathBuf {
    let safe: String = champ.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    dir.join("ddragon").join(version).join(format!("{safe}-R.json"))
}

/// Base cooldowns of `champ`'s ult for `patch`, from the disk cache or Data Dragon.
pub async fn ult_cooldown(cache_dir: Option<&Path>, patch: Option<&str>, champ: &str) -> Option<Vec<f64>> {
    let client = reqwest::Client::builder().timeout(Duration::from_secs(8)).user_agent("Clairvoyance").build().ok()?;
    // The version list (cached for a day).
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
    let version = pick_version(&versions, patch)?;
    if let Some(d) = cache_dir {
        if let Some(cd) = std::fs::read_to_string(cache_file(d, &version, champ)).ok().and_then(|t| serde_json::from_str::<Vec<f64>>(&t).ok()) {
            return Some(cd);
        }
    }
    let url = format!("https://ddragon.leagueoflegends.com/cdn/{version}/data/en_US/champion/{champ}.json");
    let text = client.get(&url).send().await.ok()?.error_for_status().ok()?.text().await.ok()?;
    let cd = parse_ult_cooldown(&text, champ)?;
    if let Some(d) = cache_dir {
        let f = cache_file(d, &version, champ);
        let _ = std::fs::create_dir_all(f.parent().unwrap());
        let _ = std::fs::write(f, serde_json::to_string(&cd).unwrap_or_default());
    }
    log::info!("ult cooldown of {champ} (Data Dragon {version}): {cd:?}");
    Some(cd)
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
