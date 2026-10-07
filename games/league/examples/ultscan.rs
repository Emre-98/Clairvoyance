//! Developer tool: scans every champion's R in Data Dragon for recast / command / charges /
//! transform wording and compares the result with `ult_rules.json` (re-run each patch).
//!
//!   cargo run -p cv-game-league --example ultscan                 newest patch from Data Dragon
//!   cargo run -p cv-game-league --example ultscan -- <championFull.json>
//!   ... -- --save        also writes tests/ddragon/r-spells.json (the unit tests' fixture)
//!
//! Prints one line per champion whose text looks special or that the rules file lists, with a
//! status: OK, NEW (looks like a recast but isn't in the rules: check it on the League wiki and
//! add it), TEXT? (listed, but the text shows nothing: keep only if checked elsewhere), and the
//! champions in the rules file Data Dragon doesn't know. Exit code 1 if there's anything to do.
use cv_game_league::ddragon;
use cv_game_league::ult::UltRules;
use cv_game_league::ultkind::{classify_text, UltKind};

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let save = args.iter().any(|a| a == "--save");
    let file = args.iter().find(|a| !a.starts_with("--"));
    let full: serde_json::Value = match file {
        Some(f) => serde_json::from_str(&std::fs::read_to_string(f)?)?,
        None => {
            let client = reqwest::Client::builder().user_agent("Clairvoyance-ultscan").build()?;
            let v = ddragon::version_for(&client, None, None).await.ok_or_else(|| anyhow::anyhow!("no Data Dragon version"))?;
            eprintln!("Data Dragon {v}");
            client
                .get(format!("https://ddragon.leagueoflegends.com/cdn/{v}/data/en_US/championFull.json"))
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?
        }
    };
    let version = full["version"].as_str().unwrap_or("?").to_string();
    let data = full["data"].as_object().ok_or_else(|| anyhow::anyhow!("not a championFull.json"))?;
    let rules = UltRules::builtin();
    let mut todo = 0;
    let mut fixture = serde_json::Map::new();
    println!("Data Dragon {version}: {} champions", data.len());
    println!("{:<14} {:<17} {:<17} {:<7} keywords", "champion", "text looks", "rules file", "status");
    let mut names: Vec<&String> = data.keys().collect();
    names.sort();
    for name in names {
        let c = &data[name];
        let Some(u) = ddragon::ult_of(c) else { continue };
        fixture.insert(name.clone(), serde_json::json!({ "name": u.name, "cooldown": u.cooldown, "maxammo": u.max_ammo.map(|a| a.to_string()), "description": u.description, "tooltip": u.tooltip }));
        let (guess, hits) = classify_text(&u.description, &u.tooltip, u.max_ammo, &u.cooldown);
        let listed = rules.kind_rule(name);
        if guess.is_none() && listed.is_none() {
            continue;
        }
        let short_cd = u.cooldown.iter().all(|c| *c <= 10.0);
        let status = match (guess, listed) {
            (Some(UltKind::MultiCast | UltKind::Command), None) => "NEW",
            (Some(_), None) => "check",
            (None, Some(l)) if l.kind != UltKind::Normal && !l.note.contains("wiki") && !(short_cd && matches!(l.kind, UltKind::ChargesOrReset | UltKind::Transform)) => "TEXT?",
            _ => "OK",
        };
        if status == "NEW" || status == "TEXT?" {
            todo += 1;
        }
        println!(
            "{:<14} {:<17} {:<17} {:<7} {}  cd {:?}{}",
            name,
            guess.map(|k| k.as_str()).unwrap_or("-"),
            listed.map(|l| l.kind.as_str()).unwrap_or("-"),
            status,
            hits.join(", "),
            u.cooldown,
            u.max_ammo.filter(|a| *a > 0).map(|a| format!(", {a} charges")).unwrap_or_default()
        );
    }
    for name in rules.kinds.keys() {
        if !data.keys().any(|k| k.eq_ignore_ascii_case(name)) {
            println!("{name:<14} not in Data Dragon (renamed or removed?)");
            todo += 1;
        }
    }
    if save {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/ddragon/r-spells.json");
        let out = serde_json::json!({ "version": version, "source": "Data Dragon championFull.json (en_US), R spells only; made by `cargo run -p cv-game-league --example ultscan -- --save`", "champions": fixture });
        std::fs::write(&p, serde_json::to_string_pretty(&out)?)?;
        println!("wrote {}", p.display());
    }
    println!("{todo} to check");
    std::process::exit(if todo > 0 { 1 } else { 0 });
}
