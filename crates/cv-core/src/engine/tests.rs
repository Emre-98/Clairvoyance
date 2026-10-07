use super::*;
use crate::game::{CaptureTarget, GameResult, PollUpdate};
use std::sync::Mutex;

struct FakeGame {
    polls: u32,
    end_after: u32,
}

#[async_trait]
impl GameIntegration for FakeGame {
    fn id(&self) -> &'static str {
        "fake"
    }
    fn name(&self) -> &'static str {
        "Fake Game"
    }
    fn short_name(&self) -> &'static str {
        "Fake"
    }
    fn process_names(&self) -> &'static [&'static str] {
        &["fake.exe"]
    }
    fn capture(&self) -> CaptureTarget {
        CaptureTarget { exe: "fake.exe".into(), display_capture_only: false }
    }
    fn supports_events(&self) -> bool {
        true
    }
    fn end_grace(&self) -> Duration {
        Duration::from_millis(0)
    }
    async fn poll(&mut self) -> anyhow::Result<PollUpdate> {
        self.polls += 1;
        let p = self.polls;
        if p <= 2 {
            anyhow::bail!("loading screen, API not up");
        }
        let gt = (p - 3) as f64 * 1.0;
        let mut u = PollUpdate { phase: MatchPhase::InProgress, game_time: Some(gt), ..Default::default() };
        // The API returns the same event every time; the engine must keep only one.
        if p >= 5 {
            u.events.push(GameEvent::new("7", EventKind::Kill, 2.0, "Killed Ahri"));
        }
        u.player = Some(PlayerInfo { name: "Me".into(), character: Some("Ahri".into()), ..Default::default() });
        if p >= self.end_after {
            u.phase = MatchPhase::Ended;
            u.result = Some(GameResult::Win);
        }
        Ok(u)
    }
    fn on_key(&mut self, key: &KeyPress, gt: f64) -> Option<GameEvent> {
        (key.key == "R").then(|| GameEvent::new(format!("ult-{gt}"), EventKind::UltPressed, gt, "Ult pressed"))
    }
}

#[derive(Default)]
struct FakeRecorder {
    dir: Mutex<Option<PathBuf>>,
    full_video: Mutex<Option<bool>>,
    started: Mutex<bool>,
    game_clock: Arc<Mutex<f64>>,
    stops: Mutex<u32>,
    /// Saving from the replay buffer works (otherwise it fails); the seconds asked for.
    replay_ok: bool,
    replays: Mutex<Vec<Option<u32>>>,
}

#[async_trait]
impl Recorder for FakeRecorder {
    async fn ensure_connected(&self) -> anyhow::Result<()> {
        Ok(())
    }
    async fn prepare(&self, _t: &CaptureTarget, o: &RecordOptions) -> anyhow::Result<()> {
        *self.dir.lock().unwrap() = Some(o.output_dir.clone());
        *self.full_video.lock().unwrap() = Some(o.full_video);
        Ok(())
    }
    async fn start_recording(&self) -> anyhow::Result<()> {
        *self.started.lock().unwrap() = true;
        Ok(())
    }
    async fn stop_recording(&self) -> anyhow::Result<PathBuf> {
        *self.stops.lock().unwrap() += 1;
        let p = self.dir.lock().unwrap().clone().unwrap().join("raw.mp4");
        std::fs::write(&p, b"video")?;
        Ok(p)
    }
    async fn record_elapsed(&self) -> anyhow::Result<Option<Duration>> {
        // The video is always 25 s ahead of the game clock (loading screen).
        let polls = *self.game_clock.lock().unwrap();
        Ok(Some(Duration::from_secs_f64(polls + 25.0)))
    }
    async fn save_replay(&self, secs: Option<u32>) -> anyhow::Result<PathBuf> {
        if !self.replay_ok {
            anyhow::bail!("no")
        }
        let mut replays = self.replays.lock().unwrap();
        replays.push(secs);
        let p = self.dir.lock().unwrap().clone().unwrap().join(format!("Replay_{}.mp4", replays.len()));
        std::fs::write(&p, b"clip")?;
        Ok(p)
    }
    async fn finish(&self) -> anyhow::Result<()> {
        Ok(())
    }
    async fn status(&self) -> RecorderStatus {
        RecorderStatus { connected: true, ..Default::default() }
    }
    fn clock_base_hns(&self) -> Option<i64> {
        Some(0)
    }
}

struct FakePlatform {
    procs: Mutex<Vec<String>>,
    capture: Mutex<Option<crate::input::CaptureRequest>>,
}
impl Platform for FakePlatform {
    fn running_processes(&self) -> Vec<String> {
        self.procs.lock().unwrap().clone()
    }
    fn foreground_process(&self) -> Option<String> {
        Some("fake.exe".into())
    }
    fn speak(&self, _t: &str, _v: u8) {}
    fn perf_sample(&self) -> Option<(f64, f64)> {
        Some((0.2, 40.0))
    }
    fn start_input_capture(&self, req: crate::input::CaptureRequest) {
        *self.capture.lock().unwrap() = Some(req);
    }
    fn stop_input_capture(&self) -> Option<crate::input::CaptureStats> {
        self.capture.lock().unwrap().take().map(|_| crate::input::CaptureStats::default())
    }
}

/// Game clock seen by the fake recorder follows the fake game's clock (poll n -> n-3 s).
struct ClockGame {
    inner: FakeGame,
    clock: Arc<Mutex<f64>>,
}
#[async_trait]
impl GameIntegration for ClockGame {
    fn id(&self) -> &'static str {
        self.inner.id()
    }
    fn name(&self) -> &'static str {
        self.inner.name()
    }
    fn short_name(&self) -> &'static str {
        self.inner.short_name()
    }
    fn process_names(&self) -> &'static [&'static str] {
        self.inner.process_names()
    }
    fn capture(&self) -> CaptureTarget {
        self.inner.capture()
    }
    fn supports_events(&self) -> bool {
        true
    }
    fn end_grace(&self) -> Duration {
        Duration::from_millis(0)
    }
    async fn poll(&mut self) -> anyhow::Result<PollUpdate> {
        let u = self.inner.poll().await?;
        if let Some(gt) = u.game_time {
            *self.clock.lock().unwrap() = gt;
        }
        Ok(u)
    }
    fn on_key(&mut self, key: &KeyPress, gt: f64) -> Option<GameEvent> {
        self.inner.on_key(key, gt)
    }
}

#[tokio::test(start_paused = true)]
async fn full_session_lifecycle() {
    let root = std::env::temp_dir().join(format!("cv-engine-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let clock = Arc::new(Mutex::new(0.0));
    let game = ClockGame { inner: FakeGame { polls: 0, end_after: 12 }, clock: clock.clone() };
    let recorder = Arc::new(FakeRecorder { game_clock: clock.clone(), ..Default::default() });
    let platform = Arc::new(FakePlatform { procs: Mutex::new(vec![]), capture: Mutex::new(None) });
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let (ctx, crx) = tokio::sync::mpsc::unbounded_channel();
    let (itx, irx) = tokio::sync::mpsc::unbounded_channel();
    let mut settings = Settings::default();
    settings.save_dir = root.to_string_lossy().to_string();
    settings.events.clip_kinds.clear();
    let engine = Engine::new(vec![Box::new(game)], recorder.clone(), platform.clone(), None, settings, root.clone(), tx);
    let handle = tokio::spawn(engine.run(crx, irx));

    tokio::time::sleep(Duration::from_secs(3)).await;
    assert!(!*recorder.started.lock().unwrap(), "nothing running yet");
    platform.procs.lock().unwrap().push("fake.exe".into());
    tokio::time::sleep(Duration::from_secs(6)).await;
    assert!(*recorder.started.lock().unwrap(), "recording started when the game appeared");
    // Ult key press while in game.
    itx.send(InputEvent::press(KeyPress { key: "R".into(), ctrl: false, shift: false, alt: false }, 0x52, Instant::now(), 0)).unwrap();

    let mut ended = None;
    let mut kills = 0;
    let mut starts = 0;
    for _ in 0..200 {
        tokio::time::sleep(Duration::from_millis(500)).await;
        while let Ok(ev) = rx.try_recv() {
            match ev {
                EngineEvent::GameEnded { session_id } => ended = Some(session_id),
                EngineEvent::GameEvent { event, .. } if event.kind == EventKind::Kill => kills += 1,
                EngineEvent::GameStarted { .. } => starts += 1,
                _ => {}
            }
        }
        if ended.is_some() {
            break;
        }
    }
    let sid = ended.expect("game ended");
    // The process stays open on the victory screen: no new session may start.
    tokio::time::sleep(Duration::from_secs(20)).await;
    while let Ok(ev) = rx.try_recv() {
        if let EngineEvent::GameStarted { .. } = ev {
            starts += 1;
        }
    }
    assert_eq!(starts, 1, "no second session while the game process is still open");
    // Closing the game and starting a new one is detected again.
    platform.procs.lock().unwrap().clear();
    tokio::time::sleep(Duration::from_secs(5)).await;

    assert_eq!(kills, 1, "duplicate API events are ignored");
    let s = GameSession::load(&root.join(&sid)).unwrap();
    assert_eq!(s.result, Some(GameResult::Win));
    assert!((s.video_offset - 25.0).abs() < 1e-6, "offset = {}", s.video_offset);
    assert_eq!(s.video_file.as_deref(), Some(format!("{}_Fake_Ahri_Win.mp4", s.started_at.format("%Y-%m-%d")).as_str()));
    assert!(root.join(&sid).join(s.video_file.unwrap()).exists());
    assert!(s.events.iter().any(|e| e.kind == EventKind::UltPressed));
    ctx.send(EngineCommand::Shutdown).unwrap();
    handle.await.unwrap();
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn clip_windows_merge() {
    let mut s = GameSession::new("x".into(), "g", "G", Local::now());
    s.video_offset = 20.0;
    s.events = vec![
        GameEvent::new("1", EventKind::Kill, 100.0, "Killed A"),
        GameEvent::new("2", EventKind::Kill, 105.0, "Killed B"),
        GameEvent::new("3", EventKind::Kill, 300.0, "Killed C"),
        GameEvent::new("4", EventKind::Death, 301.0, "Died"),
    ];
    let ev = crate::settings::EventSettings { clip_kinds: vec![EventKind::Kill], clip_before_secs: 10.0, clip_after_secs: 4.0, ..Default::default() };
    let w = clip_windows(&s, &ev);
    assert_eq!(w.len(), 2);
    assert_eq!((w[0].0, w[0].1), (110.0, 129.0));
    assert_eq!(w[0].2, "Killed A + Killed B");
    assert_eq!((w[1].0, w[1].1), (310.0, 324.0));
}

#[test]
fn median_works() {
    assert_eq!(median(&[3.0, 1.0, 2.0]), 2.0);
    assert_eq!(median(&[4.0, 1.0, 2.0, 3.0]), 2.5);
}

/// A game you keep open between matches (like CS2).
struct MatchGame {
    phase: Arc<Mutex<MatchPhase>>,
}

#[async_trait]
impl GameIntegration for MatchGame {
    fn id(&self) -> &'static str {
        "mg"
    }
    fn name(&self) -> &'static str {
        "Match Game"
    }
    fn short_name(&self) -> &'static str {
        "MG"
    }
    fn process_names(&self) -> &'static [&'static str] {
        &["mg.exe"]
    }
    fn capture(&self) -> CaptureTarget {
        CaptureTarget { exe: "mg.exe".into(), display_capture_only: true }
    }
    fn supports_events(&self) -> bool {
        true
    }
    fn match_only(&self) -> bool {
        true
    }
    fn end_grace(&self) -> Duration {
        Duration::from_secs(1)
    }
    async fn poll(&mut self) -> anyhow::Result<PollUpdate> {
        let phase = *self.phase.lock().unwrap();
        Ok(PollUpdate { phase, game_time: Some(1.0), events: vec![GameEvent::new("x", EventKind::Kill, 1.0, "Kill")], ..Default::default() })
    }
}

#[tokio::test(start_paused = true)]
async fn match_only_games_record_per_match() {
    let root = std::env::temp_dir().join(format!("cv-engine-mo-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let phase = Arc::new(Mutex::new(MatchPhase::Waiting));
    let recorder = Arc::new(FakeRecorder::default());
    let platform = Arc::new(FakePlatform { procs: Mutex::new(vec!["mg.exe".into()]), capture: Mutex::new(None) });
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let (ctx, crx) = tokio::sync::mpsc::unbounded_channel();
    let (_itx, irx) = tokio::sync::mpsc::unbounded_channel();
    let mut settings = Settings::default();
    settings.save_dir = root.to_string_lossy().to_string();
    let engine = Engine::new(vec![Box::new(MatchGame { phase: phase.clone() })], recorder.clone(), platform, None, settings, root.clone(), tx);
    let h = tokio::spawn(engine.run(crx, irx));
    let count = |rx: &mut tokio::sync::mpsc::UnboundedReceiver<EngineEvent>| {
        let (mut s, mut e) = (0, 0);
        while let Ok(ev) = rx.try_recv() {
            match ev {
                EngineEvent::GameStarted { .. } => s += 1,
                EngineEvent::GameEnded { .. } => e += 1,
                _ => {}
            }
        }
        (s, e)
    };
    tokio::time::sleep(Duration::from_secs(6)).await;
    assert_eq!(count(&mut rx), (0, 0), "in the menus: nothing recorded");
    *phase.lock().unwrap() = MatchPhase::InProgress;
    tokio::time::sleep(Duration::from_secs(6)).await;
    assert_eq!(count(&mut rx), (1, 0), "match started");
    *phase.lock().unwrap() = MatchPhase::Ended;
    tokio::time::sleep(Duration::from_secs(10)).await;
    assert_eq!(count(&mut rx), (0, 1), "match ended, no new session on the scoreboard");
    *phase.lock().unwrap() = MatchPhase::Waiting;
    tokio::time::sleep(Duration::from_secs(4)).await;
    *phase.lock().unwrap() = MatchPhase::InProgress;
    tokio::time::sleep(Duration::from_secs(6)).await;
    assert_eq!(count(&mut rx), (1, 0), "next match records again");
    *phase.lock().unwrap() = MatchPhase::Waiting;
    tokio::time::sleep(Duration::from_secs(6)).await;
    assert_eq!(count(&mut rx), (0, 1), "leaving the match ends it");
    ctx.send(EngineCommand::Shutdown).unwrap();
    h.await.unwrap();
    std::fs::remove_dir_all(root).ok();
}

/// A game with per-mode rules; `detect_mode` reports `mode`.
struct ModeGame {
    inner: FakeGame,
    mode: crate::modes::MatchMode,
}
#[async_trait]
impl GameIntegration for ModeGame {
    fn id(&self) -> &'static str {
        self.inner.id()
    }
    fn name(&self) -> &'static str {
        self.inner.name()
    }
    fn short_name(&self) -> &'static str {
        self.inner.short_name()
    }
    fn process_names(&self) -> &'static [&'static str] {
        self.inner.process_names()
    }
    fn capture(&self) -> CaptureTarget {
        self.inner.capture()
    }
    fn supports_events(&self) -> bool {
        true
    }
    fn end_grace(&self) -> Duration {
        Duration::from_millis(0)
    }
    async fn poll(&mut self) -> anyhow::Result<PollUpdate> {
        self.inner.poll().await
    }
    fn mode_groups(&self) -> Vec<crate::modes::ModeGroupInfo> {
        vec![crate::modes::ModeGroupInfo { id: "aram", label: "ARAM", help: "" }]
    }
    async fn detect_mode(&mut self) -> Option<crate::modes::MatchMode> {
        Some(self.mode.clone())
    }
}

fn mode(key: &str, name: &str) -> crate::modes::MatchMode {
    crate::modes::MatchMode { key: Some(key.into()), queue_id: Some(450), name: name.into(), game_mode: Some("ARAM".into()), group: "aram".into(), default_rule: None, source: "test".into() }
}

#[tokio::test(start_paused = true)]
async fn modes_switched_off_are_never_recorded() {
    let root = std::env::temp_dir().join(format!("cv-engine-off-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let game = ModeGame { inner: FakeGame { polls: 0, end_after: 1000 }, mode: mode("q450", "ARAM") };
    let recorder = Arc::new(FakeRecorder::default());
    let platform = Arc::new(FakePlatform { procs: Mutex::new(vec!["fake.exe".into()]), capture: Mutex::new(None) });
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let (_ctx, crx) = tokio::sync::mpsc::unbounded_channel();
    let (_itx, irx) = tokio::sync::mpsc::unbounded_channel();
    let mut settings = Settings::default();
    settings.save_dir = root.to_string_lossy().to_string();
    let mut gm = crate::modes::GameModes::default();
    gm.merge_catalog(
        &[crate::modes::CatalogMode {
            key: "q450".into(),
            queue_id: Some(450),
            name: "ARAM".into(),
            game_mode: Some("ARAM".into()),
            group: "aram".into(),
            default_rule: Some(ModeRule::Off),
            available: Some(true),
        }],
        false,
    );
    settings.modes.insert("fake".into(), gm);
    let engine = Engine::new(vec![Box::new(game)], recorder.clone(), platform.clone(), None, settings, root.clone(), tx);
    let handle = tokio::spawn(engine.run(crx, irx));
    tokio::time::sleep(Duration::from_secs(10)).await;
    assert!(!*recorder.started.lock().unwrap(), "an ARAM with ARAM switched off isn't recorded");
    let mut msg = None;
    while let Ok(ev) = rx.try_recv() {
        if let EngineEvent::Status(s) = ev {
            if s.message.is_some() {
                msg = s.message;
            }
        }
    }
    assert_eq!(msg.as_deref(), Some("ARAM: recording off for this mode"));
    // The game closes: nothing saved.
    platform.procs.lock().unwrap().clear();
    tokio::time::sleep(Duration::from_secs(6)).await;
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0, "no folder for an unrecorded game");
    handle.abort();
    std::fs::remove_dir_all(root).ok();
}

#[tokio::test(start_paused = true)]
async fn new_modes_are_added_and_follow_the_unknown_rule() {
    let root = std::env::temp_dir().join(format!("cv-engine-new-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let game = ModeGame { inner: FakeGame { polls: 0, end_after: 1000 }, mode: mode("q31337", "Brand New Mode") };
    let recorder = Arc::new(FakeRecorder::default());
    let platform = Arc::new(FakePlatform { procs: Mutex::new(vec!["fake.exe".into()]), capture: Mutex::new(None) });
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let (_ctx, crx) = tokio::sync::mpsc::unbounded_channel();
    let (_itx, irx) = tokio::sync::mpsc::unbounded_channel();
    let mut settings = Settings::default();
    settings.save_dir = root.to_string_lossy().to_string();
    settings.modes.insert("fake".into(), crate::modes::GameModes { unknown_rule: ModeRule::ClipsOnly, ..Default::default() });
    let engine = Engine::new(vec![Box::new(game)], recorder.clone(), platform.clone(), None, settings, root.clone(), tx);
    let handle = tokio::spawn(engine.run(crx, irx));
    tokio::time::sleep(Duration::from_secs(8)).await;
    assert!(*recorder.started.lock().unwrap());
    assert_eq!(*recorder.full_video.lock().unwrap(), Some(false), "clips only: no full video");
    let mut added = None;
    while let Ok(ev) = rx.try_recv() {
        if let EngineEvent::ModesChanged { modes, .. } = ev {
            added = Some(modes);
        }
    }
    let m = added.expect("the new mode is reported so it's saved in the settings");
    assert!(m.entries["q31337"].is_new);
    assert_eq!(m.entries["q31337"].rule, ModeRule::ClipsOnly);
    handle.abort();
    std::fs::remove_dir_all(root).ok();
}

#[tokio::test(start_paused = true)]
async fn clips_only_saves_event_clips_from_the_replay_buffer() {
    let root = std::env::temp_dir().join(format!("cv-engine-clips-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    // The kill comes at poll 5 (game time 2 s); the match ends at poll 9, before its clip is due.
    let game = ModeGame { inner: FakeGame { polls: 0, end_after: 9 }, mode: mode("q31337", "Brand New Mode") };
    let recorder = Arc::new(FakeRecorder { replay_ok: true, ..Default::default() });
    let platform = Arc::new(FakePlatform { procs: Mutex::new(vec!["fake.exe".into()]), capture: Mutex::new(None) });
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let (ctx, crx) = tokio::sync::mpsc::unbounded_channel();
    let (_itx, irx) = tokio::sync::mpsc::unbounded_channel();
    let mut settings = Settings::default();
    settings.save_dir = root.to_string_lossy().to_string();
    settings.events.clip_kinds = vec![EventKind::Kill];
    settings.events.clip_before_secs = 5.0;
    settings.events.clip_after_secs = 30.0;
    settings.modes.insert("fake".into(), crate::modes::GameModes { unknown_rule: ModeRule::ClipsOnly, ..Default::default() });
    let engine = Engine::new(vec![Box::new(game)], recorder.clone(), platform.clone(), None, settings, root.clone(), tx);
    let handle = tokio::spawn(engine.run(crx, irx));

    let (mut ended, mut rules) = (None, Vec::new());
    for _ in 0..60 {
        tokio::time::sleep(Duration::from_millis(500)).await;
        while let Ok(ev) = rx.try_recv() {
            match ev {
                EngineEvent::GameEnded { session_id } => ended = Some(session_id),
                EngineEvent::Status(s) if s.state == EngineState::Recording => rules.push(s.mode_rule),
                _ => {}
            }
        }
        if ended.is_some() {
            break;
        }
    }
    let sid = ended.expect("game ended");
    assert!(!rules.is_empty() && rules.iter().all(|r| *r == Some(ModeRule::ClipsOnly)), "status while recording: {rules:?}");
    // Not due yet when the match ended: saved at the end, with 5 s before + 30 s after.
    assert_eq!(*recorder.replays.lock().unwrap(), vec![Some(35)]);
    let s = GameSession::load(&root.join(&sid)).unwrap();
    assert_eq!(s.record_mode.as_deref(), Some("clips_only"));
    assert_eq!(s.video_file, None, "no full video");
    assert_eq!(s.clips.len(), 1);
    assert_eq!((s.clips[0].source.as_str(), s.clips[0].title.as_str()), ("event", "Killed Ahri"));
    assert!(root.join(&sid).join(CLIPS_DIR).join(&s.clips[0].file).exists());
    ctx.send(EngineCommand::Shutdown).unwrap();
    handle.await.unwrap();
    std::fs::remove_dir_all(root).ok();
}

/// A cursor-based game with a chat (Enter toggles it) and the ult on mouse button 5.
struct InputGame {
    inner: ClockGame,
    chat: bool,
}
#[async_trait]
impl GameIntegration for InputGame {
    fn id(&self) -> &'static str {
        self.inner.id()
    }
    fn name(&self) -> &'static str {
        self.inner.name()
    }
    fn short_name(&self) -> &'static str {
        self.inner.short_name()
    }
    fn process_names(&self) -> &'static [&'static str] {
        self.inner.process_names()
    }
    fn capture(&self) -> CaptureTarget {
        self.inner.capture()
    }
    fn supports_events(&self) -> bool {
        true
    }
    fn end_grace(&self) -> Duration {
        Duration::from_millis(0)
    }
    async fn poll(&mut self) -> anyhow::Result<PollUpdate> {
        self.inner.poll().await
    }
    fn on_key(&mut self, key: &KeyPress, gt: f64) -> Option<GameEvent> {
        if key.key == "Enter" {
            self.chat = !self.chat;
            return None;
        }
        self.inner.on_key(key, gt)
    }
    fn input_tracking(&self) -> bool {
        true
    }
    fn chat_open(&self) -> bool {
        self.chat
    }
    fn action_keys(&self) -> Vec<crate::input::actions::ActionKey> {
        vec![crate::input::actions::ActionKey {
            id: "spell1".into(),
            label: "Q".into(),
            icon: None,
            category: "ability".into(),
            size: 1.0,
            color: "#3b82f6".into(),
            default_key: "Q".into(),
            binds: vec![crate::input::actions::ActionBind::key(b'A')],
        }]
    }
    fn mouse_marks(&self, presses: &[(f64, u8)]) -> Vec<crate::game::KeyMark> {
        presses
            .iter()
            .filter(|p| p.1 == 5)
            .map(|p| crate::game::KeyMark { game_time: p.0, action: "ult".into(), key: "Mouse 5".into(), accepted: false, reason: Some("mouse".into()) })
            .collect()
    }
}

#[tokio::test(start_paused = true)]
async fn input_recording_keys_chat_and_mouse_marks() {
    use crate::input::{self, Record};
    let root = std::env::temp_dir().join(format!("cv-engine-input-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let clock = Arc::new(Mutex::new(0.0));
    let game = InputGame { inner: ClockGame { inner: FakeGame { polls: 0, end_after: 14 }, clock: clock.clone() }, chat: false };
    let recorder = Arc::new(FakeRecorder { game_clock: clock.clone(), ..Default::default() });
    let platform = Arc::new(FakePlatform { procs: Mutex::new(vec!["fake.exe".into()]), capture: Mutex::new(None) });
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let (ctx, crx) = tokio::sync::mpsc::unbounded_channel();
    let (itx, irx) = tokio::sync::mpsc::unbounded_channel();
    let mut settings = Settings::default();
    settings.save_dir = root.to_string_lossy().to_string();
    settings.events.clip_kinds.clear();
    let engine = Engine::new(vec![Box::new(game)], recorder.clone(), platform.clone(), None, settings, root.clone(), tx);
    let handle = tokio::spawn(engine.run(crx, irx));
    tokio::time::sleep(Duration::from_secs(7)).await;
    let req = platform.capture.lock().unwrap().clone().expect("input capture started with the recording");
    assert_eq!(req.process_names, vec!["fake.exe".to_string()]);
    let w = req.writer.clone();
    // What the cursor thread would record.
    w.push(Record::Focus { t: 30_000_000, focused: true });
    w.push(Record::Cursor { t: 30_000_000, x: 1000, y: 2000 });
    w.push(Record::Button { t: 31_000_000, button: 5, down: true });
    let key = |name: &str, vk: u16, down: bool, us: i64| InputEvent { key: Some(KeyPress { key: name.into(), ctrl: false, shift: false, alt: false }), vk, down, at: Instant::now(), qpc_hns: us * 10 };
    for ev in [
        key("Q", 0x51, true, 32_000_000),
        key("Q", 0x51, false, 32_100_000),
        key("Enter", 0x0D, true, 33_000_000), // chat opens
        key("Enter", 0x0D, false, 33_050_000),
        key("H", 0x48, true, 33_500_000), // typed: never recorded
        key("H", 0x48, false, 33_600_000),
        key("Enter", 0x0D, true, 34_000_000), // chat closes
        key("Enter", 0x0D, false, 34_050_000),
        key("W", 0x57, true, 35_000_000),
    ] {
        itx.send(ev).unwrap();
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let mut ended = None;
    for _ in 0..100 {
        tokio::time::sleep(Duration::from_millis(500)).await;
        while let Ok(ev) = rx.try_recv() {
            if let EngineEvent::GameEnded { session_id } = ev {
                ended = Some(session_id);
            }
        }
        if ended.is_some() {
            break;
        }
    }
    let sid = ended.expect("game ended");
    assert!(platform.capture.lock().unwrap().is_none(), "capture stopped");
    let s = GameSession::load(&root.join(&sid)).unwrap();
    let file = root.join(&sid).join(s.input_file.as_deref().expect("input file in the session"));
    assert_eq!(file.file_name().unwrap().to_string_lossy(), format!("{sid}.input"));
    let f = input::read(&file).unwrap();
    let keys: Vec<(u8, bool)> = f.records.iter().filter_map(|r| if let Record::Key { vk, down, .. } = r { Some((*vk, *down)) } else { None }).collect();
    assert_eq!(keys, vec![(0x51, true), (0x51, false), (0x57, true)], "no Enter, nothing typed in chat");
    let chats: Vec<bool> = f.records.iter().filter_map(|r| if let Record::Chat { open, .. } = r { Some(*open) } else { None }).collect();
    assert_eq!(chats, vec![true, false]);
    assert!(matches!(f.records.last(), Some(Record::End { .. })));
    // The game's action keys (binds of this game) were saved with the session when the input
    // recording started, and the press of the bound key becomes a bubble.
    let keys = s.action_keys.clone().expect("action keys saved with the session");
    assert_eq!(keys[0].binds, vec![crate::input::actions::ActionBind::key(b'A')]);
    let an = input::stats::Analysis::from_file(&f);
    assert!(input::actions::presses(&an, &keys, f.rate, &[]).is_empty(), "Q isn't bound to anything here");
    // The side-button press became an ult mark at its game time (video 31 s - offset 25 s).
    let m = s.key_presses.iter().find(|m| m.key == "Mouse 5").expect("mouse mark");
    assert!((m.game_time - 6.0).abs() < 1e-6, "{}", m.game_time);
    ctx.send(EngineCommand::Shutdown).unwrap();
    handle.await.unwrap();
    std::fs::remove_dir_all(root).ok();
}

// ---------- v1.7.1: replays and spectating are never recorded ----------

/// A game whose process start is checked (`session_check`) and whose API answers from the 3rd
/// poll: `watching` (spectator mode) or a normal match (`playing`).
struct WatchGame {
    check: SessionCheck,
    /// What the API says once it answers: Some = spectator mode.
    api_watching: Option<WatchKind>,
    spectate_setting: bool,
    polls: u32,
}

#[async_trait]
impl GameIntegration for WatchGame {
    fn id(&self) -> &'static str {
        "fake"
    }
    fn name(&self) -> &'static str {
        "Fake Game"
    }
    fn short_name(&self) -> &'static str {
        "Fake"
    }
    fn process_names(&self) -> &'static [&'static str] {
        &["fake.exe"]
    }
    fn capture(&self) -> CaptureTarget {
        CaptureTarget { exe: "fake.exe".into(), display_capture_only: false }
    }
    fn supports_events(&self) -> bool {
        true
    }
    fn end_grace(&self) -> Duration {
        Duration::from_millis(0)
    }
    async fn session_check(&mut self) -> SessionCheck {
        self.check
    }
    fn record_spectating(&self) -> bool {
        self.spectate_setting
    }
    async fn poll(&mut self) -> anyhow::Result<PollUpdate> {
        self.polls += 1;
        if self.polls <= 2 {
            anyhow::bail!("loading screen, API not up");
        }
        let gt = (self.polls - 3) as f64 * 10.0;
        let mut u = PollUpdate { phase: MatchPhase::InProgress, game_time: Some(gt), ..Default::default() };
        u.events.push(GameEvent::new("k1", EventKind::Kill, 5.0, "Killed Ahri"));
        u.watching = self.api_watching;
        u.playing = self.api_watching.is_none();
        Ok(u)
    }
}

struct WatchRun {
    root: PathBuf,
    recorder: Arc<FakeRecorder>,
    platform: Arc<FakePlatform>,
    rx: tokio::sync::mpsc::UnboundedReceiver<EngineEvent>,
    ctx: UnboundedSender<EngineCommand>,
    handle: tokio::task::JoinHandle<()>,
    last: Option<LiveStatus>,
    ended: Vec<String>,
}

impl WatchRun {
    fn start(name: &str, game: WatchGame) -> Self {
        let root = std::env::temp_dir().join(format!("cv-engine-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let recorder = Arc::new(FakeRecorder::default());
        let platform = Arc::new(FakePlatform { procs: Mutex::new(vec![]), capture: Mutex::new(None) });
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let (ctx, crx) = tokio::sync::mpsc::unbounded_channel();
        let (_itx, irx) = tokio::sync::mpsc::unbounded_channel();
        let mut settings = Settings::default();
        settings.save_dir = root.to_string_lossy().to_string();
        settings.events.clip_kinds.clear();
        let engine = Engine::new(vec![Box::new(game)], recorder.clone(), platform.clone(), None, settings, root.clone(), tx);
        let handle = tokio::spawn(engine.run(crx, irx));
        // Keep the receiver's other end alive for input.
        std::mem::forget(_itx);
        WatchRun { root, recorder, platform, rx, ctx, handle, last: None, ended: Vec::new() }
    }
    async fn wait(&mut self, secs: u64) {
        for _ in 0..secs * 2 {
            tokio::time::sleep(Duration::from_millis(500)).await;
            while let Ok(ev) = self.rx.try_recv() {
                match ev {
                    EngineEvent::Status(s) => self.last = Some(s),
                    EngineEvent::GameEnded { session_id } => self.ended.push(session_id),
                    _ => {}
                }
            }
        }
    }
    /// Game folders left in the save folder.
    fn folders(&self) -> Vec<String> {
        std::fs::read_dir(&self.root).map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect()).unwrap_or_default()
    }
    async fn finish(self) {
        self.ctx.send(EngineCommand::Shutdown).unwrap();
        self.handle.await.unwrap();
        std::fs::remove_dir_all(&self.root).ok();
    }
}

#[tokio::test(start_paused = true)]
async fn replay_seen_by_the_client_is_never_recorded() {
    let mut r = WatchRun::start("replay", WatchGame { check: SessionCheck::Watching(WatchKind::Replay), api_watching: Some(WatchKind::Replay), spectate_setting: false, polls: 0 });
    r.platform.procs.lock().unwrap().push("fake.exe".into());
    r.wait(12).await;
    assert!(!*r.recorder.started.lock().unwrap(), "recorder never started");
    let st = r.last.clone().unwrap();
    assert_eq!(st.watching, Some(WatchKind::Replay));
    assert_eq!(st.state, EngineState::Detected);
    assert_eq!(st.message.as_deref(), Some("Replay: not recorded"));
    assert!(r.folders().is_empty(), "nothing saved: {:?}", r.folders());
    r.platform.procs.lock().unwrap().clear();
    r.wait(6).await;
    assert_eq!(r.last.as_ref().unwrap().state, EngineState::Idle);
    assert!(r.ended.is_empty() && r.folders().is_empty());
    r.finish().await;
}

#[tokio::test(start_paused = true)]
async fn replay_found_late_deletes_the_partial_recording() {
    // The client couldn't tell (e.g. not reachable): recording starts, then the game's API says
    // spectator mode.
    let mut r = WatchRun::start("late", WatchGame { check: SessionCheck::Unknown, api_watching: Some(WatchKind::Replay), spectate_setting: false, polls: 0 });
    r.platform.procs.lock().unwrap().push("fake.exe".into());
    r.wait(2).await;
    assert!(*r.recorder.started.lock().unwrap(), "recording started (unknown yet)");
    r.wait(10).await;
    assert_eq!(*r.recorder.stops.lock().unwrap(), 1, "stopped when the API answered");
    assert!(r.folders().is_empty(), "partial recording deleted: {:?}", r.folders());
    let st = r.last.clone().unwrap();
    assert_eq!((st.state, st.watching), (EngineState::Detected, Some(WatchKind::Replay)));
    assert_eq!(st.message.as_deref(), Some("Replay: not recorded"));
    r.platform.procs.lock().unwrap().clear();
    r.wait(6).await;
    assert!(r.ended.is_empty(), "no game saved");
    assert!(r.folders().is_empty());
    r.finish().await;
}

#[tokio::test(start_paused = true)]
async fn unsure_waits_for_the_api_then_records_a_real_match() {
    let mut r = WatchRun::start("unsure-play", WatchGame { check: SessionCheck::Unsure(WatchKind::Unknown), api_watching: None, spectate_setting: false, polls: 0 });
    r.platform.procs.lock().unwrap().push("fake.exe".into());
    r.wait(2).await;
    assert!(!*r.recorder.started.lock().unwrap(), "not recorded while unsure");
    assert_eq!(r.last.as_ref().unwrap().message.as_deref(), Some("Checking whether this is a replay…"));
    r.wait(6).await;
    assert!(*r.recorder.started.lock().unwrap(), "the API confirmed a real match: recording");
    assert_eq!(r.last.as_ref().unwrap().state, EngineState::Recording);
    r.platform.procs.lock().unwrap().clear();
    r.wait(8).await;
    assert_eq!(r.ended.len(), 1, "saved as a game");
    r.finish().await;
}

#[tokio::test(start_paused = true)]
async fn unsure_then_spectator_mode_is_never_recorded() {
    let mut r = WatchRun::start("unsure-watch", WatchGame { check: SessionCheck::Unsure(WatchKind::Unknown), api_watching: Some(WatchKind::Unknown), spectate_setting: false, polls: 0 });
    r.platform.procs.lock().unwrap().push("fake.exe".into());
    r.wait(10).await;
    assert!(!*r.recorder.started.lock().unwrap());
    let st = r.last.clone().unwrap();
    assert_eq!(st.watching, Some(WatchKind::Unknown));
    assert_eq!(st.message.as_deref(), Some("Replay or spectating: not recorded"));
    r.platform.procs.lock().unwrap().clear();
    r.wait(6).await;
    assert!(r.ended.is_empty() && r.folders().is_empty());
    r.finish().await;
}

#[tokio::test(start_paused = true)]
async fn spectating_follows_its_setting() {
    // Off (default): not recorded.
    let mut r = WatchRun::start("spec-off", WatchGame { check: SessionCheck::Watching(WatchKind::Spectate), api_watching: Some(WatchKind::Spectate), spectate_setting: false, polls: 0 });
    r.platform.procs.lock().unwrap().push("fake.exe".into());
    r.wait(10).await;
    assert!(!*r.recorder.started.lock().unwrap());
    assert_eq!(r.last.as_ref().unwrap().message.as_deref(), Some("Spectating: not recorded"));
    r.finish().await;
    // On: recorded like a game, tagged "Spectating", even though the API says spectator mode.
    let mut r = WatchRun::start("spec-on", WatchGame { check: SessionCheck::Watching(WatchKind::Spectate), api_watching: Some(WatchKind::Spectate), spectate_setting: true, polls: 0 });
    r.platform.procs.lock().unwrap().push("fake.exe".into());
    r.wait(10).await;
    assert!(*r.recorder.started.lock().unwrap());
    assert_eq!(*r.recorder.stops.lock().unwrap(), 0, "not aborted by the API's spectator mode");
    r.platform.procs.lock().unwrap().clear();
    r.wait(8).await;
    assert_eq!(r.ended.len(), 1);
    let s = GameSession::load(&r.root.join(&r.ended[0])).unwrap();
    assert_eq!(s.mode_name.as_deref(), Some("Spectating"));
    r.finish().await;
}

#[tokio::test(start_paused = true)]
async fn leaving_the_game_ends_the_session() {
    // A match in progress, then the game closes.
    let mut r = WatchRun::start("leave", WatchGame { check: SessionCheck::Playing, api_watching: None, spectate_setting: false, polls: 0 });
    r.platform.procs.lock().unwrap().push("fake.exe".into());
    r.wait(8).await;
    assert_eq!(r.last.as_ref().unwrap().state, EngineState::Recording);
    r.platform.procs.lock().unwrap().clear();
    // The fake API keeps answering (it's not tied to the process): the 2 s process checks end it.
    r.wait(5).await;
    assert_eq!(r.ended.len(), 1);
    r.finish().await;
}
