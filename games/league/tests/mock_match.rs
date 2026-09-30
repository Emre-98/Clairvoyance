//! Plays a scripted match against the fake Live Client API over real HTTP.

use gr_core::game::{GameIntegration, GameResult, MatchPhase};
use gr_core::EventKind;
use gr_game_league::LeagueIntegration;

#[tokio::test(flavor = "multi_thread")]
async fn full_mock_match() {
    let opts = gr_mock_league::MockOptions { port: 29981, speed: 120.0, length: 600.0, loading_secs: 1.0, linger_secs: 2.0, ..Default::default() };
    let mock = gr_mock_league::spawn(opts).unwrap();
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
