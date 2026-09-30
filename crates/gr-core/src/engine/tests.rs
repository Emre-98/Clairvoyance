use super::*;
use crate::game::{CaptureTarget, GameResult, PollUpdate};
use std::sync::Mutex;

struct FakeGame {
    polls: u32,
    end_after: u32,
}

#[async_trait]
impl GameIntegration for FakeGame {
    fn id(&self) -> &'static str { "fake" }
    fn name(&self) -> &'static str { "Fake Game" }
    fn short_name(&self) -> &'static str { "Fake" }
    fn process_names(&self) -> &'static [&'static str] { &["fake.exe"] }
    fn capture(&self) -> CaptureTarget { CaptureTarget { exe: "fake.exe".into(), display_capture_only: false } }
    fn supports_events(&self) -> bool { true }
    fn end_grace(&self) -> Duration { Duration::from_millis(0) }
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
    started: Mutex<bool>,
    game_clock: Arc<Mutex<f64>>,
}

#[async_trait]
impl Recorder for FakeRecorder {
    async fn ensure_connected(&self) -> anyhow::Result<()> { Ok(()) }
    async fn prepare(&self, _t: &CaptureTarget, o: &RecordOptions) -> anyhow::Result<()> {
        *self.dir.lock().unwrap() = Some(o.output_dir.clone());
        Ok(())
    }
    async fn start_recording(&self) -> anyhow::Result<()> { *self.started.lock().unwrap() = true; Ok(()) }
    async fn stop_recording(&self) -> anyhow::Result<PathBuf> {
        let p = self.dir.lock().unwrap().clone().unwrap().join("raw.mp4");
        std::fs::write(&p, b"video")?;
        Ok(p)
    }
    async fn record_elapsed(&self) -> anyhow::Result<Option<Duration>> {
        // The video is always 25 s ahead of the game clock (loading screen).
        let polls = *self.game_clock.lock().unwrap();
        Ok(Some(Duration::from_secs_f64(polls + 25.0)))
    }
    async fn save_replay(&self) -> anyhow::Result<PathBuf> { anyhow::bail!("no") }
    async fn screenshot(&self, _p: &Path, _w: u32) -> anyhow::Result<()> { Ok(()) }
    async fn finish(&self) -> anyhow::Result<()> { Ok(()) }
    async fn status(&self) -> RecorderStatus { RecorderStatus { connected: true, ..Default::default() } }
}

struct FakePlatform {
    procs: Mutex<Vec<String>>,
}
impl Platform for FakePlatform {
    fn running_processes(&self) -> Vec<String> { self.procs.lock().unwrap().clone() }
    fn foreground_process(&self) -> Option<String> { Some("fake.exe".into()) }
    fn speak(&self, _t: &str, _v: u8) {}
    fn perf_sample(&self) -> Option<(f64, f64)> { Some((0.2, 40.0)) }
}

/// Game clock seen by the fake recorder follows the fake game's clock (poll n -> n-3 s).
struct ClockGame {
    inner: FakeGame,
    clock: Arc<Mutex<f64>>,
}
#[async_trait]
impl GameIntegration for ClockGame {
    fn id(&self) -> &'static str { self.inner.id() }
    fn name(&self) -> &'static str { self.inner.name() }
    fn short_name(&self) -> &'static str { self.inner.short_name() }
    fn process_names(&self) -> &'static [&'static str] { self.inner.process_names() }
    fn capture(&self) -> CaptureTarget { self.inner.capture() }
    fn supports_events(&self) -> bool { true }
    fn end_grace(&self) -> Duration { Duration::from_millis(0) }
    async fn poll(&mut self) -> anyhow::Result<PollUpdate> {
        let u = self.inner.poll().await?;
        if let Some(gt) = u.game_time {
            *self.clock.lock().unwrap() = gt;
        }
        Ok(u)
    }
    fn on_key(&mut self, key: &KeyPress, gt: f64) -> Option<GameEvent> { self.inner.on_key(key, gt) }
}

#[tokio::test(start_paused = true)]
async fn full_session_lifecycle() {
    let root = std::env::temp_dir().join(format!("gr-engine-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let clock = Arc::new(Mutex::new(0.0));
    let game = ClockGame { inner: FakeGame { polls: 0, end_after: 12 }, clock: clock.clone() };
    let recorder = Arc::new(FakeRecorder { game_clock: clock.clone(), ..Default::default() });
    let platform = Arc::new(FakePlatform { procs: Mutex::new(vec![]) });
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
    itx.send(InputEvent { key: KeyPress { key: "R".into(), ctrl: false, shift: false, alt: false }, at: Instant::now() }).unwrap();

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
    fn id(&self) -> &'static str { "mg" }
    fn name(&self) -> &'static str { "Match Game" }
    fn short_name(&self) -> &'static str { "MG" }
    fn process_names(&self) -> &'static [&'static str] { &["mg.exe"] }
    fn capture(&self) -> CaptureTarget { CaptureTarget { exe: "mg.exe".into(), display_capture_only: true } }
    fn supports_events(&self) -> bool { true }
    fn match_only(&self) -> bool { true }
    fn end_grace(&self) -> Duration { Duration::from_secs(1) }
    async fn poll(&mut self) -> anyhow::Result<PollUpdate> {
        let phase = *self.phase.lock().unwrap();
        Ok(PollUpdate { phase, game_time: Some(1.0), events: vec![GameEvent::new("x", EventKind::Kill, 1.0, "Kill")], ..Default::default() })
    }
}

#[tokio::test(start_paused = true)]
async fn match_only_games_record_per_match() {
    let root = std::env::temp_dir().join(format!("gr-engine-mo-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let phase = Arc::new(Mutex::new(MatchPhase::Waiting));
    let recorder = Arc::new(FakeRecorder::default());
    let platform = Arc::new(FakePlatform { procs: Mutex::new(vec!["mg.exe".into()]) });
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
