//! Deadlock support.
//!
//! Valve has no Game State Integration and no other official live API for Deadlock, and the game
//! is VAC-protected, so this module only ever reads what the game or Steam writes to disk (Steam's
//! own log, the game's console log, Valve's replay files), the game's window title and the user's
//! own key presses. No memory reading, no injection, no in-game overlay.
//!
//! Recording: one recording per match ([`GameIntegration::match_only`]), from the moment the
//! match is found (just before the loading screen) to just after the end screen. The Hideout, the
//! sandbox and spectating are never recorded: they give no match signal ([`signals`]). Nothing
//! has to be set up; with `-condebug` in Deadlock's launch options the timing is a little
//! tighter.
//!
//! Timeline: your own key presses during the match (abilities, the ultimate, item slots, melee,
//! parry), with the binds read from the game's own file ([`binds`], [`keys`]). Kills, objectives
//! and items come from the replay file later.

pub mod binds;
pub mod diag;
pub mod keys;
pub mod paths;
mod platform;
pub mod signals;

use async_trait::async_trait;
use chrono::{Datelike, NaiveDateTime};
use cv_core::game::{CaptureTarget, ConfigField, GameIntegration, KeyMark, KeyPress, MatchPhase, PollUpdate};
use cv_core::{EventKind, GameEvent};
use signals::{Detector, Follower};
use std::path::PathBuf;
use std::time::Duration;

/// Stable id used in settings and session files.
pub const ID: &str = "deadlock";

/// How much of each log's recent past is read when watching begins (enough to know whether a
/// match is on right now, e.g. when the app starts in the middle of one).
const STEAM_HISTORY: u64 = 64 * 1024;
const CONSOLE_HISTORY: u64 = 256 * 1024;

fn now() -> NaiveDateTime {
    chrono::Local::now().naive_local()
}

#[derive(Default)]
pub struct DeadlockIntegration {
    /// Settings > Games: Steam's folder, for when it isn't found by itself.
    steam_folder: String,
    steam_log: Option<Follower>,
    console_log: Option<Follower>,
    detector: Detector,
    /// The "match over" event of the current match was returned.
    end_sent: bool,
    /// Your key presses of the current match, with the binds read when it started.
    keys: keys::Tracker,
    binds_logged: bool,
    complained: bool,
}

impl DeadlockIntegration {
    pub fn new() -> Self {
        Self::default()
    }

    fn steam_root(&self) -> Option<PathBuf> {
        if self.steam_folder.is_empty() {
            paths::steam_root()
        } else {
            Some(PathBuf::from(&self.steam_folder))
        }
    }

    /// Feeds what the logs say since the last look to the detector.
    fn read_logs(&mut self, now: NaiveDateTime) {
        let mut found = Vec::new();
        if let Some(log) = &mut self.steam_log {
            found.extend(log.read().iter().filter_map(|l| signals::parse_steam(l.as_str())));
        }
        if let Some(log) = &mut self.console_log {
            found.extend(log.read().iter().filter_map(|l| signals::parse_console(l.as_str(), now.year())));
        }
        // By the lines' own times; Steam's first when both say something in the same second.
        found.sort_by_key(|(t, _)| *t);
        for (t, signal) in found {
            self.detector.feed(t, signal);
        }
        self.detector.settle(now);
    }
}

#[async_trait]
impl GameIntegration for DeadlockIntegration {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        "Deadlock"
    }
    fn short_name(&self) -> &'static str {
        "Deadlock"
    }
    fn process_names(&self) -> &'static [&'static str] {
        paths::PROCESS_NAMES
    }
    fn capture(&self) -> CaptureTarget {
        CaptureTarget { exe: "deadlock.exe".into(), display_capture_only: false }
    }
    fn supports_events(&self) -> bool {
        true
    }
    fn match_only(&self) -> bool {
        true
    }
    /// With the console log the end screen is recorded until it is left, so only a short tail is
    /// added. Without it the match ends at the end banner: a longer tail covers the end screen.
    fn end_grace(&self) -> Duration {
        Duration::from_secs(if self.detector.current().is_some_and(|m| m.console) { 2 } else { 8 })
    }

    fn config_fields(&self) -> Vec<ConfigField> {
        vec![ConfigField {
            key: "steam_folder",
            label: "Steam folder",
            kind: "text",
            help: "Found automatically. Matches are detected from Steam's own log, so there is nothing to set up. Optional: with -condebug in Deadlock's launch options (Steam > Deadlock > Properties) the recording ends exactly when you leave the end screen.",
            options: &[],
        }]
    }
    fn default_config(&self) -> serde_json::Value {
        serde_json::json!({ "steam_folder": "" })
    }
    fn configure(&mut self, config: &serde_json::Value) {
        self.steam_folder = config["steam_folder"].as_str().unwrap_or("").trim().to_string();
    }

    /// Called when the game process appears and again when a match's recording starts: begins
    /// watching the logs anew, reading their recent past to know whether a match is on.
    async fn start(&mut self) -> anyhow::Result<()> {
        let steam = self.steam_root();
        let install = steam.as_deref().and_then(paths::install_dir);
        self.steam_log = steam.as_deref().map(|s| Follower::open(s.join("logs").join("content_log.txt"), STEAM_HISTORY));
        self.console_log = install.as_deref().map(|i| Follower::open(paths::citadel_dir(i).join("console.log"), CONSOLE_HISTORY));
        if steam.is_none() && !self.complained {
            self.complained = true;
            log::warn!("Deadlock: Steam's folder wasn't found, so matches can't be detected (set it in Settings > Games)");
        }
        self.detector = Detector::default();
        self.end_sent = false;
        // The binds as the game has them now (a small file; changes count from the next match).
        let binds = binds::load(steam.as_deref());
        if !self.binds_logged || *self.keys.binds() != binds {
            self.binds_logged = true;
            let source = if binds.from_file { "the game's file" } else { "the defaults: no binds file found" };
            log::info!("Deadlock: key binds ({source}): {}", binds.summary());
            for unseen in binds.unseen() {
                log::info!("Deadlock: {unseen} can't be followed live (only keyboard keys are seen)");
            }
        }
        self.keys = keys::Tracker::new(binds);
        let now = now();
        self.read_logs(now);
        // A match that ended before now (an earlier session's, or one the app missed) is history.
        if self.detector.phase(now) == MatchPhase::Ended {
            self.detector.clear();
        }
        if let Some(m) = self.detector.current() {
            log::info!("Deadlock: a match is on (found {}, match id {:?}, console log {})", m.found, m.id, m.console);
        }
        Ok(())
    }

    async fn poll(&mut self) -> anyhow::Result<PollUpdate> {
        let now = now();
        self.read_logs(now);
        let mut u = PollUpdate { phase: self.detector.phase(now), ..Default::default() };
        let Some(m) = self.detector.current() else { return Ok(u) };
        // The clock of a match: seconds since it was found.
        let clock = |t: NaiveDateTime| (t - m.found).num_milliseconds().max(0) as f64 / 1000.0;
        u.game_time = Some(clock(now));
        if let (Some(over), false) = (m.over, self.end_sent) {
            self.end_sent = true;
            let (at, title) = match (m.post_game, m.console) {
                (Some(banner), _) => (banner, "Match over"),
                (None, true) => (over, "Left the match"),
                (None, false) => (over, "Match over"),
            };
            log::info!("Deadlock: {title} (found {}, over {over}, match id {:?}, console log {})", m.found, m.id, m.console);
            u.events.push(GameEvent::new("deadlock-end", EventKind::GameEnd, clock(at), title));
        }
        Ok(u)
    }

    async fn stop(&mut self) {
        self.detector.clear();
        self.end_sent = false;
        self.keys = keys::Tracker::new(self.keys.binds().clone());
    }

    /// The engine only passes keys pressed while Deadlock has the focus and the match is in
    /// progress.
    fn on_key(&mut self, key: &KeyPress, game_time: f64) -> Option<GameEvent> {
        self.keys.press(key, game_time)
    }
    fn take_key_marks(&mut self) -> Vec<KeyMark> {
        self.keys.take_marks()
    }
}

#[cfg(test)]
mod tests;
