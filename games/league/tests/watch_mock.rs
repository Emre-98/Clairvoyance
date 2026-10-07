//! v1.7.1: replays and spectating, end to end against the fake League (client + game API over
//! real HTTP, answering like the real ones did in a captured replay).

use cv_core::game::{GameIntegration, SessionCheck, WatchDetection, WatchKind};
use cv_game_league::LeagueIntegration;
use cv_mock_league::{MockOptions, Watch};
use std::time::Duration;

async fn run(watch: Watch, port: u16, spectating_setting: bool) -> (SessionCheck, Option<WatchKind>, bool) {
    let opts = MockOptions { port, speed: 10.0, length: 300.0, loading_secs: 1.0, linger_secs: 30.0, watch, ..Default::default() };
    let mock = cv_mock_league::spawn(opts).unwrap();
    let dir = std::env::temp_dir().join(format!("cv-watch-{port}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("lockfile"), mock.lockfile_text()).unwrap();
    let mut l = LeagueIntegration::new();
    l.configure(&serde_json::json!({ "api_base": mock.base_url, "client_dir": dir.to_string_lossy(), "record_spectating": spectating_setting }));
    l.start().await.unwrap();
    let check = l.session_check().await;
    // The in-game API: answers after the "loading screen".
    let mut watching = None;
    let mut playing = false;
    for _ in 0..40 {
        if let Ok(u) = l.poll().await {
            if u.watching.is_some() {
                watching = u.watching;
                break;
            }
            if u.playing {
                playing = true;
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    mock.stop();
    std::fs::remove_dir_all(&dir).ok();
    (check, watching, playing)
}

#[tokio::test]
async fn replay_is_seen_by_the_client_and_the_game() {
    let (c, w, p) = run(Watch::Replay, 3021, false).await;
    assert_eq!(c, SessionCheck::Watching(WatchKind::Replay));
    assert_eq!(w, Some(WatchKind::Replay), "the in-game API says spectator mode too");
    assert!(!p);
}

#[tokio::test]
async fn spectating_is_seen_by_the_client_and_the_game() {
    let (c, w, _) = run(Watch::Spectate, 3022, false).await;
    assert_eq!(c, SessionCheck::Watching(WatchKind::Spectate));
    assert_eq!(w, Some(WatchKind::Spectate));
    // The setting doesn't change what's detected (the engine decides what to record).
    let (c, _, _) = run(Watch::Spectate, 3025, true).await;
    assert_eq!(c, SessionCheck::Watching(WatchKind::Spectate));
}

#[tokio::test]
async fn replay_the_client_hides_is_found_by_the_game() {
    let (c, w, _) = run(Watch::ReplayLate, 3023, false).await;
    assert_eq!(c, SessionCheck::Playing, "the client reports a match");
    assert_eq!(w, Some(WatchKind::Unknown), "the in-game API reveals spectator mode");
    let (c, w, _) = run(Watch::ReplayUnsure, 3026, false).await;
    assert_eq!(c, SessionCheck::Unsure(WatchKind::Unknown));
    assert_eq!(w, Some(WatchKind::Unknown));
}

#[tokio::test]
async fn a_normal_match_is_a_match() {
    let (c, w, p) = run(Watch::None, 3024, false).await;
    assert_eq!(c, SessionCheck::Playing);
    assert_eq!(w, None);
    assert!(p, "the in-game API confirms your champion");
}

#[tokio::test]
async fn spectating_a_friend_the_client_calls_a_match_waits_for_the_game() {
    // The client reports it like your own match (owner's PC, 2026-10-03), but you aren't one of
    // its players: nothing is recorded until the in-game API says spectator mode.
    let (c, w, p) = run(Watch::SpectateLive, 3027, false).await;
    assert_eq!(c, SessionCheck::Unsure(WatchKind::Spectate));
    assert_eq!(w, Some(WatchKind::Spectate));
    assert!(!p);
}
