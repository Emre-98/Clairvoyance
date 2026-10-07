//! Plays a scripted match against the fake Live Client API over real HTTP.

use cv_core::game::{GameIntegration, GameResult, MatchPhase, ModeRules};
use cv_core::EventKind;
use cv_game_league::LeagueIntegration;

#[tokio::test(flavor = "multi_thread")]
async fn full_mock_match() {
    let opts = cv_mock_league::MockOptions { port: 29981, speed: 60.0, length: 600.0, loading_secs: 1.0, linger_secs: 2.0, ..Default::default() };
    let mock = cv_mock_league::spawn(opts).unwrap();
    let mut lol = LeagueIntegration::new();
    lol.configure(&serde_json::json!({ "api_base": mock.base_url, "riot_id": "" }));
    lol.start().await.unwrap();

    let mut saw_loading_or_waiting = false;
    let mut events = Vec::new();
    let mut result = None;
    let mut player = None;
    let mut sb = cv_core::scoreboard::Scoreboard::default();
    let mut reads = 0;
    for _ in 0..400 {
        let u = lol.poll().await.unwrap();
        if let Some(r) = u.scoreboard {
            reads += 1;
            sb.record(r);
        }
        if u.phase == MatchPhase::Waiting {
            saw_loading_or_waiting = true;
        }
        events.extend(u.events);
        if u.player.is_some() {
            player = u.player;
        }
        if u.phase == MatchPhase::Ended {
            result = u.result;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(saw_loading_or_waiting);
    assert_eq!(result, Some(GameResult::Win));
    let p = player.expect("found myself automatically via activeplayername");
    assert_eq!(p.character.as_deref(), Some("Ahri"));
    assert_eq!(p.mode.as_deref(), Some("Summoner's Rift"));
    let count = |k| events.iter().filter(|e| e.kind == k).count();
    assert_eq!(count(EventKind::Kill), 4);
    assert_eq!(count(EventKind::Death), 2);
    assert_eq!(count(EventKind::Assist), 2);
    assert_eq!(count(EventKind::Multikill), 1);
    assert_eq!(count(EventKind::FirstBlood), 1);
    assert_eq!(count(EventKind::Herald), 1);
    assert!(events.iter().any(|e| e.kind == EventKind::Herald && e.steal));
    assert_eq!(count(EventKind::Dragon), 1);
    assert_eq!(count(EventKind::Baron), 1);
    assert_eq!(count(EventKind::Tower), 1);
    assert_eq!(count(EventKind::Inhibitor), 1);
    assert_eq!(count(EventKind::Ace), 1);
    assert_eq!(count(EventKind::GameEnd), 1);
    // No duplicates even though the API returns every event on every call.
    let mut ids: Vec<_> = events.iter().map(|e| e.id.clone()).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), events.len());
    // The scoreboard: all 10 players, changes only, the state at any time.
    assert_eq!(sb.players.len(), 10);
    assert_eq!(sb.players.iter().filter(|p| p.me).count(), 1);
    assert_eq!(sb.players.iter().find(|p| p.me).unwrap().character, "Ahri");
    assert_eq!(sb.players[1].character_id, "LeeSin");
    assert!(reads >= 5 && sb.frames.len() <= reads, "{reads} reads, {} frames", sb.frames.len());
    let me = sb.players.iter().position(|p| p.me).unwrap();
    let last = sb.last();
    assert_eq!((last[me].kills, last[me].deaths, last[me].assists), (4, 2, 2), "final KDA");
    assert_eq!(last[me].spells, vec!["SummonerFlash".to_string(), "SummonerDot".to_string()]);
    assert_eq!(sb.names.get("SummonerFlash").map(|s| s.as_str()), Some("Flash"));
    let early = sb.state_at(sb.frames[0].t);
    assert!(early[5].items.len() < last[5].items.len() && early[5].level < last[5].level, "others buy and level over the game: {:?} -> {:?}", early[5], last[5]);
    assert!(sb.state_at(300.0)[me].kills < last[me].kills, "mid-game KDA is smaller");
    let json = serde_json::to_string(&sb).unwrap();
    println!("scoreboard: {} reads, {} frames, {} bytes", reads, sb.frames.len(), json.len());
    mock.stop();
}

/// The fake League client (lockfile + gameflow session + queue list): the exact queue is read
/// before the game, and the live queue list is used for the mode catalog.
#[tokio::test(flavor = "multi_thread")]
async fn mode_from_the_fake_league_client() {
    for (port, id, key, name, group) in [
        (29982u16, 450i64, "q450", "ARAM", "aram"),
        (29983, 420, "q420", "Ranked Solo/Duo", "ranked"),
        (29984, 9999, "q9999", "Brand New Mode", "rotating"),
        (29985, -1, "practice", "Practice Tool", "other"),
    ] {
        let opts = cv_mock_league::MockOptions { port, speed: 60.0, length: 120.0, loading_secs: 30.0, queue: cv_mock_league::queue_json(id), ..Default::default() };
        let mock = cv_mock_league::spawn(opts).unwrap();
        let dir = std::env::temp_dir().join(format!("cv-lcu-{port}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("lockfile"), mock.lockfile_text()).unwrap();
        let mut lol = LeagueIntegration::new();
        lol.configure(&serde_json::json!({ "api_base": mock.base_url, "riot_id": "", "client_dir": dir.to_string_lossy() }));
        // During the "loading screen" (the in-game API isn't up yet): the client answers.
        let m = lol.detect_mode().await.expect("mode detected");
        assert_eq!(m.key.as_deref(), Some(key), "{m:?}");
        assert_eq!(m.name, name);
        assert_eq!(m.group, group);
        assert!(m.source.contains("client"), "{}", m.source);
        let (catalog, live) = lol.mode_catalog(&dir).await;
        assert!(live, "live queue list from the fake client");
        assert!(catalog.iter().any(|c| c.key == "q450" && c.available == Some(true)));
        mock.stop();
    }
}

/// Completed-item chips against the fake game's shop script (`cv_mock_league::SHOP`): buy and
/// undo, buy again, a confirmed item undone, a sale. Slowed down (8 game seconds per real
/// second) so the polls see every step. Data Dragon comes from a prepared cache (no network).
#[tokio::test(flavor = "multi_thread")]
async fn shop_buy_undo_buy_again_and_sell() {
    let dir = std::env::temp_dir().join(format!("cv-shop-{}", std::process::id()));
    let v = dir.join("ddragon").join("16.20.1");
    std::fs::create_dir_all(&v).unwrap();
    std::fs::write(dir.join("ddragon").join("versions.json"), r#"["16.20.1"]"#).unwrap();
    let data = cv_game_league::ddragon::StaticData {
        version: "16.20.1".into(),
        items: cv_game_league::ddragon::parse_items(include_str!("ddragon/items-16.20.json")),
        spells: cv_game_league::ddragon::parse_spells(include_str!("ddragon/summoner-16.20.json")),
    };
    std::fs::write(v.join("static-v1.json"), serde_json::to_string(&data).unwrap()).unwrap();
    cv_game_league::set_data_dir(dir.clone());

    let opts = cv_mock_league::MockOptions { port: 29986, speed: 8.0, length: 600.0, loading_secs: 0.5, linger_secs: 1.0, ..Default::default() };
    let mock = cv_mock_league::spawn(opts).unwrap();
    let mut lol = LeagueIntegration::new();
    lol.configure(&serde_json::json!({ "api_base": mock.base_url, "riot_id": "" }));
    lol.start().await.unwrap();
    let mut chips: Vec<cv_core::GameEvent> = Vec::new();
    let mut log = Vec::new();
    let t0 = std::time::Instant::now();
    loop {
        let u = lol.poll().await.unwrap();
        for e in u.events.iter().filter(|e| e.kind == EventKind::ItemCompleted) {
            log.push(format!("{:.1} + {}", u.game_time.unwrap_or(0.0), e.title));
            chips.push(e.clone());
        }
        for id in &u.removed {
            log.push(format!("{:.1} - {id}", u.game_time.unwrap_or(0.0)));
            chips.retain(|e| &e.id != id);
        }
        if u.game_time.unwrap_or(0.0) > 75.0 || t0.elapsed().as_secs() > 30 {
            break;
        }
        // The engine polls every second; faster here, as the clock runs 8× faster.
        tokio::time::sleep(std::time::Duration::from_millis(120)).await;
    }
    mock.stop();
    println!("{log:#?}");
    let titles: Vec<&str> = chips.iter().map(|e| e.title.as_str()).collect();
    assert_eq!(titles, vec!["Completed Rabadon's Deathcap"], "buy+undo: none; buy again: one; Infinity Edge undone after its chip: withdrawn; sale: kept. Log: {log:?}");
    let r = &chips[0];
    assert!((r.game_time - 30.0).abs() <= 1.5, "at the purchase (30 s), not later: {}", r.game_time);
    assert!(log.iter().any(|l| l.contains("+ Completed Infinity Edge")), "Infinity Edge got a chip first: {log:?}");
    assert!(log.iter().any(|l| l.contains("- item-3031")), "then it was withdrawn: {log:?}");
    assert_eq!(log.iter().filter(|l| l.contains("Rabadon")).count(), 1, "one Rabadon chip: {log:?}");
    std::fs::remove_dir_all(dir).ok();
}
