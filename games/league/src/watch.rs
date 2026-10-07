//! Replays and spectating: League runs the same game client ("League of Legends.exe") for a
//! match you play, a replay (`.rofl`, "Watch" in match history) and spectating a live game.
//! Clairvoyance must only record matches you play, so the process start is checked before
//! anything is recorded, and the in-game API is checked again once it answers.
//!
//! What each source says (captured on the owner's PC on 2026-10-03, patch 16.19, while a replay
//! from match history played; spectating from the documented API behaviour):
//!
//! | source | your match | replay | spectating |
//! |---|---|---|---|
//! | LCU `/lol-gameflow/v1/gameflow-phase` | `"GameStart"` / `"InProgress"` / `"Reconnect"` | `"None"` (no game session) | no game session of yours |
//! | LCU `/lol-gameflow/v1/session` | the session (queue, ...) | 404 "No gameflow session exists." | |
//! | LCU `/lol-replays/v1/configuration` | `isPlayingReplay: false` | `isPlayingReplay: true` | `false` |
//! | Live `/liveclientdata/activeplayer` | 200, your champion | 400 "Spectator mode doesn't currently support this feature" | same |
//! | Live `/liveclientdata/activeplayername` | your Riot ID | `"Unknown"` | `"Unknown"` |
//!
//! The in-game API (port 2999) only answers once the game has loaded (~6 s for a replay, the end
//! of the loading screen for a match), so the League client decides first:
//! - a game session in progress → a match you play: recorded as before, unless the session's
//!   players clearly don't include you (spectating a friend's game: the client reports it like
//!   your own match, seen on the owner's PC) → nothing is recorded until the in-game API says
//!   (a wrong guess only loses the loading screen);
//! - the client playing a replay → never recorded;
//! - the client spectating (its watch state) → not recorded unless "Record games you spectate";
//! - no game session at all → most likely a replay / spectating: nothing is recorded until the
//!   in-game API says (spectator mode → not recorded; your champion → recording starts);
//! - the client can't be reached → recorded, and deleted if the in-game API then says
//!   spectator mode (a few seconds in).

use cv_core::game::{SessionCheck, WatchKind};

/// What the League client (LCU) says when the game process appears.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ClientFacts {
    /// `/lol-gameflow/v1/gameflow-phase` (None = the client couldn't be reached).
    pub phase: Option<String>,
    /// `/lol-replays/v1/configuration` → `isPlayingReplay`.
    pub playing_replay: Option<bool>,
    /// `/lol-gameflow/v1/watch` → its phase (spectating), if the client has that endpoint.
    pub watch_phase: Option<String>,
}

/// Gameflow phases in which the client has launched a match of yours.
const PLAYING: [&str; 3] = ["GameStart", "InProgress", "Reconnect"];

pub fn classify_client(f: &ClientFacts) -> SessionCheck {
    if f.playing_replay == Some(true) {
        return SessionCheck::Watching(WatchKind::Replay);
    }
    let watching = |p: &str| p.starts_with("Watch") && !p.contains("Failed");
    if f.watch_phase.as_deref().is_some_and(watching) || f.phase.as_deref().is_some_and(watching) {
        return SessionCheck::Watching(WatchKind::Spectate);
    }
    match f.phase.as_deref() {
        None => SessionCheck::Unknown,
        Some(p) if PLAYING.iter().any(|x| x.eq_ignore_ascii_case(p)) => SessionCheck::Playing,
        // "None", "Lobby", "EndOfGame" (a replay started from the end-of-game screen), ...: no
        // match of yours is running.
        Some(_) => SessionCheck::Unsure(WatchKind::Unknown),
    }
}

/// Are you one of the match's players? `me` = `/lol-summoner/v1/current-summoner`, `session` =
/// `/lol-gameflow/v1/session` (its `gameData.teamOne` / `teamTwo`). Compared by `puuid`, else
/// by `summonerId`. `None` = can't tell (a list without ids, an unknown format, your account
/// unknown): then nothing changes. Only a list with at least two players carrying the same id
/// as yours, and none of them you, says `Some(false)`.
pub fn in_roster(me: &serde_json::Value, session: &serde_json::Value) -> Option<bool> {
    let players: Vec<&serde_json::Value> = ["teamOne", "teamTwo"].iter().filter_map(|t| session["gameData"][*t].as_array()).flatten().collect();
    let id = |v: &serde_json::Value, key: &str| match &v[key] {
        serde_json::Value::String(s) if !s.is_empty() => Some(s.clone()),
        serde_json::Value::Number(n) if n.as_u64().is_some_and(|n| n > 0) => Some(n.to_string()),
        _ => None,
    };
    for key in ["puuid", "summonerId"] {
        let Some(mine) = id(me, key) else { continue };
        let ids: Vec<String> = players.iter().filter_map(|p| id(p, key)).collect();
        if ids.len() >= 2 {
            return Some(ids.contains(&mine));
        }
    }
    None
}

/// The client's verdict refined by the match's players: a "match of yours" you aren't playing
/// in is probably spectating, so the in-game API decides (`Unsure`).
pub fn refine_with_roster(c: SessionCheck, in_roster: Option<bool>) -> SessionCheck {
    match (c, in_roster) {
        (SessionCheck::Playing, Some(false)) => SessionCheck::Unsure(WatchKind::Spectate),
        _ => c,
    }
}

/// The in-game API's answer to `/liveclientdata/activeplayer`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveVerdict {
    /// Your champion's data: a match you play.
    Playing,
    /// Spectator mode (replay or spectating).
    Spectator,
    /// Not answering yet / something else.
    Undecided,
}

pub fn classify_active_player(status: u16, body: &str) -> LiveVerdict {
    if (200..300).contains(&status) {
        // A real answer has the champion's stats; the loading screen answers nothing.
        return if body.contains("championStats") || body.contains("abilities") { LiveVerdict::Playing } else { LiveVerdict::Undecided };
    }
    if body.to_ascii_lowercase().contains("spectator") {
        return LiveVerdict::Spectator;
    }
    LiveVerdict::Undecided
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(phase: Option<&str>, replay: Option<bool>, watch: Option<&str>) -> ClientFacts {
        ClientFacts { phase: phase.map(String::from), playing_replay: replay, watch_phase: watch.map(String::from) }
    }

    #[test]
    fn client_says_what_runs() {
        // Captured: a replay from match history.
        assert_eq!(classify_client(&facts(Some("None"), Some(true), None)), SessionCheck::Watching(WatchKind::Replay));
        // A match of yours, in every phase the game can start in.
        for p in ["GameStart", "InProgress", "Reconnect"] {
            assert_eq!(classify_client(&facts(Some(p), Some(false), None)), SessionCheck::Playing, "{p}");
        }
        // Spectating: the client's watch state.
        assert_eq!(classify_client(&facts(Some("None"), Some(false), Some("WatchInProgress"))), SessionCheck::Watching(WatchKind::Spectate));
        assert_eq!(classify_client(&facts(Some("WatchInProgress"), None, None)), SessionCheck::Watching(WatchKind::Spectate));
        assert_eq!(classify_client(&facts(Some("None"), Some(false), Some("WatchFailedToLaunch"))), SessionCheck::Unsure(WatchKind::Unknown));
        // No game session and no replay flag: probably watching, the game decides.
        assert_eq!(classify_client(&facts(Some("None"), None, None)), SessionCheck::Unsure(WatchKind::Unknown));
        assert_eq!(classify_client(&facts(Some("EndOfGame"), Some(false), Some("None"))), SessionCheck::Unsure(WatchKind::Unknown));
        // Client not reachable.
        assert_eq!(classify_client(&facts(None, None, None)), SessionCheck::Unknown);
    }

    #[test]
    fn the_match_players_tell_spectating_apart() {
        use serde_json::json;
        let me = json!({"puuid": "p-me", "summonerId": 11, "gameName": "Me"});
        let session = |one: Vec<serde_json::Value>| json!({"phase": "InProgress", "gameData": {"teamOne": one, "teamTwo": [{"puuid": "p-5", "summonerId": 5}, {"puuid": "p-6", "summonerId": 6}]}});
        let mine = session(vec![json!({"puuid": "p-me", "summonerId": 11}), json!({"puuid": "p-2", "summonerId": 2})]);
        let friends = session(vec![json!({"puuid": "p-friend", "summonerId": 12}), json!({"puuid": "p-2", "summonerId": 2})]);
        assert_eq!(in_roster(&me, &mine), Some(true));
        assert_eq!(in_roster(&me, &friends), Some(false));
        // Only summoner ids (no puuid anywhere): compared by those.
        let me_id = json!({"summonerId": 11});
        assert_eq!(in_roster(&me_id, &json!({"gameData": {"teamOne": [{"summonerId": 11}], "teamTwo": [{"summonerId": 5}]}})), Some(true));
        assert_eq!(in_roster(&me_id, &json!({"gameData": {"teamOne": [{"summonerId": 12}], "teamTwo": [{"summonerId": 5}]}})), Some(false));
        // Can't tell: no teams, empty teams, players without ids (bots), one player only, ids
        // of another kind, an unknown account, the client's 404 answer.
        assert_eq!(in_roster(&me, &json!({"gameData": {"queue": {"id": 420}}})), None);
        assert_eq!(in_roster(&me, &json!({"gameData": {"teamOne": [], "teamTwo": []}})), None);
        assert_eq!(in_roster(&me, &json!({"gameData": {"teamOne": [{"championId": 1}, {"championId": 2}], "teamTwo": []}})), None);
        assert_eq!(in_roster(&me, &json!({"gameData": {"teamOne": [{"puuid": "p-x"}], "teamTwo": []}})), None);
        assert_eq!(in_roster(&json!({"puuid": "p-me"}), &json!({"gameData": {"teamOne": [{"summonerId": 1}, {"summonerId": 2}]}})), None);
        assert_eq!(in_roster(&json!({}), &friends), None);
        assert_eq!(in_roster(&json!({"puuid": "", "summonerId": 0}), &friends), None);
        assert_eq!(in_roster(&me, &json!({"errorCode": "RPC_ERROR", "httpStatus": 404})), None);
        // Practice Tool with bots: you're there (bots carry no puuid).
        let practice = json!({"gameData": {"teamOne": [{"puuid": "p-me", "summonerId": 11}], "teamTwo": [{"puuid": "", "summonerId": 0, "botDifficulty": "EASY"}]}});
        assert_ne!(in_roster(&me, &practice), Some(false));

        assert_eq!(refine_with_roster(SessionCheck::Playing, Some(false)), SessionCheck::Unsure(WatchKind::Spectate));
        assert_eq!(refine_with_roster(SessionCheck::Playing, Some(true)), SessionCheck::Playing);
        assert_eq!(refine_with_roster(SessionCheck::Playing, None), SessionCheck::Playing);
        assert_eq!(refine_with_roster(SessionCheck::Watching(WatchKind::Replay), Some(false)), SessionCheck::Watching(WatchKind::Replay));
    }

    #[test]
    fn in_game_api_spectator_mode() {
        // Captured on the owner's PC during a replay (patch 16.19).
        let spect = r#"{
	"errorCode": "RPC_ERROR",
	"httpStatus": 400,
	"implementationDetails": {},
	"message": "Spectator mode doesn't currently support this feature"
}"#;
        assert_eq!(classify_active_player(400, spect), LiveVerdict::Spectator);
        // Loading screen (captured): not decided yet.
        let loading = r#"{
	"errorCode": "RESOURCE_NOT_FOUND",
	"httpStatus": 404,
	"message": "Invalid URI format"
}"#;
        assert_eq!(classify_active_player(404, loading), LiveVerdict::Undecided);
        // A match you play (documented format, shortened).
        let me = r#"{"abilities":{"R":{"abilityLevel":1}},"championStats":{"abilityHaste":0.0},"currentGold":500.0,"level":6,"riotId":"Me#EUW"}"#;
        assert_eq!(classify_active_player(200, me), LiveVerdict::Playing);
        assert_eq!(classify_active_player(200, "{}"), LiveVerdict::Undecided);
    }
}
