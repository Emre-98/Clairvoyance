//! Drives a real OBS through the full recording flow:
//! `cargo run -p gr-obs --example obs_smoke -- <password> <out_dir>`
use gr_core::game::CaptureTarget;
use gr_core::recorder::{RecordOptions, Recorder};
use gr_obs::{ObsConfig, ObsRecorder};
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut a = std::env::args().skip(1);
    let password = a.next().unwrap_or_default();
    let out = std::path::PathBuf::from(a.next().unwrap_or("/tmp/obs-out".into()));
    let enc = std::env::var("ENC").unwrap_or("x264".into());
    let rec = ObsRecorder::new(ObsConfig { host: "127.0.0.1".into(), port: 4455, password }, None);
    rec.ensure_connected().await?;
    println!("status: {:?}", rec.status().await);
    let target = CaptureTarget { exe: "League of Legends.exe".into(), window: "League of Legends (TM) Client:RiotWindowClass:League of Legends.exe".into(), display_capture_only: false };
    let opts = RecordOptions { output_dir: out.clone(), encoder: enc, quality: "standard".into(), fps: 30, height: 720, replay_buffer_secs: 10, record_mic: false, display_capture: true };
    rec.prepare(&target, &opts).await?;
    println!("prepared: {:?}", rec.status().await);
    rec.start_recording().await?;
    println!("recording: {:?}", rec.status().await);
    for _ in 0..3 {
        tokio::time::sleep(Duration::from_secs(2)).await;
        println!("elapsed {:?}", rec.record_elapsed().await?);
    }
    let shot = out.join("thumb.jpg");
    rec.screenshot(&shot, 480).await?;
    println!("screenshot exists: {}", shot.exists());
    let clip = rec.save_replay().await;
    println!("replay: {clip:?}");
    tokio::time::sleep(Duration::from_secs(2)).await;
    let path = rec.stop_recording().await?;
    println!("stopped -> {} ({} bytes)", path.display(), std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0));
    rec.finish().await?;
    println!("final: {:?}", rec.status().await);
    Ok(())
}
