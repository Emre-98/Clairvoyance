//! Plays a scripted match against the fake Live Client API over real HTTP.

use cv_core::game::{GameIntegration, GameResult, MatchPhase, ModeRules};
use cv_core::EventKind;
use cv_game_league::LeagueIntegration;

#[tokio::test(flavor = "multi_thread")]
async fn full_mock_match() {
    let opts = cv_mock_league::MockOptions { port: 29981, speed: 120.0, length: 600.0, loading_secs: 1.0, linger_secs: 2.0, ..Default::default() };
    let mock = cv_mock_league::spawn(opts).unwrap();
    let mut lol = LeagueIntegration::new();
    lol.configure(&serde_json::json!({ "api_base": mock.base_url, "riot_id": "" }));
    lol.start().await.unwrap();

    let mut saw_loading_or_waiting = false;
    let mut events = Vec::new();
    let mut result = None;
    let mut player = None;
    for _ in 0..200 {
        let u = lol.poll().await.unwrap();
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
