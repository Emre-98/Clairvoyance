//! End-to-end: engine + League module + fake League API + real OBS.
//! `GR_ALLOW_CPU_ENCODER=1 cargo run -p gr-obs --example e2e -- <password> <save_dir>`
use gr_core::engine::{Engine, EngineCommand, EngineEvent, Platform};
use gr_obs::{ObsConfig, ObsRecorder};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

struct P(Arc<AtomicBool>);
impl Platform for P {
    fn running_processes(&self) -> Vec<String> {
        if self.0.load(Ordering::SeqCst) { vec!["league of legends.exe".into()] } else { vec![] }
    }
    fn foreground_process(&self) -> Option<String> { Some("league of legends.exe".into()) }
    fn speak(&self, t: &str, _v: u8) { println!("  (tts) {t}"); }
    fn perf_sample(&self) -> Option<(f64, f64)> { Some((0.1, 30.0)) }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut a = std::env::args().skip(1);
    let password = a.next().unwrap_or_default();
    let save = std::path::PathBuf::from(a.next().unwrap_or("/tmp/gr-e2e".into()));
    let mock = gr_mock_league::spawn(gr_mock_league::MockOptions { port: 29990, speed: 1.0, length: 40.0, loading_secs: 6.0, linger_secs: 3.0, ..Default::default() })?;
    let mut s = gr_core::Settings::default();
    s.save_dir = save.to_string_lossy().into();
    s.video.encoder = "x264".into();
    s.video.display_capture = true;
    s.video.height = 720;
    s.video.fps = 30;
    s.events.tts_enabled = true;
    s.events.clip_kinds.clear();
    s.games.insert("league".into(), serde_json::json!({ "api_base": mock.base_url }));
    let rec = Arc::new(ObsRecorder::new(ObsConfig { host: "127.0.0.1".into(), port: 4455, password }, None));
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let (ctx, crx) = tokio::sync::mpsc::unbounded_channel();
    let (_itx, irx) = tokio::sync::mpsc::unbounded_channel();
    let engine = Engine::new(vec![Box::new(gr_game_league::LeagueIntegration::new())], rec, Arc::new(P(mock.running.clone())), None, s, save.clone(), tx);
    let h = tokio::spawn(engine.run(crx, irx));
    let mut clip_sent = false;
    while let Some(ev) = rx.recv().await {
        match &ev {
            EngineEvent::Status(st) => {
                if !clip_sent && st.game_time.unwrap_or(0.0) > 15.0 {
                    clip_sent = true;
                    ctx.send(EngineCommand::SaveClip).ok();
                }
            }
            EngineEvent::GameEvent { event, .. } => println!("event {:>7.1}s {:?} {}", event.game_time, event.kind, event.title),
            EngineEvent::GameEnded { session_id } => {
                println!("ended {session_id}");
                let s = gr_core::GameSession::load(&save.join(session_id))?;
                println!("offset {:.2}s video {:?} dur {:?} result {:?} clips {} events {}", s.video_offset, s.video_file, s.video_duration, s.result, s.clips.len(), s.events.len());
                println!("stats {:?}", s.stats);
                break;
            }
            EngineEvent::Notice { level, text } => println!("notice {level}: {text}"),
            _ => {}
        }
    }
    ctx.send(EngineCommand::Shutdown).ok();
    h.await?;
    Ok(())
}
