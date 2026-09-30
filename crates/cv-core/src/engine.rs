//! The recording engine: a small state machine that watches for supported games,
//! drives the recorder, collects events, syncs the game clock to the video, and
//! saves the session. It is completely game-agnostic.
//!
//! Performance: while idle it only looks at the process list every 2 seconds.
//! While a game runs it polls the game module about once per second.

use crate::events::{fmt_clock, EventKind, GameEvent};
use crate::game::{GameIntegration, KeyPress, MatchPhase, PlayerInfo, PlayerStats};
use crate::recorder::{RecordOptions, Recorder, RecorderStatus};
use crate::session::{self, ClipInfo, GameSession, StatSample, CLIPS_DIR, THUMB_FILE};
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

#[derive(Debug, Clone)]
pub struct InputEvent {
    pub key: KeyPress,
    pub at: Instant,
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
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EngineEvent {
    Status(LiveStatus),
    GameEvent { session_id: String, event: GameEvent },
    GameStarted { game_name: String },
    GameEnded { session_id: String },
    /// Session file changed (saved, clips added, retention ran).
    LibraryChanged,
    Notice { level: String, text: String },
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
    thumb_done: bool,
    missing_checks: u32,
    last_perf: Instant,
    last_event: Option<GameEvent>,
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
                    } else if n % 2 == 0 {
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
        self.games.iter().position(|g| {
            !self.settings.disabled_games.iter().any(|d| d == g.id())
                && g.process_names().iter().any(|p| procs.iter().any(|r| r.eq_ignore_ascii_case(p)))
        })
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

    fn record_options(&self, dir: &Path) -> RecordOptions {
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
        let (gid, gname, capture) = {
            let g = &self.games[idx];
            (g.id(), g.name(), g.capture())
        };
        let session = GameSession::new(id.clone(), gid, gname, started);
        let _ = session.save(&dir);
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
            thumb_done: false,
            missing_checks: 0,
            last_perf: Instant::now(),
            last_event: None,
        };
        self.message = None;
        if self.settings.auto_record {
            let opts = self.record_options(&dir);
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
        self.platform.set_input_enabled(true);
        self.active = Some(active);
        self.emit(EngineEvent::GameStarted { game_name: gname.into() });
        self.emit_status();
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
        let update = match self.games[idx].poll().await {
            Ok(u) => Some(u),
            Err(e) => {
                log::debug!("poll: {e:#}");
                None
            }
        };
        let recording = self.active.as_ref().unwrap().recording;
        // Ask the recorder for its own elapsed time right after the poll (used for the offset).
        // Only sample while the clock runs (League reports 0:00 during the loading screen).
        let rec_elapsed = if recording
            && update.as_ref().is_some_and(|u| u.game_time.is_some_and(|t| t > 0.5))
            && self.active.as_ref().unwrap().offset_samples.len() < OFFSET_SAMPLES
        {
            match self.recorder.record_elapsed().await {
                Ok(Some(d)) => Some(d.as_secs_f64()),
                _ => self.active.as_ref().unwrap().rec_started.map(|r| r.elapsed().as_secs_f64()),
            }
        } else {
            None
        };

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
                    a.session.video_offset = median(&a.offset_samples).max(0.0);
                }
                if gt >= a.next_stat_sample {
                    if let Some(st) = &u.stats {
                        a.session.timeline.push(StatSample { t: gt, kills: st.kills, deaths: st.deaths, assists: st.assists, cs: st.cs, gold: st.gold });
                    }
                    a.next_stat_sample = (gt / STAT_SAMPLE_EVERY).floor() * STAT_SAMPLE_EVERY + STAT_SAMPLE_EVERY;
                }
            }
            if let Some(p) = u.player {
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
                a.session.game_duration = a.clock.map(|(_, t)| t);
            }
        }
        for ev in new_events {
            self.push_event(ev);
        }

        // Thumbnail ~90 s into the match.
        let (want_thumb, dir) = {
            let a = self.active.as_ref().unwrap();
            let gt = self.game_time_at(Instant::now());
            (a.recording && !a.thumb_done && gt >= 90.0, a.dir.clone())
        };
        if want_thumb {
            if let Err(e) = self.recorder.screenshot(&dir.join(THUMB_FILE), 480).await {
                log::warn!("thumbnail: {e:#}");
            }
            self.active.as_mut().unwrap().thumb_done = true;
        }

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
        if n % 2 == 0 {
            let running = self.game_running(idx);
            let a = self.active.as_mut().unwrap();
            a.missing_checks = if running { 0 } else { a.missing_checks + 1 };
            gone = a.missing_checks >= 2;
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

    fn push_event(&mut self, ev: GameEvent) {
        let s = &self.settings.events;
        if s.tts_enabled && s.tts_kinds.contains(&ev.kind) {
            let text = if ev.kind == EventKind::Multikill || ev.steal { ev.title.clone() } else { ev.kind.callout().to_string() };
            self.platform.speak(&text, s.tts_volume);
        }
        let a = self.active.as_mut().unwrap();
        a.seen.insert(ev.id.clone());
        a.session.events.push(ev.clone());
        a.last_event = Some(ev.clone());
        let sid = a.session.id.clone();
        self.emit(EngineEvent::GameEvent { session_id: sid, event: ev });
    }

    async fn handle_input(&mut self, ev: InputEvent) {
        let Some(a) = &self.active else { return };
        if self.hk_clip.as_ref().is_some_and(|h| h.matches(&ev.key)) {
            self.save_clip().await;
            return;
        }
        if self.hk_marker.as_ref().is_some_and(|h| h.matches(&ev.key)) {
            self.add_marker(ev.at);
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
        let gt = self.game_time_at(ev.at);
        if let Some(e) = self.games[idx].on_key(&ev.key, gt) {
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
        let video_end = match self.recorder.record_elapsed().await {
            Ok(Some(d)) => Some(d.as_secs_f64()),
            _ => a.rec_started.map(|r| r.elapsed().as_secs_f64()),
        };
        match self.recorder.save_replay().await {
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
                    video_start: video_end.map(|e| (e - len).max(0.0)),
                    video_end,
                    created_at: Local::now(),
                    source: "replay".into(),
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

    async fn end_session(&mut self) {
        let Some(mut a) = self.active.take() else { return };
        self.platform.set_input_enabled(false);
        let idx = a.game;
        let short = self.games[idx].short_name();
        if a.recording && !a.thumb_done {
            // Short game: grab the thumbnail now.
            let _ = self.recorder.screenshot(&a.dir.join(THUMB_FILE), 480).await;
        }
        if a.recording {
            let elapsed = a.rec_started.map(|r| r.elapsed().as_secs_f64());
            match self.recorder.stop_recording().await {
                Ok(path) => {
                    a.session.video_duration = elapsed;
                    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("mp4").to_string();
                    let dest = session::unique_path(&a.dir, &a.session.video_name(short, &ext));
                    let final_path = if move_file(&path, &dest).await { dest } else { path };
                    a.session.video_file = Some(
                        if final_path.parent() == Some(a.dir.as_path()) {
                            final_path.file_name().unwrap().to_string_lossy().to_string()
                        } else {
                            final_path.to_string_lossy().to_string()
                        },
                    );
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

        let too_short = !a.reached_in_progress
            && a.session.events.is_empty()
            && a.session.clips.is_empty()
            && a.session.video_duration.unwrap_or(0.0) < 30.0;
        let sid = a.session.id.clone();
        if too_short {
            log::info!("discarding short session {sid}");
            let _ = std::fs::remove_dir_all(&a.dir);
        } else {
            if let Err(e) = a.session.save(&a.dir) {
                self.notice("error", format!("Couldn't save the game: {e:#}"));
            }
            self.emit(EngineEvent::GameEnded { session_id: sid.clone() });
            self.spawn_post_process(a.dir.clone(), sid);
        }
        self.emit(EngineEvent::LibraryChanged);
        self.emit_status();
    }

    /// After a game: cut automatic event clips (stream copy, low priority) and apply retention.
    fn spawn_post_process(&self, dir: PathBuf, sid: String) {
        let cutter = self.cutter.clone();
        let ev = self.settings.events.clone();
        let root = self.save_dir();
        let (days, gb) = (self.settings.auto_delete_days, self.settings.max_disk_gb);
        let tx = self.tx.clone();
        tokio::spawn(async move {
            if let (Some(cutter), false) = (cutter, ev.clip_kinds.is_empty()) {
                if let Err(e) = auto_clips(&*cutter, &dir, &ev).await {
                    log::warn!("auto clips: {e:#}");
                    let _ = tx.send(EngineEvent::Notice { level: "warn".into(), text: format!("Event clips weren't created: {e:#}") });
                }
            }
            let removed = crate::library::apply_retention(&root, days, gb, Local::now(), Some(&sid));
            if !removed.is_empty() {
                log::info!("retention removed {removed:?}");
            }
            let _ = tx.send(EngineEvent::LibraryChanged);
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
    if cfg!(windows) { e.raw_os_error() == Some(17) } else { e.raw_os_error() == Some(18) }
}

#[cfg(test)]
mod tests;
