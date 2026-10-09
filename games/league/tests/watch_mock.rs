//! v1.7.1: replays and spectating, end to end against the fake League (client + game API over
//! real HTTP, answering like the real ones did in a captured replay).

use cv_core::game::{GameIntegration, SessionCheck, WatchDetection, WatchKind};
use cv_game_league::LeagueIntegration;
use cv_mock_league::{MockOptions, Watch};
use std::time::Duration;

async fn run(watch: Watch, port: u16, spectating_setting: bool) -> (SessionCheck, Option<WatchKind>, bool) {
    let opts = MockOptions { port, speed: 10.0, length: 300.0, loading_secs: 1.0, linger_secs: 30.0, watch, ..Default::default() };
    run_opts(opts, spectating_setting).await
}

async fn run_opts(opts: MockOptions, spectating_setting: bool) -> (SessionCheck, Option<WatchKind>, bool) {
    let port = opts.port;
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
    let (c, w, _) = run(Watch::ReplayUnsure, 3026, false).await;
    assert_eq!(c, SessionCheck::Unsure(WatchKind::Unknown));
    assert_eq!(w, Some(WatchKind::Unknown));
}

#[tokio::test]
async fn the_clients_word_that_you_play_is_final() {
    // The client lists your account among the match's players; the in-game API stays in
    // spectator mode. Never taken for a replay / spectating (before: deleted a few seconds in).
    let (c, w, _) = run(Watch::ReplayLate, 3023, false).await;
    assert_eq!(c, SessionCheck::Playing, "the client reports your match");
    assert_eq!(w, None, "the in-game API can't make it spectating");
}

#[tokio::test]
async fn spectator_answer_before_the_champion_spawns_isnt_spectating() {
    // Owner's PC, 2026-10-09 (first ranked game after switching accounts): the in-game API
    // came up 20 ms before the champions were spawned and answered "spectator mode" once; the
    // recording was deleted. Here that window is 0.5 s (several reads at this test's pace).
    let opts = |port, watch| MockOptions { port, speed: 10.0, length: 300.0, loading_secs: 1.0, linger_secs: 30.0, watch, no_champion_secs: 0.5, ..Default::default() };
    // Your match, the client lists you: recorded from the start, never "watching".
    let (c, w, p) = run_opts(opts(3031, Watch::None), false).await;
    assert_eq!(c, SessionCheck::Playing);
    assert_eq!(w, None, "one early answer isn't spectator mode");
    assert!(p, "then the in-game API shows your champion");
    // Your match, the client can't show its players: held, and the hold ends in "playing".
    let (c, w, p) = run_opts(opts(3032, Watch::MatchUnreadable), false).await;
    assert_eq!(c, SessionCheck::Unsure(WatchKind::Unknown));
    assert_eq!(w, None);
    assert!(p);
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

#[tokio::test]
async fn players_the_client_cant_show_wait_for_the_game() {
    // v1.8.1: the client reports your match but its players can't be read: nothing is recorded
    // until the in-game API says spectator mode (never as "spectating": could be a replay)...
    let (c, w, p) = run(Watch::SpectateUnreadable, 3028, false).await;
    assert_eq!(c, SessionCheck::Unsure(WatchKind::Unknown));
    assert_eq!(w, Some(WatchKind::Unknown));
    assert!(!p);
    // ... or your champion, and recording starts.
    let (c, w, p) = run(Watch::MatchUnreadable, 3029, false).await;
    assert_eq!(c, SessionCheck::Unsure(WatchKind::Unknown));
    assert_eq!(w, None);
    assert!(p, "the in-game API confirms your champion");
}

#[tokio::test]
async fn no_league_client_waits_for_the_game() {
    // The client can't be found (no lockfile): the in-game API decides before anything is recorded.
    let opts = MockOptions { port: 3030, speed: 10.0, length: 300.0, loading_secs: 1.0, linger_secs: 30.0, watch: Watch::Spectate, ..Default::default() };
    let mock = cv_mock_league::spawn(opts).unwrap();
    let dir = std::env::temp_dir().join(format!("cv-watch-noclient-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut l = LeagueIntegration::new();
    l.configure(&serde_json::json!({ "api_base": mock.base_url, "client_dir": dir.to_string_lossy() }));
    l.start().await.unwrap();
    assert_eq!(l.session_check().await, SessionCheck::Unsure(WatchKind::Unknown));
    let mut watching = None;
    for _ in 0..40 {
        if let Ok(u) = l.poll().await {
            if u.watching.is_some() {
                watching = u.watching;
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    mock.stop();
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(watching, Some(WatchKind::Unknown));
}
