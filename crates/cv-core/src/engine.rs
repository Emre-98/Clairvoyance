//! The recording engine: a small state machine that watches for supported games,
//! drives the recorder, collects events, syncs the game clock to the video, and
//! saves the session. It is completely game-agnostic.
//!
//! Performance: while idle it only looks at the process list every 2 seconds.
//! While a game runs it polls the game module about once per second.

use crate::events::{fmt_clock, EventKind, GameEvent};
use crate::game::{GameIntegration, KeyPress, MatchPhase, PlayerInfo, PlayerStats, SessionCheck, WatchKind};
use crate::modes::{GameModes, ModeRule};
use crate::recorder::{RecordOptions, Recorder, RecorderStatus};
use crate::session::{self, ClipInfo, GameSession, StatSample, CLIPS_DIR};
use crate::settings::{Hotkey, Settings};
use async_trait::async_trait;
use chrono::Local;
use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

/// OS services the engine needs. Implemented with Win32 APIs in the app.
pub trait Platform: Send + Sync {
    /// Lower-case executable names of running processes.
    fn running_processes(&self) -> Vec<String>;
    /// Lower-case executable name of the focused window's process.
    fn foreground_process(&self) -> Option<String>;
    /// Text-to-speech callout (must not block).
    fn speak(&self, text: &str, volume: u8);
    /// This app's own (cpu %, RAM MB).
    fn perf_sample(&self) -> Option<(f64, f64)>;
    /// Tells the input layer whether key events are wanted (only during games).
    fn set_input_enabled(&self, _enabled: bool) {}
    /// Starts recording the cursor, mouse buttons, wheel, window position and focus of the game
    /// window into `req.writer` (cursor-based games, while a video records). Must not affect the
    /// game: no hooks, low-priority thread.
    fn start_input_capture(&self, _req: crate::input::CaptureRequest) {}
    /// Stops it (waits for the capture thread) and says what it cost.
    fn stop_input_capture(&self) -> Option<crate::input::CaptureStats> {
        None
    }
}

/// Cuts a clip out of a recording (implemented with ffmpeg in the app).
#[async_trait]
pub trait ClipCutter: Send + Sync {
    async fn cut(&self, video: &Path, start: f64, end: f64, out: &Path, precise: bool) -> anyhow::Result<()>;
}

#[derive(Debug)]
pub enum EngineCommand {
    SaveClip,
    AddMarker,
    ReloadSettings(Box<Settings>),
    /// Ends the current session now (e.g. from the tray).
    StopSession,
    Shutdown,
}

/// A key going down or up (auto-repeat is filtered out by the input layer).
#[derive(Debug, Clone)]
pub struct InputEvent {
    /// Named key with modifiers (None for modifiers themselves and unnamed keys).
    pub key: Option<KeyPress>,
    /// Windows virtual-key code.
    pub vk: u16,
    pub down: bool,
    pub at: Instant,
    /// QPC timestamp in 100 ns units (the recorder's video clock base is in the same units).
    pub qpc_hns: i64,
}

impl InputEvent {
    /// A key press as older code paths saw it (tests, simulator).
    pub fn press(key: KeyPress, vk: u16, at: Instant, qpc_hns: i64) -> Self {
        InputEvent { key: Some(key), vk, down: true, at, qpc_hns }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum EngineState {
    #[default]
    Idle,
    /// Game running, not recording (auto-record off or recorder failed).
    Detected,
    Recording,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct LiveStatus {
    pub state: EngineState,
    pub game_id: Option<String>,
    pub game_name: Option<String>,
    pub phase: MatchPhase,
    pub game_time: Option<f64>,
    pub session_id: Option<String>,
    pub player: Option<PlayerInfo>,
    pub stats: Option<PlayerStats>,
    pub event_count: usize,
    pub last_event: Option<GameEvent>,
    pub video_offset: Option<f64>,
    pub recorder: RecorderStatus,
    pub message: Option<String>,
    /// The match's mode, e.g. "Ranked Solo/Duo".
    pub mode_name: Option<String>,
    /// What this mode's rule said: record / clips_only / off.
    pub mode_rule: Option<ModeRule>,
    /// A replay / spectating (not recorded): the tray and Home say so.
    pub watching: Option<WatchKind>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EngineEvent {
    Status(LiveStatus),
    GameEvent {
        session_id: String,
        event: GameEvent,
    },
    GameStarted {
        game_name: String,
    },
    GameEnded {
        session_id: String,
    },
    /// Session file changed (saved, clips added).
    LibraryChanged,
    /// A mode seen for the first time was added to the game's mode list (save the settings).
    ModesChanged {
        game_id: String,
        modes: GameModes,
    },
    /// A finished game's after-work (auto clips) is done: a good moment for thumbnails and the
    /// storage clean-up, which never run while a game is recording.
    PostProcessed {
        session_id: String,
    },
    Notice {
        level: String,
        text: String,
    },
}

struct Active {
    game: usize,
    session: GameSession,
    dir: PathBuf,
    seen: HashSet<String>,
    phase: MatchPhase,
    reached_in_progress: bool,
    recording: bool,
    rec_started: Option<Instant>,
    clock: Option<(Instant, f64)>,
    offset_samples: Vec<f64>,
    /// tokio's clock so tests can fast-forward time.
    ended_at: Option<tokio::time::Instant>,
    last_save: Instant,
    next_stat_sample: f64,
    missing_checks: u32,
    last_perf: Instant,
    last_event: Option<GameEvent>,
    /// This mode's rule: Off = watch the process only (no recording, no polling, nothing saved).
    rule: ModeRule,
    /// Clips-only mode: event clips waiting for their "seconds after".
    pending_clips: Vec<PendingClip>,
    /// Mouse and keyboard recording (cursor-based games, full recordings only).
    input: Option<Arc<crate::input::InputWriter>>,
    /// A replay / spectating: nothing recorded, nothing saved, waiting for the game to close.
    watch: Option<WatchKind>,
    /// Probably a replay (see `SessionCheck::Unsure`): polling the game's API, nothing recorded
    /// until it says whether this is a match you play.
    hold: Option<WatchKind>,
    /// A spectated game recorded because the setting allows it.
    spectating: bool,
    /// When the match was seen ending (victory screen) or the game closing (for the post-game
    /// timings in the log).
    over_at: Option<Instant>,
}

/// An automatic event clip in clips-only mode, saved from the replay buffer once `due`.
struct PendingClip {
    due: Instant,
    /// Game time of the first and last event in it.
    first_gt: f64,
    last_gt: f64,
    title: String,
}

pub struct Engine {
    games: Vec<Box<dyn GameIntegration>>,
    recorder: Arc<dyn Recorder>,
    platform: Arc<dyn Platform>,
    cutter: Option<Arc<dyn ClipCutter>>,
    settings: Settings,
    default_save_dir: PathBuf,
    tx: UnboundedSender<EngineEvent>,
    active: Option<Active>,
    recorder_status: RecorderStatus,
    message: Option<String>,
    hk_clip: Option<Hotkey>,
    hk_marker: Option<Hotkey>,
    /// After a match ends, the game process often stays open (victory screen, replays).
    /// Don't start a new session for that game until its process has exited.
    wait_for_exit: Option<usize>,
    /// Match-only game (see `GameIntegration::match_only`) whose process is running and
    /// is polled while idle to see when a match starts.
    probing: Option<usize>,
    /// Match-only game that just finished a match: wait until it's back in the menus.
    wait_for_lobby: Option<usize>,
}

const OFFSET_SAMPLES: usize = 9;
const STAT_SAMPLE_EVERY: f64 = 30.0;

impl Engine {
    pub fn new(
        games: Vec<Box<dyn GameIntegration>>,
        recorder: Arc<dyn Recorder>,
        platform: Arc<dyn Platform>,
        cutter: Option<Arc<dyn ClipCutter>>,
        settings: Settings,
        default_save_dir: PathBuf,
        tx: UnboundedSender<EngineEvent>,
    ) -> Self {
        let mut e = Self {
            games,
            recorder,
            platform,
            cutter,
            settings: Settings::default(),
            default_save_dir,
            tx,
            active: None,
            recorder_status: RecorderStatus::default(),
            message: None,
            hk_clip: None,
            hk_marker: None,
            wait_for_exit: None,
            probing: None,
            wait_for_lobby: None,
        };
        e.apply_settings(settings);
        e
    }

    fn apply_settings(&mut self, s: Settings) {
        for g in self.games.iter_mut() {
            let cfg = s.game_config(g.id(), g.default_config());
            g.configure(&cfg);
        }
        self.hk_clip = Hotkey::parse(&s.hotkey_clip);
        self.hk_marker = Hotkey::parse(&s.hotkey_marker);
        self.settings = s;
    }

    pub fn save_dir(&self) -> PathBuf {
        self.settings.save_dir_or(&self.default_save_dir)
    }

    fn emit(&self, e: EngineEvent) {
        let _ = self.tx.send(e);
    }

    fn notice(&self, level: &str, text: impl Into<String>) {
        let text = text.into();
        log::info!("[{level}] {text}");
        self.emit(EngineEvent::Notice { level: level.into(), text });
    }

    pub fn status(&self) -> LiveStatus {
        let mut st = LiveStatus { recorder: self.recorder_status.clone(), message: self.message.clone(), ..Default::default() };
        if let Some(a) = &self.active {
            let g = &self.games[a.game];
            st.state = if a.recording { EngineState::Recording } else { EngineState::Detected };
            st.game_id = Some(g.id().into());
            st.game_name = Some(g.name().into());
            st.phase = a.phase;
            st.game_time = a.clock.map(|(i, t)| t + i.elapsed().as_secs_f64());
            st.session_id = Some(a.session.id.clone());
            st.player = a.session.player.clone();
            st.stats = a.session.stats.clone();
            st.event_count = a.session.events.len();
            st.last_event = a.last_event.clone();
            st.video_offset = (!a.offset_samples.is_empty()).then_some(a.session.video_offset);
            st.mode_name = a.session.mode_name.clone();
            st.mode_rule = if a.hold.is_some() { None } else { Some(a.rule) };
            st.watching = a.watch;
        }
        st
    }

    fn emit_status(&self) {
        self.emit(EngineEvent::Status(self.status()));
    }

    /// Runs until `Shutdown`. Ends any running session cleanly first.
    pub async fn run(mut self, mut cmds: UnboundedReceiver<EngineCommand>, mut input: UnboundedReceiver<InputEvent>) {
        let mut ticker = tokio::time::interval(Duration::from_millis(1000));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut n: u64 = 0;
        self.emit_status();
        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    n += 1;
                    if self.active.is_some() {
                        self.tick_active(n).await;
                    } else if n.is_multiple_of(2) {
                        self.tick_idle().await;
                    }
                }
                Some(cmd) = cmds.recv() => {
                    match cmd {
                        EngineCommand::Shutdown => {
                            if self.active.is_some() { self.end_session().await; }
                            break;
                        }
                        other => self.handle_command(other).await,
                    }
                }
                Some(ev) = input.recv() => self.handle_input(ev).await,
            }
        }
    }

    async fn handle_command(&mut self, cmd: EngineCommand) {
        match cmd {
            EngineCommand::SaveClip => self.save_clip().await,
            EngineCommand::AddMarker => self.add_marker(Instant::now()),
            EngineCommand::ReloadSettings(s) => {
                self.apply_settings(*s);
                self.emit_status();
            }
            EngineCommand::StopSession => {
                if let Some(idx) = self.active.as_ref().map(|a| a.game) {
                    // Stopped by hand: don't restart until the game is closed / back in menus.
                    if self.games[idx].match_only() {
                        self.wait_for_lobby = Some(idx);
                    } else {
                        self.wait_for_exit = Some(idx);
                    }
                    self.end_session().await;
                }
            }
            EngineCommand::Shutdown => {}
        }
    }

    /// Returns the index of a running, enabled game.
    fn detect_game(&self) -> Option<usize> {
        let procs = self.platform.running_processes();
        self.games
            .iter()
            .position(|g| !self.settings.disabled_games.iter().any(|d| d == g.id()) && g.process_names().iter().any(|p| procs.iter().any(|r| r.eq_ignore_ascii_case(p))))
    }

    fn game_running(&self, idx: usize) -> bool {
        let procs = self.platform.running_processes();
        self.games[idx].process_names().iter().any(|p| procs.iter().any(|r| r.eq_ignore_ascii_case(p)))
    }

    async fn tick_idle(&mut self) {
        if let Some(idx) = self.wait_for_exit {
            if self.game_running(idx) {
                return;
            }
            self.wait_for_exit = None;
        }
        let Some(idx) = self.detect_game() else {
            if let Some(p) = self.probing.take() {
                self.games[p].stop().await;
            }
            self.wait_for_lobby = None;
            return;
        };
        if !self.games[idx].match_only() {
            self.start_session(idx).await;
            return;
        }
        // Match-only games: only start a session once a match is actually loading/live.
        if self.probing != Some(idx) {
            if let Some(p) = self.probing.take() {
                self.games[p].stop().await;
            }
            if let Err(e) = self.games[idx].start().await {
                log::warn!("game module start failed: {e:#}");
            }
            self.probing = Some(idx);
        }
        let phase = self.games[idx].poll().await.map(|u| u.phase).unwrap_or(MatchPhase::Waiting);
        if phase == MatchPhase::Waiting {
            if self.wait_for_lobby == Some(idx) {
                self.wait_for_lobby = None;
            }
            return;
        }
        if self.wait_for_lobby != Some(idx) && matches!(phase, MatchPhase::Loading | MatchPhase::InProgress) {
            self.probing = None;
            self.start_session(idx).await;
        }
    }

    fn record_options(&self, dir: &Path, full_video: bool) -> RecordOptions {
        let v = &self.settings.video;
        RecordOptions {
            output_dir: dir.to_path_buf(),
            encoder: v.encoder.clone(),
            quality: v.quality.clone(),
            fps: v.fps,
            height: v.height,
            replay_buffer_secs: v.replay_buffer_secs,
            record_mic: v.record_mic,
            display_capture: v.display_capture,
            full_video,
        }
    }

    async fn start_session(&mut self, idx: usize) {
        let started = Local::now();
        let root = self.save_dir();
        let (id, dir) = {
            let g = &self.games[idx];
            let base = session::session_folder_name(started, g.short_name());
            let dir = session::unique_path(&root, &base);
            (dir.file_name().unwrap().to_string_lossy().to_string(), dir)
        };
        if let Err(e) = std::fs::create_dir_all(&dir) {
            self.message = Some(format!("Can't create save folder {}: {e}", dir.display()));
            self.notice("error", self.message.clone().unwrap());
            return;
        }
        if let Err(e) = self.games[idx].start().await {
            log::warn!("game module start failed: {e:#}");
        }
        let (gid, gname) = {
            let g = &self.games[idx];
            (g.id(), g.name())
        };
        let session = GameSession::new(id.clone(), gid, gname, started);
        log::info!("{gname} detected, session {id}");
        let mut active = Active {
            game: idx,
            session,
            dir: dir.clone(),
            seen: HashSet::new(),
            phase: MatchPhase::Waiting,
            reached_in_progress: false,
            recording: false,
            rec_started: None,
            clock: None,
            offset_samples: Vec::new(),
            ended_at: None,
            last_save: Instant::now(),
            next_stat_sample: 0.0,
            missing_checks: 0,
            last_perf: Instant::now(),
            last_event: None,
            rule: ModeRule::Record,
            pending_clips: Vec::new(),
            input: None,
            watch: None,
            hold: None,
            spectating: false,
            over_at: None,
        };
        self.message = None;

        // A replay or spectating? Decided before anything is recorded (v1.7.1).
        let t0 = Instant::now();
        let check = self.games[idx].session_check().await;
        log::info!("session check: {check:?} in {} ms", t0.elapsed().as_millis());
        match check {
            SessionCheck::Watching(WatchKind::Spectate) if self.games[idx].record_spectating() => {
                log::info!("spectating a live game: recorded (setting \"Record games you spectate\" is on)");
                active.spectating = true;
            }
            SessionCheck::Watching(k) => {
                self.enter_watch(&mut active, k);
                self.platform.set_input_enabled(false);
                self.active = Some(active);
                self.emit(EngineEvent::GameStarted { game_name: gname.into() });
                self.emit_status();
                return;
            }
            SessionCheck::Unsure(k) => {
                // Not recorded until the game's API says this is a match you play.
                log::info!("probably {} (no game session in the client): not recording until the game confirms", k.label().to_lowercase());
                active.hold = Some(k);
                self.message = Some("Checking whether this is a replay…".into());
                self.platform.set_input_enabled(false);
                self.active = Some(active);
                self.emit(EngineEvent::GameStarted { game_name: gname.into() });
                self.emit_status();
                return;
            }
            SessionCheck::Playing | SessionCheck::Unknown => {}
        }
        self.begin(&mut active).await;
        self.active = Some(active);
        self.emit(EngineEvent::GameStarted { game_name: gname.into() });
        self.emit_status();
    }

    /// A replay / spectating: nothing recorded or saved; only the tray / Home say why.
    fn enter_watch(&mut self, a: &mut Active, k: WatchKind) {
        a.watch = Some(k);
        a.hold = None;
        a.rule = ModeRule::Off;
        a.recording = false;
        self.message = Some(format!("{}: not recorded", k.label()));
        log::info!("{}: not recorded", k.label());
        let _ = std::fs::remove_dir_all(&a.dir);
    }

    /// Decides the mode's rule, then starts recording (unless the mode is off).
    async fn begin(&mut self, active: &mut Active) {
        let idx = active.game;
        let (gid, capture) = {
            let g = &self.games[idx];
            (g.id(), g.capture())
        };
        let dir = active.dir.clone();
        // Which mode is this, and is it recorded? Decided once, before recording starts.
        let mut rule = ModeRule::Record;
        let mut reason: Option<String> = None;
        if !self.games[idx].mode_groups().is_empty() {
            let t0 = Instant::now();
            let mode = self.games[idx].detect_mode().await;
            match &mode {
                Some(m) => log::info!(
                    "mode: {} (queue {}, key {}, game mode {}) via {} in {} ms",
                    m.name,
                    m.queue_id.map(|q| q.to_string()).unwrap_or("-".into()),
                    m.key.as_deref().unwrap_or("-"),
                    m.game_mode.as_deref().unwrap_or("-"),
                    m.source,
                    t0.elapsed().as_millis()
                ),
                None => log::info!("mode: unknown (League client and game API not reachable); using the \"unknown / new modes\" rule"),
            }
            let modes = self.settings.modes.entry(gid.to_string()).or_default();
            let d = modes.decide(mode.as_ref());
            if d.added {
                let snapshot = modes.clone();
                self.emit(EngineEvent::ModesChanged { game_id: gid.to_string(), modes: snapshot });
            }
            log::info!("mode rule: {}", d.reason);
            rule = d.rule;
            reason = Some(d.reason);
            if let Some(m) = &mode {
                active.session.queue_id = m.queue_id;
                active.session.mode_name = Some(m.name.clone());
                active.session.mode_key = m.key.clone();
            }
        }
        if active.spectating {
            active.session.mode_name = Some(match active.session.mode_name.take() {
                Some(m) => format!("Spectating · {m}"),
                None => "Spectating".into(),
            });
        }
        active.rule = rule;
        if rule != ModeRule::Off {
            let _ = active.session.save(&dir);
        }
        if rule == ModeRule::Off {
            // Not recorded: no recorder, no polling, nothing saved. Only the tray says why.
            self.message = reason.clone();
            let _ = std::fs::remove_dir_all(&dir);
        } else if self.settings.auto_record {
            active.session.record_mode = Some(if rule == ModeRule::ClipsOnly { "clips_only" } else { "full" }.into());
            let opts = self.record_options(&dir, rule == ModeRule::Record);
            let res = async {
                self.recorder.ensure_connected().await?;
                self.recorder.prepare(&capture, &opts).await?;
                self.recorder.start_recording().await
            }
            .await;
            match res {
                Ok(()) => {
                    active.recording = true;
                    active.rec_started = Some(Instant::now());
                    if rule == ModeRule::Record {
                        active.input = self.start_input(idx, &mut active.session, &dir);
                    }
                }
                Err(e) => {
                    let msg = format!("Recording couldn't start: {e:#}");
                    active.session.warnings.push(msg.clone());
                    self.message = Some(msg.clone());
                    self.notice("error", msg);
                }
            }
            self.recorder_status = self.recorder.status().await;
        }
        self.platform.set_input_enabled(rule != ModeRule::Off);
    }

    /// A replay / spectating found out only after recording started (the game's API answers a
    /// few seconds after the process appears): everything recorded so far is deleted.
    async fn abort_watch(&mut self, k: WatchKind) {
        let Some(mut a) = self.active.take() else { return };
        let since = a.rec_started.map(|r| r.elapsed().as_secs_f64()).unwrap_or(0.0);
        if let Some(w) = a.input.take() {
            let _ = self.platform.stop_input_capture();
            let _ = w.finish(0);
        }
        self.platform.set_input_enabled(false);
        a.pending_clips.clear();
        if a.recording {
            if let Err(e) = self.recorder.stop_recording().await {
                log::debug!("stopping the recording of a replay: {e:#}");
            }
            let _ = self.recorder.finish().await;
            self.recorder_status = self.recorder.status().await;
        }
        a.recording = false;
        // Never listed in the library from now on (it lists folders with a session.json), and
        // removed by the maintenance pass if a file can't be deleted right now.
        let _ = std::fs::write(a.dir.join(session::DISCARDED_MARK), b"replay or spectating: not recorded");
        let _ = std::fs::remove_file(a.dir.join(session::SESSION_FILE));
        // The recorder has closed its files; an antivirus scan may hold one for a moment.
        let mut gone = false;
        for _ in 0..20 {
            match std::fs::remove_dir_all(&a.dir) {
                Ok(()) => {
                    gone = true;
                    break;
                }
                Err(_) if !a.dir.exists() => {
                    gone = true;
                    break;
                }
                Err(_) => tokio::time::sleep(Duration::from_millis(250)).await,
            }
        }
        log::info!(
            "{} detected {since:.1} s after recording started: the partial recording was deleted{}",
            k.label().to_lowercase(),
            if gone { "" } else { " (some files were still in use; the folder is removed at the next start)" }
        );
        self.enter_watch(&mut a, k);
        self.active = Some(a);
        self.emit_status();
    }

    /// Starts the mouse/keyboard recording for a cursor-based game whose video records, if the
    /// game's setting allows it. The file is named after the recording and set in the session
    /// at once, so a crash keeps it linked.
    fn start_input(&self, idx: usize, session: &mut GameSession, dir: &Path) -> Option<Arc<crate::input::InputWriter>> {
        let g = &self.games[idx];
        if !g.input_tracking() {
            return None;
        }
        let mut def = crate::input::default_config();
        if let (Some(d), Some(o)) = (def.as_object_mut(), g.default_config().as_object()) {
            for (k, v) in o {
                d.insert(k.clone(), v.clone());
            }
        }
        let (on, rate) = crate::input::settings_from(&self.settings.game_config(g.id(), def));
        if !on {
            log::info!("input recording off for {}", g.name());
            return None;
        }
        let Some(base) = self.recorder.clock_base_hns() else {
            log::warn!("input recording: the recorder has no video clock");
            return None;
        };
        let name = format!("{}.{}", session.id, crate::input::EXT);
        let meta = crate::input::Meta { game_id: g.id().into(), rate, app_version: env!("CARGO_PKG_VERSION").into() };
        match crate::input::InputWriter::create(&dir.join(&name), base, rate, &meta) {
            Ok(w) => {
                let w = Arc::new(w);
                session.input_file = Some(name);
                // The binds in effect for this game (read by the module at `start()`), so the
                // replay's ability bubbles show the right actions even if they change later.
                let keys = g.action_keys();
                session.action_keys = (!keys.is_empty()).then_some(keys);
                let _ = session.save(dir);
                self.platform
                    .start_input_capture(crate::input::CaptureRequest { writer: w.clone(), process_names: g.process_names().iter().map(|p| p.to_lowercase()).collect() });
                log::info!("input recording on ({rate} Hz cursor)");
                Some(w)
            }
            Err(e) => {
                log::warn!("input recording: can't create {name}: {e}");
                None
            }
        }
    }

    /// Best estimate of the in-game clock at `at`.
    fn game_time_at(&self, at: Instant) -> f64 {
        let Some(a) = &self.active else { return 0.0 };
        if let Some((i, t)) = a.clock {
            let dt = if at >= i { (at - i).as_secs_f64() } else { -((i - at).as_secs_f64()) };
            return (t + dt).max(0.0);
        }
        // No game clock (game without an API): use the recording time.
        a.rec_started.map(|r| at.saturating_duration_since(r).as_secs_f64() - a.session.video_offset).unwrap_or(0.0).max(0.0)
    }

    async fn tick_active(&mut self, n: u64) {
        let idx = self.active.as_ref().unwrap().game;
        if self.active.as_ref().unwrap().hold.is_some() {
            self.tick_hold(n).await;
            return;
        }
        if self.active.as_ref().unwrap().rule == ModeRule::Off {
            // Mode switched off: just wait for the game to close (every 2 s).
            if n.is_multiple_of(2) {
                let running = self.game_running(idx);
                let a = self.active.as_mut().unwrap();
                a.missing_checks = if running { 0 } else { a.missing_checks + 1 };
                if a.missing_checks >= 2 {
                    self.wait_for_exit = None;
                    self.end_session().await;
                }
            }
            return;
        }
        let update = match self.games[idx].poll().await {
            Ok(u) => Some(u),
            Err(e) => {
                log::debug!("poll: {e:#}");
                None
            }
        };
        // Spectator mode found by the game's API after recording started: delete it all.
        if let Some(k) = update.as_ref().and_then(|u| u.watching) {
            let spectating_ok = k == WatchKind::Spectate && self.games[idx].record_spectating();
            if !spectating_ok {
                self.abort_watch(k).await;
                return;
            }
        }
        let recording = self.active.as_ref().unwrap().recording;
        // Ask the recorder for its own elapsed time right after the poll (used for the offset).
        // Only sample while the clock runs (League reports 0:00 during the loading screen).
        let rec_elapsed = if recording && update.as_ref().is_some_and(|u| u.game_time.is_some_and(|t| t > 0.5)) && self.active.as_ref().unwrap().offset_samples.len() < OFFSET_SAMPLES {
            match self.recorder.record_elapsed().await {
                Ok(Some(d)) => Some(d.as_secs_f64()),
                _ => self.active.as_ref().unwrap().rec_started.map(|r| r.elapsed().as_secs_f64()),
            }
        } else {
            None
        };

        let update_failed = update.is_none();
        let mut new_events = Vec::new();
        if let Some(u) = update {
            let a = self.active.as_mut().unwrap();
            if u.phase != a.phase {
                log::info!("phase {:?} -> {:?}", a.phase, u.phase);
                a.phase = u.phase;
            }
            if u.phase == MatchPhase::InProgress {
                a.reached_in_progress = true;
            }
            if let Some(gt) = u.game_time {
                a.clock = Some((Instant::now(), gt));
                if let Some(el) = rec_elapsed.filter(|_| gt > 0.5) {
                    a.offset_samples.push(el - gt);
                    // Negative when the recording started after the game clock (app started or
                    // restarted mid-game): events before the video then sit before its start.
                    a.session.video_offset = median(&a.offset_samples);
                }
                if gt >= a.next_stat_sample {
                    if let Some(st) = &u.stats {
                        a.session.timeline.push(StatSample { t: gt, kills: st.kills, deaths: st.deaths, assists: st.assists, cs: st.cs, gold: st.gold });
                    }
                    a.next_stat_sample = (gt / STAT_SAMPLE_EVERY).floor() * STAT_SAMPLE_EVERY + STAT_SAMPLE_EVERY;
                }
            }
            if let Some(mut p) = u.player {
                // The exact queue name ("Ranked Solo/Duo") beats the coarse one from the game API.
                if a.session.mode_name.is_some() {
                    p.mode = a.session.mode_name.clone();
                }
                a.session.player = Some(p);
            }
            if let Some(s) = u.stats {
                a.session.stats = Some(s);
            }
            if u.result.is_some() {
                a.session.result = u.result;
            }
            for ev in u.events {
                if a.seen.insert(ev.id.clone()) {
                    new_events.push(ev);
                }
            }
            if a.phase == MatchPhase::Ended && a.ended_at.is_none() {
                a.ended_at = Some(tokio::time::Instant::now());
                a.over_at.get_or_insert_with(Instant::now);
                a.session.game_duration = a.clock.map(|(_, t)| t);
                log::info!("match over (victory/defeat screen): stopping in {} ms", self.games[idx].end_grace().as_millis());
            }
        }
        for ev in new_events {
            self.push_event(ev);
        }
        self.save_due_event_clips(false).await;

        // Performance debug stat every 5 s.
        if self.active.as_ref().unwrap().last_perf.elapsed() >= Duration::from_secs(5) {
            let sample = self.platform.perf_sample();
            let a = self.active.as_mut().unwrap();
            a.last_perf = Instant::now();
            if let Some((cpu, ram)) = sample {
                a.session.perf.add(cpu, ram);
            }
        }

        // Crash safety: save the session every 20 s.
        {
            let a = self.active.as_mut().unwrap();
            if a.last_save.elapsed() >= Duration::from_secs(20) {
                let _ = a.session.save(&a.dir);
                a.last_save = Instant::now();
            }
        }

        // End conditions: match over (after a grace period) or the process is gone.
        let grace = self.games[idx].end_grace();
        let ended = self.active.as_ref().unwrap().ended_at.is_some_and(|t| t.elapsed() >= grace);
        let mut gone = false;
        // The process list every 2 s; every second while the game's API doesn't answer (it
        // stops answering the moment the game closes), so leaving the game is noticed fast.
        if n.is_multiple_of(2) || update_failed {
            let running = self.game_running(idx);
            let a = self.active.as_mut().unwrap();
            a.missing_checks = if running { 0 } else { a.missing_checks + 1 };
            gone = a.missing_checks >= 2 || (update_failed && a.missing_checks >= 1 && a.reached_in_progress);
            if gone {
                a.over_at.get_or_insert_with(Instant::now);
            }
        }
        let match_only = self.games[idx].match_only();
        let left_match = {
            let a = self.active.as_ref().unwrap();
            match_only && a.reached_in_progress && a.phase == MatchPhase::Waiting
        };
        if ended || gone || left_match {
            if ended && !gone {
                if match_only {
                    self.wait_for_lobby = Some(idx);
                } else {
                    self.wait_for_exit = Some(idx);
                }
            }
            self.end_session().await;
        } else {
            self.emit_status();
        }
    }

    /// Probably a replay (no game session in the client): poll the game's API until it says
    /// whether this is spectator mode (never recorded) or a match you play (recording starts).
    async fn tick_hold(&mut self, n: u64) {
        let idx = self.active.as_ref().unwrap().game;
        let update = self.games[idx].poll().await.ok();
        if let Some(u) = &update {
            let watching = u.watching.filter(|k| !(*k == WatchKind::Spectate && self.games[idx].record_spectating()));
            if let Some(k) = watching {
                let mut a = self.active.take().unwrap();
                self.enter_watch(&mut a, k);
                self.active = Some(a);
                self.emit_status();
                return;
            }
            if u.playing || u.watching.is_some() {
                let mut a = self.active.take().unwrap();
                let waited = a.session.started_at.signed_duration_since(Local::now()).num_seconds().abs();
                if u.watching.is_some() {
                    log::info!("spectating a live game: recorded (setting \"Record games you spectate\" is on)");
                    a.spectating = true;
                } else {
                    log::info!("the game confirms a match you play: recording starts now ({waited} s after the game started)");
                }
                a.hold = None;
                self.message = None;
                self.begin(&mut a).await;
                self.active = Some(a);
                self.emit_status();
                return;
            }
        }
        if n.is_multiple_of(2) || update.is_none() {
            let running = self.game_running(idx);
            let a = self.active.as_mut().unwrap();
            a.missing_checks = if running { 0 } else { a.missing_checks + 1 };
            if a.missing_checks >= 2 {
                self.end_session().await;
                return;
            }
        }
        self.emit_status();
    }

    fn push_event(&mut self, ev: GameEvent) {
        let s = &self.settings.events;
        if s.tts_enabled && s.tts_kinds.contains(&ev.kind) {
            let text = if ev.kind == EventKind::Multikill || ev.steal { ev.title.clone() } else { ev.kind.callout().to_string() };
            self.platform.speak(&text, s.tts_volume);
        }
        let clip_kind = s.clip_kinds.contains(&ev.kind);
        let (before, after) = (s.clip_before_secs, s.clip_after_secs);
        let a = self.active.as_mut().unwrap();
        if a.rule == ModeRule::ClipsOnly && a.recording && clip_kind {
            // Clips only: save this moment from the replay buffer a few seconds later.
            let due = Instant::now() + Duration::from_secs_f64(after.max(1.0));
            match a.pending_clips.last_mut() {
                Some(p) if ev.game_time - p.last_gt <= before + after => {
                    p.due = due;
                    p.last_gt = ev.game_time;
                    if !p.title.contains(&ev.title) && p.title.len() < 60 {
                        p.title = format!("{} + {}", p.title, ev.title);
                    }
                }
                _ => a.pending_clips.push(PendingClip { due, first_gt: ev.game_time, last_gt: ev.game_time, title: ev.title.clone() }),
            }
        }
        a.seen.insert(ev.id.clone());
        a.session.events.push(ev.clone());
        a.last_event = Some(ev.clone());
        let sid = a.session.id.clone();
        self.emit(EngineEvent::GameEvent { session_id: sid, event: ev });
    }

    async fn handle_input(&mut self, ev: InputEvent) {
        let Some(a) = &self.active else { return };
        let idx = a.game;
        let writer = a.input.clone();
        let chat_before = writer.is_some() && self.games[idx].chat_open();
        // Hotkeys, game keys (ult): key-downs with a name, exactly as before input recording.
        if ev.down {
            if let Some(key) = &ev.key {
                self.handle_key(key.clone(), ev.at).await;
            }
        }
        // Input recording: every key down/up while the game window is focused, except while
        // the chat is open (key codes only, never text).
        let Some(w) = writer else { return };
        let chat_after = self.games[idx].chat_open();
        let t = w.video_us(ev.qpc_hns);
        if chat_after != chat_before {
            w.push(crate::input::Record::Chat { t, open: chat_after });
        }
        if w.is_focused() && !chat_before && !chat_after && ev.vk > 0 && ev.vk < 256 {
            w.push(crate::input::Record::Key { t, vk: ev.vk as u8, down: ev.down });
        }
    }

    async fn handle_key(&mut self, key: KeyPress, at: Instant) {
        let Some(a) = &self.active else { return };
        if self.hk_clip.as_ref().is_some_and(|h| h.matches(&key)) {
            self.save_clip().await;
            return;
        }
        if self.hk_marker.as_ref().is_some_and(|h| h.matches(&key)) {
            self.add_marker(at);
            return;
        }
        if a.phase != MatchPhase::InProgress {
            return;
        }
        let idx = a.game;
        // Game keys only count while the game window is focused.
        let fg = self.platform.foreground_process();
        let focused = fg.is_some_and(|f| self.games[idx].process_names().iter().any(|p| p.eq_ignore_ascii_case(&f)));
        if !focused {
            return;
        }
        let gt = self.game_time_at(at);
        let event = self.games[idx].on_key(&key, gt);
        let marks = self.games[idx].take_key_marks();
        if let Some(a) = self.active.as_mut() {
            a.session.key_presses.extend(marks);
        }
        if let Some(e) = event {
            self.push_event(e);
            self.emit_status();
        }
    }

    fn add_marker(&mut self, at: Instant) {
        if self.active.is_none() {
            return;
        }
        let gt = self.game_time_at(at);
        let id = format!("marker-{}", self.active.as_ref().unwrap().session.events.len());
        let ev = GameEvent::new(id, EventKind::ManualMarker, gt, format!("Marker at {}", fmt_clock(gt)));
        self.push_event(ev);
        self.emit_status();
    }

    async fn save_clip(&mut self) {
        let Some(a) = &self.active else {
            self.notice("warn", "No game is running, so there's nothing to clip.");
            return;
        };
        if !a.recording {
            self.notice("warn", "Not recording, so the clip couldn't be saved.");
            return;
        }
        let gt = self.game_time_at(Instant::now());
        let clips_only = a.rule == ModeRule::ClipsOnly;
        let video_end = match self.recorder.record_elapsed().await {
            Ok(Some(d)) => Some(d.as_secs_f64()),
            _ => a.rec_started.map(|r| r.elapsed().as_secs_f64()),
        };
        match self.recorder.save_replay(None).await {
            Ok(path) => {
                let a = self.active.as_mut().unwrap();
                let clips = a.dir.join(CLIPS_DIR);
                let _ = std::fs::create_dir_all(&clips);
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("mp4").to_string();
                let name = format!("clip_{}.{ext}", fmt_clock(gt).replace(':', "-"));
                let dest = session::unique_path(&clips, &name);
                let moved = move_file(&path, &dest).await;
                let final_path = if moved { dest } else { path };
                let len = self.settings.video.replay_buffer_secs as f64;
                a.session.clips.push(ClipInfo {
                    file: final_path.file_name().unwrap().to_string_lossy().to_string(),
                    title: format!("Clip at {}", fmt_clock(gt)),
                    // Clips-only games have no full video to place the clip in.
                    video_start: if clips_only { None } else { video_end.map(|e| (e - len).max(0.0)) },
                    video_end: if clips_only { None } else { video_end },
                    created_at: Local::now(),
                    source: "replay".into(),
                    keep: false,
                });
                let id = format!("clip-{}", a.session.clips.len());
                let ev = GameEvent::new(id, EventKind::Clip, gt, format!("Clip saved at {}", fmt_clock(gt)));
                let _ = a.session.save(&a.dir);
                self.push_event(ev);
                self.emit(EngineEvent::LibraryChanged);
            }
            Err(e) => self.notice("error", format!("Clip failed: {e:#}")),
        }
        self.emit_status();
    }

    /// Clips-only mode: saves the event clips whose "seconds after" have passed (all of them if
    /// `all`, when the game ends).
    async fn save_due_event_clips(&mut self, all: bool) {
        let now = Instant::now();
        let Some(a) = self.active.as_mut() else { return };
        if a.pending_clips.is_empty() || !a.recording {
            return;
        }
        let (due, keep): (Vec<PendingClip>, Vec<PendingClip>) = std::mem::take(&mut a.pending_clips).into_iter().partition(|p| all || p.due <= now);
        a.pending_clips = keep;
        let ev = self.settings.events.clone();
        for p in due {
            let secs = ((p.last_gt - p.first_gt) + ev.clip_before_secs + ev.clip_after_secs).ceil().max(3.0) as u32;
            match self.recorder.save_replay(Some(secs)).await {
                Ok(path) => {
                    let a = self.active.as_mut().unwrap();
                    let clips = a.dir.join(CLIPS_DIR);
                    let _ = std::fs::create_dir_all(&clips);
                    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("mp4").to_string();
                    let name = format!("{}_{}.{ext}", session::sanitize(&p.title), fmt_clock(p.first_gt).replace(':', "-"));
                    let dest = session::unique_path(&clips, &name);
                    let final_path = if move_file(&path, &dest).await { dest } else { path };
                    a.session.clips.push(ClipInfo {
                        file: final_path.file_name().unwrap().to_string_lossy().to_string(),
                        title: p.title.clone(),
                        video_start: None,
                        video_end: None,
                        created_at: Local::now(),
                        source: "event".into(),
                        keep: false,
                    });
                    let _ = a.session.save(&a.dir);
                    log::info!("event clip saved ({secs} s): {}", p.title);
                }
                Err(e) => log::warn!("event clip failed: {e:#}"),
            }
        }
        self.emit(EngineEvent::LibraryChanged);
    }

    async fn end_session(&mut self) {
        if self.active.as_ref().is_some_and(|a| a.rule == ModeRule::ClipsOnly) {
            self.save_due_event_clips(true).await;
        }
        let Some(mut a) = self.active.take() else { return };
        if a.rule == ModeRule::Off || a.hold.is_some() {
            self.games[a.game].stop().await;
            self.platform.set_input_enabled(false);
            if a.hold.is_some() {
                // Closed before the game said what it was: nothing was recorded.
                let _ = std::fs::remove_dir_all(&a.dir);
            }
            log::info!("{} ended (not recorded: {})", a.session.game_name, self.message.clone().unwrap_or_default());
            self.message = None;
            self.emit_status();
            return;
        }
        self.platform.set_input_enabled(false);
        let idx = a.game;
        let short = self.games[idx].short_name();
        if let Some(w) = a.input.take() {
            let end_us = match self.recorder.record_elapsed().await {
                Ok(Some(d)) => d.as_micros() as i64,
                _ => a.rec_started.map(|r| r.elapsed().as_micros() as i64).unwrap_or(0),
            };
            let cost = self.platform.stop_input_capture();
            let (bytes, n) = w.finish(end_us);
            match &cost {
                Some(c) => log::info!(
                    "input recording: {n} records, {:.0} KB; capture thread {:.0} ms CPU in {:.0} s ({:.3}% of one core), {} cursor samples, {} raw mouse messages",
                    bytes as f64 / 1024.0,
                    c.cpu_ms,
                    c.wall_secs,
                    c.core_percent(),
                    c.cursor_samples,
                    c.raw_mouse_msgs
                ),
                None => log::info!("input recording: {n} records, {:.0} KB", bytes as f64 / 1024.0),
            }
            // Mouse buttons bound to a tracked game key (League: ult on a side button).
            let presses: Vec<(f64, u8)> = w.button_downs().into_iter().map(|(t, b)| (t - a.session.video_offset, b)).collect();
            let marks = self.games[idx].mouse_marks(&presses);
            if !marks.is_empty() {
                log::info!("{} mouse presses on tracked binds", marks.len());
                a.session.key_presses.extend(marks);
            }
        }
        let over = *a.over_at.get_or_insert_with(Instant::now);
        if a.recording {
            let elapsed = a.rec_started.map(|r| r.elapsed().as_secs_f64());
            let t_stop = Instant::now();
            let stopped = self.recorder.stop_recording().await;
            log::info!("post-game: recording stopped and made playable in {} ms ({} ms after the match ended)", t_stop.elapsed().as_millis(), over.elapsed().as_millis());
            match stopped {
                Ok(_) if a.rule == ModeRule::ClipsOnly => {
                    log::info!("clips-only game ended: {} clips", a.session.clips.len());
                }
                Ok(path) => {
                    a.session.video_duration = elapsed;
                    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("mp4").to_string();
                    let dest = session::unique_path(&a.dir, &a.session.video_name(short, &ext));
                    let final_path = if move_file(&path, &dest).await { dest } else { path };
                    a.session.video_file =
                        Some(if final_path.parent() == Some(a.dir.as_path()) { final_path.file_name().unwrap().to_string_lossy().to_string() } else { final_path.to_string_lossy().to_string() });
                }
                Err(e) => {
                    let msg = format!("Stopping the recording failed: {e:#}");
                    a.session.warnings.push(msg.clone());
                    self.notice("error", msg);
                }
            }
            if let Err(e) = self.recorder.finish().await {
                log::warn!("recorder finish: {e:#}");
            }
        }
        self.games[idx].stop().await;
        a.session.ended_at = Some(Local::now());
        if a.session.game_duration.is_none() {
            a.session.game_duration = a.clock.map(|(i, t)| t + i.elapsed().as_secs_f64());
        }
        self.recorder_status = self.recorder.status().await;

        let too_short = !a.reached_in_progress && a.session.events.is_empty() && a.session.clips.is_empty() && a.session.video_duration.unwrap_or(0.0) < 30.0;
        let sid = a.session.id.clone();
        if too_short {
            log::info!("discarding short session {sid}");
            let _ = std::fs::remove_dir_all(&a.dir);
        } else {
            if let Err(e) = a.session.save(&a.dir) {
                self.notice("error", format!("Couldn't save the game: {e:#}"));
            }
            log::info!("post-game: {sid} saved and in the library {} ms after the match ended", over.elapsed().as_millis());
            self.emit(EngineEvent::GameEnded { session_id: sid.clone() });
            self.spawn_post_process(a.dir.clone(), sid);
        }
        self.emit(EngineEvent::LibraryChanged);
        self.emit_status();
    }

    /// After a game: cut automatic event clips (stream copy, low priority). Thumbnails and the
    /// storage clean-up follow in the app once this reports `PostProcessed`.
    fn spawn_post_process(&self, dir: PathBuf, sid: String) {
        let cutter = self.cutter.clone();
        let ev = self.settings.events.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            if let (Some(cutter), false) = (cutter, ev.clip_kinds.is_empty()) {
                if let Err(e) = auto_clips(&*cutter, &dir, &ev).await {
                    log::warn!("auto clips: {e:#}");
                    let _ = tx.send(EngineEvent::Notice { level: "warn".into(), text: format!("Event clips weren't created: {e:#}") });
                }
            }
            let _ = tx.send(EngineEvent::LibraryChanged);
            let _ = tx.send(EngineEvent::PostProcessed { session_id: sid });
        });
    }
}

/// Merges overlapping clip windows and cuts one clip per window.
pub async fn auto_clips(cutter: &dyn ClipCutter, dir: &Path, ev: &crate::settings::EventSettings) -> anyhow::Result<()> {
    let session = GameSession::load(dir)?;
    let Some(video) = session.video_file.as_ref().map(|f| dir.join(f)) else { return Ok(()) };
    if !video.exists() {
        return Ok(());
    }
    let windows = clip_windows(&session, ev);
    if windows.is_empty() {
        return Ok(());
    }
    let clips_dir = dir.join(CLIPS_DIR);
    std::fs::create_dir_all(&clips_dir)?;
    let ext = video.extension().and_then(|e| e.to_str()).unwrap_or("mp4").to_string();
    let mut new_clips = Vec::new();
    for (start, end, title) in windows {
        let name = format!("{}_{}.{ext}", session::sanitize(&title), fmt_clock(start - session.video_offset).replace(':', "-"));
        let out = session::unique_path(&clips_dir, &name);
        cutter.cut(&video, start, end, &out, false).await?;
        new_clips.push(ClipInfo {
            file: out.file_name().unwrap().to_string_lossy().to_string(),
            title,
            video_start: Some(start),
            video_end: Some(end),
            created_at: Local::now(),
            source: "event".into(),
            keep: false,
        });
    }
    // Reload in case the UI changed the session meanwhile (e.g. favorite).
    let mut session = GameSession::load(dir)?;
    session.clips.extend(new_clips);
    session.save(dir)?;
    Ok(())
}

/// (video start, video end, title) for each auto clip, overlapping windows merged.
pub fn clip_windows(s: &GameSession, ev: &crate::settings::EventSettings) -> Vec<(f64, f64, String)> {
    let mut wins: Vec<(f64, f64, String)> = Vec::new();
    let mut events: Vec<&GameEvent> = s.events.iter().filter(|e| ev.clip_kinds.contains(&e.kind)).collect();
    events.sort_by(|a, b| a.game_time.total_cmp(&b.game_time));
    for e in events {
        let pos = s.to_video_position(e.game_time);
        let (start, end) = ((pos - ev.clip_before_secs).max(0.0), pos + ev.clip_after_secs);
        if let Some(last) = wins.last_mut() {
            if start <= last.1 {
                last.1 = last.1.max(end);
                if !last.2.contains(&e.title) && last.2.len() < 60 {
                    last.2 = format!("{} + {}", last.2, e.title);
                }
                continue;
            }
        }
        wins.push((start, end, e.title.clone()));
    }
    if let Some(d) = s.video_duration {
        for w in wins.iter_mut() {
            w.1 = w.1.min(d);
        }
        wins.retain(|w| w.1 - w.0 > 1.0);
    }
    wins
}

fn median(v: &[f64]) -> f64 {
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.total_cmp(b));
    if s.is_empty() {
        0.0
    } else if s.len() % 2 == 1 {
        s[s.len() / 2]
    } else {
        (s[s.len() / 2 - 1] + s[s.len() / 2]) / 2.0
    }
}

/// Renames a file, retrying for a while because the recorder may still be finishing it.
/// Falls back to copy+delete across drives. Returns false if it couldn't be moved.
async fn move_file(from: &Path, to: &Path) -> bool {
    if from == to {
        return true;
    }
    for _ in 0..40 {
        match std::fs::rename(from, to) {
            Ok(()) => return true,
            Err(e) => {
                if !from.exists() {
                    return false;
                }
                // Different drive: copy then delete.
                if is_cross_device(&e) && std::fs::copy(from, to).is_ok() {
                    let _ = std::fs::remove_file(from);
                    return true;
                }
                log::debug!("rename {} failed ({e}), retrying", from.display());
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
    }
    false
}

fn is_cross_device(e: &std::io::Error) -> bool {
    // ERROR_NOT_SAME_DEVICE on Windows, EXDEV elsewhere.
    if cfg!(windows) {
        e.raw_os_error() == Some(17)
    } else {
        e.raw_os_error() == Some(18)
    }
}

#[cfg(test)]
mod tests;
