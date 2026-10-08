//! The Discord copy end to end with the real ffmpeg (x264 standing in for the GPU encoders):
//! hard-to-compress clips (moving pattern + noise, like a teamfight) with the game and a
//! microphone track, at the recorder's bitrate, must come out under 19.5 MB with the planned
//! size and frame rate and a single audio track. Skipped when ffmpeg isn't installed.

use cv_capture::share::{self, DISCORD_TARGET_BYTES};
use std::path::{Path, PathBuf};
use std::process::Command;

fn have_ffmpeg() -> bool {
    Command::new("ffmpeg").arg("-version").output().is_ok() && Command::new("ffprobe").arg("-version").output().is_ok()
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("cv-share-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d.join(name)
}

/// A clip like a recording: H.264 at `kbps`, two audio tracks (game, microphone).
fn make_clip(out: &Path, secs: u32, w: u32, h: u32, fps: u32, kbps: u32) {
    let st = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i"])
        .arg(format!("testsrc2=s={w}x{h}:r={fps}:d={secs},noise=alls=25:allf=t"))
        .args(["-f", "lavfi", "-i"])
        .arg(format!("sine=f=440:d={secs}"))
        .args(["-f", "lavfi", "-i"])
        .arg(format!("sine=f=660:d={secs}"))
        .args(["-map", "0:v", "-map", "1:a", "-map", "2:a", "-c:v", "libx264", "-preset", "ultrafast", "-b:v"])
        .arg(format!("{kbps}k"))
        .args(["-c:a", "aac", "-b:a", "160k"])
        .arg(out)
        .status()
        .unwrap();
    assert!(st.success());
}

fn probe(path: &Path, what: &str) -> String {
    let o = Command::new("ffprobe")
        .args(["-v", "error", "-select_streams", what, "-show_entries", "stream=width,height,avg_frame_rate", "-of", "csv=p=0"])
        .arg(path)
        .output()
        .unwrap();
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn check(name: &str, secs: u32, w: u32, h: u32, fps: u32, kbps: u32) {
    let src = tmp(&format!("{name}.mp4"));
    make_clip(&src, secs, w, h, fps, kbps);
    let info = cv_capture::remux::info(&src).unwrap();
    assert_eq!(info.audio_tracks, 2);
    assert!(info.bytes > DISCORD_TARGET_BYTES, "{name}: the test clip should be too big to send as is ({} bytes)", info.bytes);
    assert!(!share::fits_as_is(info.bytes, info.codec.as_deref(), info.audio_tracks, DISCORD_TARGET_BYTES));
    let plan = share::plan(info.duration_secs, info.width, info.height, fps, DISCORD_TARGET_BYTES).unwrap();
    let out = tmp(&format!("{name}-discord.mp4"));
    let t = std::time::Instant::now();
    let (used, size) = share::make(Path::new("ffmpeg"), &src, &out, plan, info.audio_tracks, "libx264", DISCORD_TARGET_BYTES).unwrap();
    eprintln!("{name}: {} -> {} bytes ({:?}) in {:?}", info.bytes, size, used, t.elapsed());
    assert!(size <= DISCORD_TARGET_BYTES && std::fs::metadata(&out).unwrap().len() == size);
    assert_eq!(probe(&out, "v"), format!("{},{},{}/1", used.width, used.height, used.fps));
    assert_eq!(probe(&out, "a").lines().count(), 1, "{name}: one audio track");
    let o = cv_capture::remux::info(&out).unwrap();
    assert!((o.duration_secs - info.duration_secs).abs() < 0.2, "{name}: {} vs {}", o.duration_secs, info.duration_secs);
    assert!(!out.with_extension("part.mp4").exists());
}

#[test]
fn hotkey_clip_fits_discord() {
    if !have_ffmpeg() {
        eprintln!("skipped: no ffmpeg");
        return;
    }
    // 30 s at 1080p60 and 12 Mbps (the recorder's "standard"): ~46 MB.
    check("hotkey30", 30, 1920, 1080, 60, 12_000);
}

#[test]
fn long_clip_fits_discord() {
    if !have_ffmpeg() {
        eprintln!("skipped: no ffmpeg");
        return;
    }
    // 100 s at 720p30 (a long editor clip): stepped down to 540p.
    check("long100", 100, 1280, 720, 30, 4_000);
}
