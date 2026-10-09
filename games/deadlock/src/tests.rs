use super::*;
use std::io::Write;
use std::path::Path;

fn append(path: &Path, line: &str) {
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path).unwrap();
    writeln!(f, "{line}").unwrap();
}

/// A line of Steam's log / the console log, stamped `secs_ago` seconds before now.
fn steam_line(secs_ago: i64, text: &str) -> String {
    format!("[{}] {text}", (now() - chrono::Duration::seconds(secs_ago)).format("%Y-%m-%d %H:%M:%S"))
}
fn console_line(text: &str) -> String {
    format!("{} {text}", now().format("%m/%d %H:%M:%S"))
}

/// The module against real files: a Steam folder with Steam's log and Deadlock's console log.
#[tokio::test]
async fn a_match_from_the_logs_to_the_engine() {
    let steam = std::env::temp_dir().join(format!("cv-deadlock-game-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&steam);
    let citadel = steam.join("steamapps").join("common").join("Deadlock").join("game").join("citadel");
    std::fs::create_dir_all(&citadel).unwrap();
    std::fs::create_dir_all(steam.join("logs")).unwrap();
    let (content, console) = (steam.join("logs").join("content_log.txt"), citadel.join("console.log"));
    // Yesterday's match is in the log: over long ago.
    append(&content, &steam_line(90_000, "App 1422450 updates disabled for 300 seconds (until whenever)"));
    append(&content, &steam_line(88_000, "App 1422450 updates now enabled"));
    append(&content, &steam_line(60, "AppID 1422450 state changed : Fully Installed,App Running,"));

    let mut game = DeadlockIntegration::new();
    assert!(game.match_only() && game.supports_events());
    game.configure(&serde_json::json!({ "steam_folder": steam.to_string_lossy() }));
    game.start().await.unwrap();
    let u = game.poll().await.unwrap();
    assert_eq!((u.phase, u.game_time), (MatchPhase::Waiting, None), "in the Hideout");

    // A match is found: Steam says so, with nothing set up.
    append(&content, &steam_line(0, "App 1422450 updates disabled for 300 seconds (until whenever)"));
    let u = game.poll().await.unwrap();
    assert_eq!(u.phase, MatchPhase::Loading);
    assert!(u.game_time.is_some_and(|t| (0.0..5.0).contains(&t)), "{:?}", u.game_time);
    assert_eq!(game.end_grace(), Duration::from_secs(8), "no console log: a tail for the end screen");

    // With -condebug the console log joins in.
    append(&console, &console_line("Lobby 174168628287162241 for Match 113291198 created"));
    append(&console, &console_line("OnGameStateChanged: GameInProgress (7)"));
    assert_eq!(game.poll().await.unwrap().phase, MatchPhase::InProgress);
    assert_eq!(game.end_grace(), Duration::from_secs(2));

    // Your keys during the match, with the game's default binds (this Steam folder has no binds file).
    let key = |k: &str| cv_core::game::KeyPress { key: k.into(), ctrl: false, shift: false, alt: false };
    let ult = game.on_key(&key("4"), 75.0).expect("the ultimate");
    assert_eq!((ult.kind, ult.title.as_str()), (EventKind::UltPressed, "Ultimate"));
    assert!(game.on_key(&key("W"), 76.0).is_none());
    assert_eq!(game.take_key_marks().iter().map(|m| m.action.as_str()).collect::<Vec<_>>(), ["ult"]);

    // The engine starts the module again when the recording starts: the match is still known.
    game.start().await.unwrap();
    let u = game.poll().await.unwrap();
    assert_eq!(u.phase, MatchPhase::InProgress);
    assert!(u.events.is_empty());

    // End banner: the end screen still belongs to the match, until it is left.
    append(&content, &steam_line(0, "App 1422450 updates now enabled"));
    append(&console, &console_line("OnGameStateChanged: PostGame (8)"));
    assert_eq!(game.poll().await.unwrap().phase, MatchPhase::InProgress);
    append(&console, &console_line("OnGameStateChanged: End (11)"));
    let u = game.poll().await.unwrap();
    assert_eq!(u.phase, MatchPhase::Ended);
    assert_eq!(u.events.iter().map(|e| (e.kind, e.title.as_str())).collect::<Vec<_>>(), [(EventKind::GameEnd, "Match over")]);
    let u = game.poll().await.unwrap();
    assert!(u.phase == MatchPhase::Ended && u.events.is_empty(), "the end is told once");

    // The recording ended: back to waiting, also after starting anew (the match is history).
    game.stop().await;
    assert_eq!(game.poll().await.unwrap().phase, MatchPhase::Waiting);
    game.start().await.unwrap();
    assert_eq!(game.poll().await.unwrap().phase, MatchPhase::Waiting);

    // No Steam folder: nothing is detected, nothing breaks.
    let mut lost = DeadlockIntegration::new();
    lost.configure(&serde_json::json!({ "steam_folder": steam.join("nowhere").to_string_lossy() }));
    lost.start().await.unwrap();
    assert_eq!(lost.poll().await.unwrap().phase, MatchPhase::Waiting);
    std::fs::remove_dir_all(steam).ok();
}
