use super::*;

// The owner's real log lines of 2026-10-09 (see the files' first lines for what was played).
const RUN1: &str = include_str!("../../tests/fixtures/console-run1.log");
const RUN2: &str = include_str!("../../tests/fixtures/console-run2.log");
const STEAM: &str = include_str!("../../tests/fixtures/steam-content-log.txt");

fn at(hms: &str) -> NaiveDateTime {
    NaiveDateTime::parse_from_str(&format!("2026-10-09 {hms}"), "%Y-%m-%d %H:%M:%S").unwrap()
}

/// Every signal of the chosen logs, in time order (Steam's first within a second).
fn timeline(steam: bool, console: bool) -> Vec<(NaiveDateTime, Signal)> {
    let mut v: Vec<(NaiveDateTime, Signal)> = Vec::new();
    if steam {
        v.extend(STEAM.lines().filter_map(parse_steam));
    }
    if console {
        v.extend(RUN1.lines().chain(RUN2.lines()).filter_map(|l| parse_console(l, 2026)));
    }
    v.sort_by_key(|(t, _)| *t);
    v
}

/// Plays the signals the way the engine uses the module: a recording runs while the phase is
/// Loading / InProgress and ends at Ended (the module is then stopped). Returns each recording
/// as (start, end, match id).
fn recordings(signals: Vec<(NaiveDateTime, Signal)>) -> Vec<(String, String, Option<String>)> {
    let hms = |t: NaiveDateTime| t.format("%H:%M:%S").to_string();
    let mut d = Detector::default();
    let mut out = Vec::new();
    let mut start = None;
    for (t, signal) in signals {
        d.feed(t, signal);
        d.settle(t);
        match d.phase(t) {
            MatchPhase::Loading | MatchPhase::InProgress => start = start.or(Some(t)),
            MatchPhase::Ended => {
                let m = d.current().unwrap();
                out.push((hms(start.take().unwrap_or(m.found)), hms(m.over.unwrap()), m.id.clone()));
                d.clear();
            }
            MatchPhase::Waiting => assert_eq!(start, None, "the match vanished without ending at {t}"),
        }
    }
    assert_eq!(start, None, "a recording never ended");
    out
}

fn rec(start: &str, end: &str, id: Option<&str>) -> (String, String, Option<String>) {
    (start.to_string(), end.to_string(), id.map(str::to_string))
}

#[test]
fn steam_log_lines() {
    assert_eq!(parse_steam("[2026-10-09 09:28:21] App 1422450 updates disabled for 300 seconds (until Fri Oct  9 09:33:21 2026)"), Some((at("09:28:21"), Signal::UpdatesDisabled)));
    assert_eq!(parse_steam("[2026-10-09 10:09:07] App 1422450 updates now enabled"), Some((at("10:09:07"), Signal::UpdatesEnabled)));
    assert_eq!(parse_steam("[2026-10-09 10:10:54] AppID 1422450 state changed : Fully Installed,"), Some((at("10:10:54"), Signal::Closed)));
    // Still running, another game, other lines, rubbish.
    assert_eq!(parse_steam("[2026-10-09 10:09:07] AppID 1422450 state changed : Fully Installed,App Running,"), None);
    assert_eq!(parse_steam("[2026-10-09 09:28:21] AppID 1422450 state changed : Fully Installed,App Running,Updated Disabled by app,"), None);
    assert_eq!(parse_steam("[2026-10-09 09:28:21] App 730 updates disabled for 300 seconds (until Fri Oct  9 09:33:21 2026)"), None);
    assert_eq!(parse_steam("[2026-10-09 08:05:00] stats: (SteamCache, 401) cache10-ams1.steamcontent.com: 3500204048 Bytes"), None);
    assert_eq!(parse_steam("# a comment"), None);
    assert_eq!(parse_steam(""), None);
}

#[test]
fn console_log_lines() {
    let p = |l: &str| parse_console(l, 2026);
    assert_eq!(p("10/09 09:28:20 Lobby 174168628287162241 for Match 113291198 created"), Some((at("09:28:20"), Signal::LobbyCreated("113291198".into()))));
    assert_eq!(p("10/09 09:29:01 OnGameStateChanged: GameInProgress (7)"), Some((at("09:29:01"), Signal::State(7))));
    assert_eq!(p("10/09 10:09:07 OnGameStateChanged: PostGame (8)\r"), Some((at("10:09:07"), Signal::State(8))));
    assert_eq!(p("10/09 10:09:17 OnGameStateChanged: End (11)"), Some((at("10:09:17"), Signal::State(11))));
    assert_eq!(p("10/09 10:09:17 [Client] CL:  disconnect"), Some((at("10:09:17"), Signal::Disconnect)));
    assert_eq!(p("10/09 10:09:20 [Client] Map: \"dl_hideout\""), Some((at("10:09:20"), Signal::Hideout)));
    assert_eq!(p("10/09 09:26:12 Source2Init OK"), Some((at("09:26:12"), Signal::Closed)));
    // The local server's own state lines (Hideout, sandbox), the lobby going away, other lines.
    assert_eq!(p("10/09 09:26:22 ChangeGameState: GameInProgress (7)"), None);
    assert_eq!(p("10/09 10:09:08 Lobby 174168628287162241 for Match 113291198 destroyed"), None);
    assert_eq!(p("10/09 09:28:21 [Client] Map: \"start\""), None);
    assert_eq!(p("10/09 09:26:14 [Client] Disconnecting from server: NETWORK_DISCONNECT_LOOPDEACTIVATE"), None);
    assert_eq!(p("[2026-10-09 09:50:27] GameOverlay: started"), None);
    assert_eq!(p("short"), None);
}

#[test]
fn steam_alone_gives_one_recording_per_match() {
    // No -condebug: found -> end banner (run 1), found -> the moment it was left (run 2).
    assert_eq!(recordings(timeline(true, false)), [rec("09:28:21", "10:09:07", None), rec("10:15:44", "10:17:12", None)]);
}

#[test]
fn with_the_console_log_the_end_screen_is_included_and_the_match_is_named() {
    let both = recordings(timeline(true, true));
    // Run 1: until the end screen was left (10 s after the banner). Run 2: left early.
    assert_eq!(both, [rec("09:28:20", "10:09:17", Some("113291198")), rec("10:15:44", "10:17:12", Some("113300990"))]);
    // The console log alone tells the same (Steam's log isn't needed when it's there).
    assert_eq!(recordings(timeline(false, true)), both);
}

#[test]
fn hideout_sandbox_and_spectating_never_look_like_a_match() {
    // Everything outside the two matches: the Hideout (09:26, 10:09, 10:13, ...), the sandbox
    // (10:14:13-10:15:13) and spectating a live match (10:18:37-10:19:51) all print
    // "OnGameStateChanged: GameInProgress (7)".
    let mut d = Detector::default();
    let mut in_progress_lines = 0;
    for (t, signal) in timeline(true, true) {
        let outside = t < at("09:28:20") || (t > at("10:09:17") && t < at("10:15:44")) || t > at("10:17:12");
        in_progress_lines += usize::from(outside && signal == Signal::State(7));
        d.feed(t, signal);
        d.settle(t);
        if d.phase(t) == MatchPhase::Ended {
            d.clear();
        }
        if outside {
            assert_eq!(d.phase(t), MatchPhase::Waiting, "at {t}");
        }
    }
    assert!(in_progress_lines >= 6, "the fixtures hold the misleading lines ({in_progress_lines})");
}

#[test]
fn phases_of_a_match() {
    // With the console log: loading until the server says in progress; the end screen still
    // counts as the match.
    let mut d = Detector::default();
    d.feed(at("09:28:20"), Signal::LobbyCreated("113291198".into()));
    d.feed(at("09:28:21"), Signal::UpdatesDisabled);
    assert_eq!(d.phase(at("09:28:59")), MatchPhase::Loading);
    d.feed(at("09:29:01"), Signal::State(7));
    assert_eq!(d.phase(at("09:29:01")), MatchPhase::InProgress);
    d.feed(at("10:09:07"), Signal::UpdatesEnabled);
    d.feed(at("10:09:07"), Signal::State(8));
    d.settle(at("10:09:16"));
    assert_eq!(d.phase(at("10:09:16")), MatchPhase::InProgress);
    let m = d.current().unwrap();
    assert_eq!((m.found, m.in_progress, m.post_game, m.over), (at("09:28:20"), Some(at("09:29:01")), Some(at("10:09:07")), None));
    d.feed(at("10:09:17"), Signal::State(11));
    assert_eq!(d.phase(at("10:09:17")), MatchPhase::Ended);

    // Without it: in progress after the usual loading time.
    let mut d = Detector::default();
    d.feed(at("09:28:21"), Signal::UpdatesDisabled);
    assert_eq!(d.phase(at("09:28:22")), MatchPhase::Loading);
    assert_eq!(d.phase(at("09:29:01")), MatchPhase::InProgress);
    d.feed(at("09:34:21"), Signal::UpdatesDisabled);
    assert_eq!(d.current().unwrap().found, at("09:28:21"), "a renewal isn't a new match");
    d.feed(at("10:09:07"), Signal::UpdatesEnabled);
    assert_eq!(d.phase(at("10:09:07")), MatchPhase::Ended);
    // The next match replaces the finished one, even if nobody cleared it.
    d.feed(at("10:15:44"), Signal::UpdatesDisabled);
    assert_eq!((d.phase(at("10:15:45")), d.current().unwrap().found), (MatchPhase::Loading, at("10:15:44")));
}

#[test]
fn ends_that_only_time_tells() {
    // Staying on the end screen: recorded for 45 s at most.
    let mut d = Detector::default();
    d.feed(at("09:28:20"), Signal::LobbyCreated("1".into()));
    d.feed(at("09:29:01"), Signal::State(7));
    d.feed(at("10:09:07"), Signal::State(8));
    d.settle(at("10:09:51"));
    assert_eq!(d.phase(at("10:09:51")), MatchPhase::InProgress);
    d.settle(at("10:09:52"));
    assert_eq!(d.current().unwrap().over, Some(at("10:09:52")));

    // The console log goes quiet (Steam says the match is over, the game says nothing).
    let mut d = Detector::default();
    d.feed(at("09:28:20"), Signal::LobbyCreated("1".into()));
    d.feed(at("09:40:00"), Signal::UpdatesEnabled);
    d.settle(at("09:40:29"));
    assert_eq!(d.current().unwrap().over, None);
    d.settle(at("09:40:30"));
    assert_eq!(d.current().unwrap().over, Some(at("09:40:00")));

    // Steam stops renewing (leftover lines of a run that crashed): no match.
    let mut d = Detector::default();
    d.feed(at("09:28:21"), Signal::UpdatesDisabled);
    d.settle(at("09:35:20"));
    assert_eq!(d.phase(at("09:35:20")), MatchPhase::InProgress);
    d.settle(at("09:35:21"));
    assert_eq!(d.phase(at("09:35:21")), MatchPhase::Ended);

    // The game closes, or is started again, in the middle of a match.
    for by_console in [false, true] {
        let mut d = Detector::default();
        d.feed(at("09:28:20"), if by_console { Signal::LobbyCreated("1".into()) } else { Signal::UpdatesDisabled });
        d.feed(at("09:30:00"), Signal::Closed);
        assert_eq!(d.current().unwrap().over, Some(at("09:30:00")));
    }
}

#[test]
fn a_log_is_followed_from_its_recent_past() {
    let dir = std::env::temp_dir().join(format!("cv-deadlock-follow-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("content_log.txt");
    let append = |text: &str| {
        use std::io::Write;
        std::fs::OpenOptions::new().create(true).append(true).open(&path).unwrap().write_all(text.as_bytes()).unwrap();
    };
    let mut missing = Follower::open(path.clone(), 100);
    assert!(missing.read().is_empty());
    append("first line, long ago\nsecond line\nthird ");
    assert_eq!(missing.read(), ["first line, long ago", "second line"], "a file that appears is read from its start");

    // Opened 10 bytes before the end: in the middle of "second line".
    let mut f = Follower::open(path.clone(), 10);
    assert!(f.read().is_empty(), "the cut line is dropped, the unfinished one waits");
    append("line\r\nfourth\n");
    assert_eq!(f.read(), ["third line", "fourth"]);
    assert!(f.read().is_empty());

    // Steam starts the file over.
    std::fs::write(&path, "new file\n").unwrap();
    assert_eq!(f.read(), ["new file"]);
    std::fs::remove_dir_all(dir).ok();
}
