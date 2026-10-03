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
//! - a game session in progress → a match you play: recorded as before;
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
