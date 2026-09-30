use super::*;
use serde_json::json;

fn snap(phase: &str, round: i64, rphase: &str, k: i64, d: i64, a: i64, rk: i64, local: bool) -> Value {
    json!({
        "provider": {"steamid": "111"},
        "map": {"name": "de_mirage", "mode": "competitive", "phase": phase, "round": round, "team_ct": {"score": 13}, "team_t": {"score": 9}},
        "round": {"phase": rphase, "win_team": "CT"},
        "player": {"steamid": if local {"111"} else {"222"}, "name": "Me", "team": "CT",
                   "state": {"round_kills": rk}, "match_stats": {"kills": k, "deaths": d, "assists": a, "mvps": 3, "score": 40}},
        "auth": {"token": TOKEN}
    })
}

#[test]
fn counters_become_events() {
    let mut t = Tracker::default();
    assert!(diff(&mut t, &snap("live", 0, "live", 0, 0, 0, 0, true), 1.0).is_empty(), "first snapshot is the baseline");
    let e = diff(&mut t, &snap("live", 0, "live", 2, 0, 1, 2, true), 5.0);
    assert_eq!(e.iter().filter(|e| e.kind == EventKind::Kill).count(), 2);
    assert_eq!(e.iter().filter(|e| e.kind == EventKind::Assist).count(), 1);
    let e = diff(&mut t, &snap("live", 0, "live", 3, 0, 1, 3, true), 6.0);
    assert!(e.iter().any(|e| e.kind == EventKind::Multikill && e.title == "3K"));
    // Spectating a teammate after dying: their stats must not count.
    let e = diff(&mut t, &snap("live", 0, "live", 9, 9, 9, 0, false), 7.0);
    assert!(e.is_empty());
    let e = diff(&mut t, &snap("live", 0, "over", 3, 1, 1, 3, true), 8.0);
    assert!(e.iter().any(|e| e.kind == EventKind::Death));
    assert!(e.iter().any(|e| e.kind == EventKind::Round && e.title == "Round 1 won"));
    assert_eq!(result(&snap("gameover", 22, "over", 3, 1, 1, 0, true), t.team.as_deref()), Some(GameResult::Win));
    assert_eq!(map_phase(&snap("warmup", 0, "live", 0, 0, 0, 0, true)), MatchPhase::Loading);
    assert_eq!(map_title("de_mirage"), "Mirage");
}

#[test]
fn library_folders() {
    let vdf = "\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\t\"C:\\\\Program Files (x86)\\\\Steam\"\n\t}\n\t\"1\"\n\t{\n\t\t\"path\"\t\t\"D:\\\\SteamLibrary\"\n\t}\n}";
    let l = gsi::parse_library_folders(vdf);
    assert_eq!(l, vec![PathBuf::from("C:\\Program Files (x86)\\Steam"), PathBuf::from("D:\\SteamLibrary")]);
}

use std::path::PathBuf;

fn post(port: u16, body: &Value) {
    use std::io::{Read, Write};
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    let b = body.to_string();
    write!(s, "POST / HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}", b.len(), b).unwrap();
    let mut buf = [0u8; 64];
    let n = s.read(&mut buf).unwrap();
    assert!(String::from_utf8_lossy(&buf[..n]).starts_with("HTTP/1.1 200"));
}

#[tokio::test]
async fn gsi_server_and_poll() {
    let mut cs = Cs2Integration::new();
    cs.configure(&json!({ "gsi_port": "33801", "cfg_folder": "/nonexistent" }));
    cs.start().await.unwrap();
    assert_eq!(cs.poll().await.unwrap().phase, MatchPhase::Waiting);
    post(33801, &json!({"provider": {"steamid": "1"}, "auth": {"token": "wrong"}, "map": {"phase": "live"}}));
    assert_eq!(cs.poll().await.unwrap().phase, MatchPhase::Waiting, "wrong token ignored");
    post(33801, &snap("live", 1, "live", 0, 0, 0, 0, true));
    let u = cs.poll().await.unwrap();
    assert_eq!(u.phase, MatchPhase::InProgress);
    assert_eq!(u.player.unwrap().character.as_deref(), Some("Mirage"));
    post(33801, &snap("live", 1, "live", 1, 0, 0, 1, true));
    assert_eq!(cs.poll().await.unwrap().events.len(), 1);
    post(33801, &snap("gameover", 22, "over", 1, 0, 0, 0, true));
    let u = cs.poll().await.unwrap();
    assert_eq!(u.phase, MatchPhase::Ended);
    assert_eq!(u.result, Some(GameResult::Win));
}

#[test]
fn cfg_install() {
    let dir = std::env::temp_dir().join(format!("cs2cfg-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    assert!(gsi::install_cfg(&dir, 3380, TOKEN).unwrap());
    assert!(!gsi::install_cfg(&dir, 3380, TOKEN).unwrap(), "unchanged the second time");
    let text = std::fs::read_to_string(dir.join(CFG_NAME)).unwrap();
    assert!(text.contains("http://127.0.0.1:3380/") && text.contains(TOKEN));
    std::fs::remove_dir_all(dir).ok();
}
