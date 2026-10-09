//! Telling when a Deadlock match starts and ends, from two logs (chosen from the owner's real
//! runs of 2026-10-09; the lines are in `tests/fixtures`, the findings in PLAN.md "Deadlock").
//!
//! - **Steam's `logs\content_log.txt`**, always written, nothing to set up: Deadlock asks Steam
//!   not to update it while a match is on. "updates disabled" appears the second a match is
//!   found (before the loading screen) and is renewed every 6 minutes; "updates now enabled"
//!   appears at the end banner, or the moment you leave. Never for the Hideout, the sandbox or
//!   spectating.
//! - **The game's `console.log`**, only with `-condebug` in the launch options: the match's
//!   lobby and every stage of it, to the second. It refines the first when it is there (the
//!   match id, when the match is really in progress, when the end screen is left); it is never
//!   required.
//!
//! The Hideout and the sandbox are local games that print the same game-state lines as a
//! match, so those lines only count between a lobby of your own being created and you leaving.

use chrono::{Duration, NaiveDateTime};
use cv_core::game::MatchPhase;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

/// One thing a log line says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Signal {
    /// Steam: updates disabled for Deadlock (a match was found, or is still on).
    UpdatesDisabled,
    /// Steam: updates enabled again (the match is over, or you left it).
    UpdatesEnabled,
    /// Steam: Deadlock isn't running any more. Console: it was started anew.
    Closed,
    /// Console: a lobby of your own for this match id.
    LobbyCreated(String),
    /// Console: the game state the server reports (4 intro ... 7 in progress, 8 post game, 11 end).
    State(u8),
    /// Console: you disconnected (left early, or left the end screen).
    Disconnect,
    /// Console: the Hideout's map was loaded.
    Hideout,
}

const STATE_IN_PROGRESS: u8 = 7;
const STATE_POST_GAME: u8 = 8;
const STATE_END: u8 = 11;

/// A line of Steam's `content_log.txt` (`[2026-10-09 09:28:21] App 1422450 updates ...`).
pub fn parse_steam(line: &str) -> Option<(NaiveDateTime, Signal)> {
    let t = NaiveDateTime::parse_from_str(line.get(1..20)?, "%Y-%m-%d %H:%M:%S").ok()?;
    let rest = line.get(22..)?;
    if let Some(what) = rest.strip_prefix("App 1422450 updates ") {
        if what.starts_with("disabled") {
            return Some((t, Signal::UpdatesDisabled));
        }
        return what.starts_with("now enabled").then_some((t, Signal::UpdatesEnabled));
    }
    let state = rest.strip_prefix("AppID 1422450 state changed : ")?;
    (!state.contains("App Running")).then_some((t, Signal::Closed))
}

/// A line of the game's `console.log` (`10/09 09:28:20 Lobby ... created`; it has no year).
pub fn parse_console(line: &str, year: i32) -> Option<(NaiveDateTime, Signal)> {
    let t = NaiveDateTime::parse_from_str(&format!("{year}/{}", line.get(..14)?), "%Y/%m/%d %H:%M:%S").ok()?;
    let rest = line.get(15..)?.trim_end();
    if let Some(state) = rest.strip_prefix("OnGameStateChanged: ") {
        let n = state.rsplit_once('(')?.1.strip_suffix(')')?.parse().ok()?;
        return Some((t, Signal::State(n)));
    }
    if let Some(lobby) = rest.strip_prefix("Lobby ") {
        let id = lobby.split_once(" for Match ")?.1.strip_suffix(" created")?;
        return Some((t, Signal::LobbyCreated(id.to_string())));
    }
    match rest {
        "[Client] CL:  disconnect" => Some((t, Signal::Disconnect)),
        "[Client] Map: \"dl_hideout\"" => Some((t, Signal::Hideout)),
        // The game was started (the log is appended to across launches): whatever was on is over.
        "Source2Init OK" => Some((t, Signal::Closed)),
        _ => None,
    }
}

/// One match, as far as the logs have told.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    /// Valve's match id (console log only).
    pub id: Option<String>,
    /// When the match was found (just before the loading screen).
    pub found: NaiveDateTime,
    /// The console log is telling about this match (`-condebug` is on).
    pub console: bool,
    pub in_progress: Option<NaiveDateTime>,
    /// The end banner (played to the end).
    pub post_game: Option<NaiveDateTime>,
    /// Steam's last "updates disabled" (renewed every 6 minutes while the match is on).
    heartbeat: Option<NaiveDateTime>,
    enabled: Option<NaiveDateTime>,
    /// When recording should stop: the end screen was left, or the match was.
    pub over: Option<NaiveDateTime>,
}

/// Without the console log: the usual time from "match found" to the match running (41 s in
/// both of the owner's matches), only for the phase shown in the status.
const USUAL_LOADING: i64 = 40;
/// With the console log: the end screen is recorded until it is left, at most this long.
const MAX_END_SCREEN: i64 = 45;
/// Steam renews "updates disabled" every 360 s; without a renewal the match is over (or the
/// lines are leftovers from a run that crashed).
const HEARTBEAT_LOST: i64 = 420;
/// The console log said nothing after Steam enabled updates: trust Steam after this long.
const CONSOLE_SILENT: i64 = 30;
/// No match is this long: whatever the logs say, it is over.
const MAX_MATCH: i64 = 3 * 3600;

/// Follows the signals and knows whether a match is on.
#[derive(Debug, Default)]
pub struct Detector {
    current: Option<Match>,
}

impl Detector {
    /// The match that is on, or the one that just ended (until [`Self::clear`]).
    pub fn current(&self) -> Option<&Match> {
        self.current.as_ref()
    }

    /// Forgets the match (its recording was ended).
    pub fn clear(&mut self) {
        self.current = None;
    }

    fn open(&mut self) -> Option<&mut Match> {
        self.current.as_mut().filter(|m| m.over.is_none())
    }

    fn begin(&mut self, found: NaiveDateTime) {
        self.current = Some(Match { id: None, found, console: false, in_progress: None, post_game: None, heartbeat: None, enabled: None, over: None });
    }

    /// One signal, with the time its log line carries.
    pub fn feed(&mut self, t: NaiveDateTime, signal: Signal) {
        match signal {
            Signal::UpdatesDisabled => {
                if self.open().is_none() {
                    self.begin(t);
                }
                if let Some(m) = self.open() {
                    m.heartbeat = Some(t);
                }
            }
            Signal::LobbyCreated(id) => {
                // The same match Steam just announced (or will, a second later), or a new one.
                let same = self.open().is_some_and(|m| m.id.as_ref().is_none_or(|known| *known == id));
                if !same {
                    self.begin(t);
                }
                if let Some(m) = self.open() {
                    m.id = Some(id);
                    m.console = true;
                }
            }
            Signal::UpdatesEnabled => {
                if let Some(m) = self.open() {
                    m.enabled = Some(t);
                    if !m.console {
                        m.over = Some(t);
                    }
                }
            }
            Signal::Closed => {
                if let Some(m) = self.open() {
                    m.over = Some(t);
                }
            }
            Signal::State(n) => {
                // The Hideout and the sandbox print these too: only inside a match of your own.
                if let Some(m) = self.open().filter(|m| m.console) {
                    match n {
                        STATE_IN_PROGRESS => m.in_progress = m.in_progress.or(Some(t)),
                        STATE_POST_GAME => m.post_game = m.post_game.or(Some(t)),
                        STATE_END => m.over = Some(t),
                        _ => {}
                    }
                }
            }
            Signal::Disconnect | Signal::Hideout => {
                if let Some(m) = self.open().filter(|m| m.console) {
                    m.over = Some(t);
                }
            }
        }
    }

    /// What only time can tell: a match whose end was never written.
    pub fn settle(&mut self, now: NaiveDateTime) {
        let Some(m) = self.open() else { return };
        let after = |t: Option<NaiveDateTime>, secs: i64| t.map(|t| t + Duration::seconds(secs)).filter(|end| *end <= now);
        let too_long = after(Some(m.found), MAX_MATCH);
        let end_screen = after(m.post_game, MAX_END_SCREEN);
        let silent = after(m.enabled.filter(|_| m.post_game.is_none()), CONSOLE_SILENT).and(m.enabled);
        let lost = after(m.heartbeat, HEARTBEAT_LOST).and(m.heartbeat);
        let by_log = if m.console { end_screen.or(silent) } else { lost };
        m.over = by_log.or(too_long);
    }

    pub fn phase(&self, now: NaiveDateTime) -> MatchPhase {
        match &self.current {
            None => MatchPhase::Waiting,
            Some(m) if m.over.is_some() => MatchPhase::Ended,
            Some(m) if m.in_progress.is_some() => MatchPhase::InProgress,
            Some(m) if !m.console && now - m.found >= Duration::seconds(USUAL_LOADING) => MatchPhase::InProgress,
            Some(_) => MatchPhase::Loading,
        }
    }
}

/// Reads what is appended to a log file, without locking it (opened for a moment, shared).
#[derive(Debug)]
pub struct Follower {
    path: PathBuf,
    offset: u64,
    /// The bytes after the last line break (a line still being written).
    partial: Vec<u8>,
    /// The first line read starts in the middle of a line: drop it.
    skip_first: bool,
}

/// At most this much of a log is read in one go.
const MAX_READ: u64 = 4 * 1024 * 1024;

impl Follower {
    /// Starts `back` bytes before the file's end (its recent history comes with the first read).
    pub fn open(path: PathBuf, back: u64) -> Follower {
        let len = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        let offset = len.saturating_sub(back);
        Follower { path, offset, partial: Vec::new(), skip_first: offset > 0 }
    }

    /// The complete lines written since the last call.
    pub fn read(&mut self) -> Vec<String> {
        let Ok(len) = std::fs::metadata(&self.path).map(|m| m.len()) else { return Vec::new() };
        if len < self.offset {
            // Started over (Steam rotates its logs).
            self.offset = 0;
            self.partial.clear();
            self.skip_first = false;
        }
        if len == self.offset {
            return Vec::new();
        }
        let (offset, want) = (self.offset, (len - self.offset).min(MAX_READ));
        let mut buf = Vec::new();
        let read = File::open(&self.path).and_then(|mut f| {
            f.seek(SeekFrom::Start(offset))?;
            f.take(want).read_to_end(&mut buf)
        });
        if read.is_err() {
            return Vec::new();
        }
        self.offset += buf.len() as u64;
        self.partial.extend_from_slice(&buf);
        let Some(end) = self.partial.iter().rposition(|b| *b == b'\n') else { return Vec::new() };
        let done: Vec<u8> = self.partial.drain(..=end).collect();
        let mut lines: Vec<String> = String::from_utf8_lossy(&done).lines().map(str::to_string).collect();
        if std::mem::take(&mut self.skip_first) && !lines.is_empty() {
            lines.remove(0);
        }
        lines
    }
}

#[cfg(test)]
mod tests;
