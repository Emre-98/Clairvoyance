//! Developer diagnostic "Deadlock: log match signals" (Settings > General > Developer tools).
//!
//! Deadlock has no live API, so before the integration is built every candidate signal for
//! "match loading / started / ended / back in the Hideout" is recorded from one real match, with
//! the time it showed up, and the signals are then chosen from that log. While the game runs
//! this watches, read-only and without locking anything (files are opened for a moment with
//! full sharing; folders are only listed):
//!
//! - the game's console log (`game\citadel\console.log`, written with `-condebug`) and any other
//!   `.log` file next to it: every new line, as written (its own timestamp included);
//! - the game's window titles and whether it has the focus;
//! - the replays folders, the game's `cfg` / `rpt` / `save` folders and its Steam cloud folder;
//! - Steam's game-recording folder (`timelines\*.json`, copied into the log when they change);
//! - Steam's own logs (new lines) and its HTTP cache (file names, sizes and times only).
//!
//! Every change becomes one line of `logs\deadlock-signals-<date>-<time>.log`: wall-clock time,
//! seconds since the log began, the source, the text. Nothing is sent anywhere.

use crate::{paths, platform};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// The option's key in the game's settings (`settings.games["deadlock"]`).
pub const OPTION_KEY: &str = "log_match_signals";
pub const OPTION_LABEL: &str = "Deadlock: log match signals";
pub const OPTION_HELP: &str = "while Deadlock runs, every change of its console log, window title, replay folders, Steam's recording timeline and Steam's logs and HTTP cache (file names and times only) is written to a log file with its time; used to choose the match start / end signals";
/// Log files are `<LOG_PREFIX><date>-<time>.log` in the app's logs folder.
pub const LOG_PREFIX: &str = "deadlock-signals-";

/// How often the console log is looked at while the game runs.
const TICK: Duration = Duration::from_millis(100);
/// How often the process list is checked while the game isn't running.
const IDLE: Duration = Duration::from_secs(2);
/// Keep watching this long after the game closed (Steam writes its timeline file then).
const AFTER_EXIT: Duration = Duration::from_secs(60);
/// A file last written longer ago than this isn't from this game run: its old content is skipped.
const FRESH: Duration = Duration::from_secs(12 * 3600);
const MAX_LOG_BYTES: u64 = 256 * 1024 * 1024;
const MAX_LINE_CHARS: usize = 2000;
const MAX_READ: u64 = 4 * 1024 * 1024;
const MAX_PARTIAL: usize = 64 * 1024;
const MAX_FILES: usize = 50_000;
const MAX_DUMP: u64 = 256 * 1024;
const DUMP_CHUNK: usize = 1500;
/// Log files kept (the newest ones).
const KEEP_LOGS: usize = 8;

/// Where the lines go (a file; a list in the tests).
pub trait Sink {
    fn line(&mut self, source: &str, text: &str);
}

fn stamp(t: SystemTime) -> String {
    chrono::DateTime::<chrono::Local>::from(t).format("%Y-%m-%d %H:%M:%S%.3f").to_string()
}

fn written(t: Option<SystemTime>) -> String {
    t.map(stamp).unwrap_or_else(|| "?".into())
}

/// One line of text: no control characters, cut at a sane length.
fn clean(text: &str) -> String {
    let mut s: String = text.chars().take(MAX_LINE_CHARS).map(|c| if c.is_control() { ' ' } else { c }).collect();
    if text.chars().nth(MAX_LINE_CHARS).is_some() {
        s.push_str(" (cut)");
    }
    s
}

/// The signal log of one game run.
pub struct LogFile {
    path: PathBuf,
    file: File,
    started: Instant,
    bytes: u64,
}

impl LogFile {
    pub fn create(dir: &Path) -> std::io::Result<LogFile> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join(format!("{LOG_PREFIX}{}.log", chrono::Local::now().format("%Y%m%d-%H%M%S")));
        let file = std::fs::OpenOptions::new().create(true).append(true).open(&path)?;
        Ok(LogFile { path, file, started: Instant::now(), bytes: 0 })
    }
}

impl Sink for LogFile {
    fn line(&mut self, source: &str, text: &str) {
        if self.bytes > MAX_LOG_BYTES {
            return;
        }
        let row = format!("{}\t{:+.3}\t{source}\t{}\n", stamp(SystemTime::now()), self.started.elapsed().as_secs_f64(), clean(text));
        if self.file.write_all(row.as_bytes()).is_ok() {
            self.bytes += row.len() as u64;
        }
        if self.bytes > MAX_LOG_BYTES {
            let _ = self.file.write_all(b"(the log is full: nothing more is written)\n");
        }
    }
}

/// The newest signal log in `dir`.
pub fn latest_log(dir: &Path) -> Option<PathBuf> {
    logs_in(dir).pop()
}

/// The signal logs in `dir`, oldest first (their names sort by time).
fn logs_in(dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let ours = |p: &PathBuf| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(LOG_PREFIX));
    let mut v: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(ours).collect();
    v.sort();
    v
}

/// Follows one text file: every new line goes to the log.
pub struct Tail {
    path: PathBuf,
    label: String,
    /// Never log what was in the file before watching began (Steam's long-lived logs).
    skip_existing: bool,
    offset: u64,
    /// The bytes after the last line break (a line still being written).
    partial: Vec<u8>,
    seen: bool,
    polled: bool,
    /// Size of the file when watching began: lines before it are marked with `*`.
    backlog: u64,
    complained: bool,
}

impl Tail {
    /// `appeared`: the file showed up while watching (everything in it is new).
    pub fn new(path: PathBuf, skip_existing: bool, appeared: bool) -> Tail {
        let label = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        Tail { path, label, skip_existing, offset: 0, partial: Vec::new(), seen: false, polled: appeared, backlog: 0, complained: false }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn forget(&mut self) {
        self.seen = false;
        self.offset = 0;
        self.backlog = 0;
        self.partial.clear();
    }

    pub fn poll(&mut self, out: &mut dyn Sink) {
        let first = !self.polled;
        self.polled = true;
        let meta = match std::fs::metadata(&self.path) {
            Ok(m) if m.is_file() => m,
            _ => {
                if self.seen {
                    out.line(&self.label, "(the file is gone)");
                }
                self.forget();
                return;
            }
        };
        let len = meta.len();
        let when = written(meta.modified().ok());
        if !self.seen {
            self.seen = true;
            let old = meta.modified().ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age > FRESH);
            let at = self.path.display();
            if first && (self.skip_existing || old) {
                self.offset = len;
                out.line(&self.label, &format!("(watching {at} from its end: {len} bytes were already there, last written {when})"));
            } else if first {
                self.backlog = len;
                out.line(&self.label, &format!("(watching {at}: {len} bytes were already there, last written {when}; those lines are marked *)"));
            } else {
                out.line(&self.label, &format!("(the file appeared: {at}, {len} bytes)"));
            }
        }
        if len < self.offset {
            out.line(&self.label, &format!("(the file got shorter: {} -> {len} bytes; reading it from its start)", self.offset));
            self.forget();
            self.seen = true;
        }
        if len == self.offset {
            return;
        }
        let (offset, want) = (self.offset, (len - self.offset).min(MAX_READ));
        let mut buf = Vec::new();
        let read = File::open(&self.path).and_then(|mut f| {
            f.seek(SeekFrom::Start(offset))?;
            f.take(want).read_to_end(&mut buf)
        });
        match read {
            Ok(_) => self.feed(&buf, out),
            Err(e) if !self.complained => {
                self.complained = true;
                out.line(&self.label, &format!("(can't read the file: {e})"));
            }
            Err(_) => {}
        }
    }

    fn feed(&mut self, buf: &[u8], out: &mut dyn Sink) {
        // File offset of the first byte in `partial`.
        let base = self.offset - self.partial.len() as u64;
        self.offset += buf.len() as u64;
        self.partial.extend_from_slice(buf);
        let mut start = 0;
        while let Some(n) = self.partial[start..].iter().position(|b| *b == b'\n') {
            self.emit(start, start + n, base, out);
            start += n + 1;
        }
        if self.partial.len() - start > MAX_PARTIAL {
            self.emit(start, self.partial.len(), base, out);
            start = self.partial.len();
        }
        self.partial.drain(..start);
    }

    /// Logs a line that never got its line break (the game closed).
    pub fn flush(&mut self, out: &mut dyn Sink) {
        let base = self.offset - self.partial.len() as u64;
        self.emit(0, self.partial.len(), base, out);
        self.partial.clear();
    }

    fn emit(&self, from: usize, to: usize, base: u64, out: &mut dyn Sink) {
        let text = String::from_utf8_lossy(&self.partial[from..to]);
        let text = text.trim_end_matches('\r');
        if text.trim().is_empty() {
            return;
        }
        if base + (from as u64) < self.backlog {
            out.line(&format!("{}*", self.label), text);
        } else {
            out.line(&self.label, text);
        }
    }
}

/// (size, last written) of a file.
type Stat = (u64, Option<SystemTime>);

fn describe(s: &Stat) -> String {
    format!("{} bytes, written {}", s.0, written(s.1))
}

fn walk(dir: &Path, depth: usize, into: &mut BTreeMap<PathBuf, Stat>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        if into.len() >= MAX_FILES {
            return;
        }
        let Ok(meta) = e.metadata() else { continue };
        if !meta.is_dir() {
            into.insert(e.path(), (meta.len(), meta.modified().ok()));
        } else if depth > 0 {
            walk(&e.path(), depth - 1, into);
        }
    }
}

/// Watches a folder's files (names, sizes, times; never their content): added, changed, removed.
pub struct DirWatch {
    root: PathBuf,
    label: String,
    /// Sub-folder levels looked into (0 = only the folder's own files).
    depth: usize,
    /// Log the files that are already there at the first look (small folders only).
    list_existing: bool,
    exists: Option<bool>,
    files: BTreeMap<PathBuf, Stat>,
}

impl DirWatch {
    pub fn new(root: PathBuf, label: &str, depth: usize, list_existing: bool) -> DirWatch {
        DirWatch { root, label: label.to_string(), depth, list_existing, exists: None, files: BTreeMap::new() }
    }

    fn rel(&self, p: &Path) -> String {
        p.strip_prefix(&self.root).unwrap_or(p).display().to_string()
    }

    /// Logs what changed since the last look; returns the files that are new or changed.
    pub fn poll(&mut self, out: &mut dyn Sink) -> Vec<PathBuf> {
        if !self.root.is_dir() {
            if self.exists != Some(false) {
                out.line(&self.label, &format!("(no such folder: {})", self.root.display()));
            }
            self.exists = Some(false);
            self.files.clear();
            return Vec::new();
        }
        let mut now = BTreeMap::new();
        walk(&self.root, self.depth, &mut now);
        let mut touched = Vec::new();
        if self.exists.is_none() {
            let newest = now.iter().filter_map(|(p, s)| s.1.map(|t| (t, p))).max();
            let newest = newest.map(|(t, p)| format!("; newest: {} ({})", self.rel(p), stamp(t))).unwrap_or_default();
            out.line(&self.label, &format!("(watching {}: {} files already there{newest})", self.root.display(), now.len()));
            if self.list_existing && now.len() <= 60 {
                for (p, s) in &now {
                    out.line(&self.label, &format!("existing {} ({})", self.rel(p), describe(s)));
                }
            }
        } else {
            if self.exists == Some(false) {
                out.line(&self.label, &format!("(the folder appeared: {})", self.root.display()));
            }
            for (p, s) in &now {
                match self.files.get(p) {
                    None => out.line(&self.label, &format!("added {} ({})", self.rel(p), describe(s))),
                    Some(old) if old != s => out.line(&self.label, &format!("changed {} ({}; was {} bytes)", self.rel(p), describe(s), old.0)),
                    Some(_) => continue,
                }
                touched.push(p.clone());
            }
            for p in self.files.keys().filter(|p| !now.contains_key(*p)) {
                out.line(&self.label, &format!("removed {}", self.rel(p)));
            }
        }
        self.exists = Some(true);
        self.files = now;
        touched
    }
}

/// Small text files whose content is worth having in the log (Steam's timeline files, the
/// game's little state lists).
fn wants_dump(p: &Path) -> bool {
    let ext = p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    ["json", "lst", "kv3", "vdf", "txt"].contains(&ext.as_str())
}

/// Copies a small text file into the log, in pieces of one line each.
fn dump(path: &Path, label: &str, out: &mut dyn Sink) {
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    if size > MAX_DUMP {
        out.line(label, &format!("content of {name}: not copied ({size} bytes)"));
        return;
    }
    let Ok(bytes) = std::fs::read(path) else { return };
    let flat: Vec<char> = String::from_utf8_lossy(&bytes).split_whitespace().collect::<Vec<_>>().join(" ").chars().collect();
    let parts = flat.len().div_ceil(DUMP_CHUNK);
    if parts == 0 {
        out.line(label, &format!("content of {name}: (empty)"));
    }
    for (i, chunk) in flat.chunks(DUMP_CHUNK).enumerate() {
        out.line(label, &format!("content of {name} [{}/{parts}]: {}", i + 1, chunk.iter().collect::<String>()));
    }
}

struct Watched {
    watch: DirWatch,
    /// Looked at every this many ticks.
    every: u64,
    /// Copy small text files into the log when they change.
    dump: bool,
}

/// One game run being logged.
pub struct Session {
    out: LogFile,
    game_logs: Vec<Tail>,
    steam_logs: Vec<Tail>,
    log_dirs: Vec<PathBuf>,
    dirs: Vec<Watched>,
    pids: Vec<u32>,
    running: bool,
    exited: Option<Instant>,
    windows: Option<String>,
    ticks: u64,
    /// Time spent looking (the diagnostic's own cost).
    busy: Duration,
}

fn show(p: Option<&Path>) -> String {
    p.map(|p| p.display().to_string()).unwrap_or_else(|| "not found".into())
}

impl Session {
    pub fn start(logs_dir: &Path) -> std::io::Result<Session> {
        let mut out = LogFile::create(logs_dir)?;
        let old = logs_in(logs_dir);
        for p in old.iter().take(old.len().saturating_sub(KEEP_LOGS)) {
            let _ = std::fs::remove_file(p);
        }
        out.line("info", &format!("Clairvoyance {}, Deadlock match signals. Columns: wall-clock time, seconds since this log began, source, text.", env!("CARGO_PKG_VERSION")));
        let steam = paths::steam_root();
        let install = steam.as_deref().and_then(paths::install_dir);
        out.line("info", &format!("Steam: {}", show(steam.as_deref())));
        out.line("info", &format!("Deadlock: {}", show(install.as_deref())));

        let mut dirs = Vec::new();
        let mut watch = |root: PathBuf, label: &str, depth: usize, every: u64, list: bool, dump: bool| dirs.push(Watched { watch: DirWatch::new(root, label, depth, list), every, dump });
        let mut log_dirs = Vec::new();
        if let Some(install) = &install {
            log_dirs = paths::log_dirs(install);
            let citadel = paths::citadel_dir(install);
            watch(citadel.clone(), "game-folder", 0, 10, false, false);
            watch(citadel.join("cfg"), "game-cfg", 0, 10, false, false);
            watch(citadel.join("rpt"), "game-rpt", 2, 10, true, false);
            watch(citadel.join("save"), "game-save", 2, 10, true, false);
            for d in paths::replay_dirs(install) {
                watch(d, "replays", 1, 10, true, false);
            }
        }
        let mut steam_logs = Vec::new();
        if let Some(steam) = &steam {
            let options = paths::deadlock_launch_options(steam);
            let options = if options.is_empty() { "none saved by Steam (it saves them late; console.log needs -condebug)".to_string() } else { options.join(" | ") };
            out.line("info", &format!("Deadlock launch options: {options}"));
            for rec in paths::recording_dirs(steam) {
                watch(rec.clone(), "steam-recording", 0, 10, true, false);
                watch(rec.join("timelines"), "steam-timeline", 1, 10, true, true);
            }
            for user in paths::user_dirs(steam) {
                watch(user.join(paths::APP_ID), "steam-userdata", 3, 10, false, true);
            }
            watch(steam.join("appcache").join("httpcache"), "steam-httpcache", 1, 30, false, false);
            steam_logs = steam_log_files(&steam.join("logs")).into_iter().map(|p| Tail::new(p, true, false)).collect();
        }
        let mut s = Session { out, game_logs: Vec::new(), steam_logs, log_dirs, dirs, pids: Vec::new(), running: false, exited: None, windows: None, ticks: 0, busy: Duration::ZERO };
        s.discover(false);
        if s.game_logs.is_empty() {
            s.out.line("info", "no console log yet (it is written only with -condebug in Deadlock's launch options)");
        }
        Ok(s)
    }

    pub fn path(&self) -> &Path {
        &self.out.path
    }

    /// New `.log` files next to the game (console.log shows up when the game starts writing it).
    fn discover(&mut self, appeared: bool) {
        for dir in &self.log_dirs {
            let Ok(rd) = std::fs::read_dir(dir) else { continue };
            for p in rd.flatten().map(|e| e.path()) {
                let is_log = p.extension().is_some_and(|e| e.eq_ignore_ascii_case("log")) && p.is_file();
                if is_log && !self.game_logs.iter().any(|t| t.path() == p) {
                    self.game_logs.push(Tail::new(p, false, appeared));
                }
            }
        }
    }

    fn check_process(&mut self) {
        self.pids = platform::pids(paths::PROCESS_NAMES);
        if self.running == self.pids.is_empty() {
            self.running = !self.running;
            if self.running {
                self.exited = None;
                self.out.line("process", &format!("running (pid {:?})", self.pids));
            } else {
                self.exited = Some(Instant::now());
                self.windows = None;
                self.out.line("process", "exited");
                for t in &mut self.game_logs {
                    t.poll(&mut self.out);
                    t.flush(&mut self.out);
                }
            }
        }
    }

    fn look_at_windows(&mut self) {
        if !self.running {
            return;
        }
        let wins = platform::windows_of(&self.pids);
        let list: Vec<String> = wins.iter().map(|w| format!("\"{}\" [{}] {}x{}", w.title, w.class, w.width, w.height)).collect();
        let focus = if platform::foreground_is(&self.pids) { "yes" } else { "no" };
        let now = format!("focus={focus} {}", if list.is_empty() { "(no visible window)".to_string() } else { list.join(" | ") });
        if self.windows.as_deref() != Some(now.as_str()) {
            self.out.line("window", &now);
            self.windows = Some(now);
        }
    }

    fn look_at_dirs(&mut self, all: bool) {
        for w in &mut self.dirs {
            if all || self.ticks.is_multiple_of(w.every) {
                let touched = w.watch.poll(&mut self.out);
                for p in touched.iter().filter(|p| w.dump && wants_dump(p)) {
                    dump(p, &w.watch.label, &mut self.out);
                }
            }
        }
    }

    /// One look (every [`TICK`]): the console log every time, the rest less often.
    pub fn tick(&mut self) {
        let t = Instant::now();
        if self.ticks.is_multiple_of(10) {
            self.check_process();
            for tail in &mut self.steam_logs {
                tail.poll(&mut self.out);
            }
        }
        if self.ticks.is_multiple_of(50) && self.ticks > 0 {
            self.discover(true);
        }
        for tail in &mut self.game_logs {
            tail.poll(&mut self.out);
        }
        if self.ticks.is_multiple_of(3) {
            self.look_at_windows();
        }
        self.look_at_dirs(false);
        self.ticks += 1;
        self.busy += t.elapsed();
    }

    /// The game closed and the time for late files (Steam's timeline) has passed.
    pub fn done(&self) -> bool {
        self.exited.is_some_and(|t| t.elapsed() > AFTER_EXIT)
    }

    pub fn finish(mut self, why: &str) {
        for tail in self.game_logs.iter_mut().chain(self.steam_logs.iter_mut()) {
            tail.poll(&mut self.out);
        }
        self.look_at_dirs(true);
        let (busy, total) = (self.busy.as_secs_f64(), self.out.started.elapsed().as_secs_f64());
        self.out.line("info", &format!("end of the log: {why}. Looking took {busy:.2} s in total over {total:.0} s ({} looks).", self.ticks));
    }
}

/// Steam's own logs worth following: not the web helper's (long and unrelated) and not the
/// connection logs (network addresses, nothing about a match).
fn steam_log_files(dir: &Path) -> Vec<PathBuf> {
    const SKIPPED: &[&str] = &["webhelper", "cef", "connection_log"];
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let wanted = |p: &PathBuf| {
        let name = p.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        name.ends_with(".txt") && !name.contains(".previous") && !SKIPPED.iter().any(|s| name.starts_with(s))
    };
    let mut v: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(wanted).collect();
    v.sort();
    v
}

/// Runs for the life of the app (its own thread): while `enabled()` and Deadlock runs, one log
/// per game run. Idle cost: the process list every 2 s, and only while the option is on.
pub fn run(logs_dir: PathBuf, enabled: impl Fn() -> bool) {
    loop {
        if !enabled() || platform::pids(paths::PROCESS_NAMES).is_empty() {
            std::thread::sleep(IDLE);
            continue;
        }
        let mut session = match Session::start(&logs_dir) {
            Ok(s) => s,
            Err(e) => {
                log::warn!("Deadlock match signals: can't write a log in {}: {e}", logs_dir.display());
                std::thread::sleep(Duration::from_secs(30));
                continue;
            }
        };
        log::info!("Deadlock match signals: logging to {}", session.path().display());
        let why = loop {
            session.tick();
            if session.done() {
                break "Deadlock closed";
            }
            if !enabled() {
                break "the option was switched off";
            }
            std::thread::sleep(TICK);
        };
        session.finish(why);
        log::info!("Deadlock match signals: log finished ({why})");
    }
}

#[cfg(test)]
mod tests;
