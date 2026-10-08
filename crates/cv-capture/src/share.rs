//! Sharing a clip: a copy that fits Discord's free upload limit (20 MB since 2026-08-13; the
//! target is 19.5 million bytes, under it whichever way a MB is counted). The bitrate follows
//! from the clip's length; the resolution and frame rate follow from the bitrate, so a short
//! clip stays 1080p60 and a long one steps down instead of turning to mush. Always H.264 with
//! one audio track (the game and the microphone mixed): what Discord's embed plays everywhere.
//! Portable: needs only an ffmpeg executable (tested with the system ffmpeg).

use std::path::{Path, PathBuf};
use std::process::Stdio;

/// What a Discord copy must stay under (bytes). Well clear of 20 MB: Windows and Discord count
/// a MB differently, and a copy showing "19.x MB" next to a 20 MB limit looked like it didn't fit.
pub const DISCORD_TARGET_BYTES: u64 = 18_000_000;
/// Share of the target given to the streams; the rest covers the container and the encoder
/// overshooting its average (hardware encoders overshoot by up to ~10 % in busy scenes).
const USABLE: f64 = 0.90;
/// Below this video bitrate even 540p30 looks bad: the clip is too long to share this way.
pub const MIN_VIDEO_KBPS: u32 = 600;
/// A short clip gets no more than this (more would only make the file bigger, not better).
const MAX_VIDEO_KBPS: u32 = 16_000;

/// (height, fps, the least video bitrate in kbps it gets): the first rung the bitrate reaches.
const LADDER: [(u32, u32, u32); 5] = [(1080, 60, 8_000), (1080, 30, 4_000), (720, 60, 3_500), (720, 30, 1_800), (540, 30, 0)];

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct SharePlan {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub video_kbps: u32,
    pub audio_kbps: u32,
}

/// A clip that can be shared as it is: small enough, H.264, at most one audio track.
pub fn fits_as_is(bytes: u64, codec: Option<&str>, audio_tracks: usize, limit: u64) -> bool {
    bytes <= limit && codec.is_some_and(|c| c.starts_with("avc")) && audio_tracks <= 1
}

/// The longest clip (seconds) that still fits `limit`.
pub fn max_secs(limit: u64) -> f64 {
    limit as f64 * 8.0 * USABLE / 1000.0 / (MIN_VIDEO_KBPS + 96) as f64
}

fn even(v: f64) -> u32 {
    ((v / 2.0).round() as u32 * 2).max(2)
}

/// How to encode a clip of `duration` seconds (source `src_w` × `src_h` at `src_fps`) so it
/// fits `limit` bytes. `None` when it's too long (see [`max_secs`]).
pub fn plan(duration: f64, src_w: u32, src_h: u32, src_fps: u32, limit: u64) -> Option<SharePlan> {
    let total_kbps = limit as f64 * 8.0 * USABLE / 1000.0 / duration.max(0.5);
    let audio_kbps = if total_kbps >= 2_000.0 { 128 } else { 96 };
    let video = total_kbps - audio_kbps as f64;
    if video < MIN_VIDEO_KBPS as f64 {
        return None;
    }
    let video_kbps = (video as u32).min(MAX_VIDEO_KBPS);
    let &(rung_h, rung_fps, _) = LADDER.iter().find(|r| video_kbps >= r.2).unwrap_or(&LADDER[LADDER.len() - 1]);
    let (src_w, src_h) = (src_w.max(2), src_h.max(2));
    let height = even(rung_h.min(src_h) as f64);
    let width = even(src_w as f64 * height as f64 / src_h as f64);
    Some(SharePlan { width, height, fps: rung_fps.min(src_fps.max(1)), video_kbps, audio_kbps })
}

/// Encoder arguments for `encoder` (e.g. "h264_nvenc", "h264_mf", "libx264") at `kbps`: an
/// average with a capped peak, which every H.264 encoder ffmpeg has understands.
pub fn encoder_args(encoder: &str, kbps: u32) -> Vec<String> {
    let mut a: Vec<String> = vec!["-c:v".into(), encoder.into(), "-b:v".into(), format!("{kbps}k"), "-maxrate".into(), format!("{}k", kbps * 5 / 4), "-bufsize".into(), format!("{}k", kbps * 2)];
    if encoder == "libx264" {
        a.extend(["-preset".into(), "veryfast".into()]);
    }
    a
}

/// ffmpeg's arguments for the Discord copy of `input` (with `audio_tracks` audio tracks).
pub fn ffmpeg_args(input: &Path, out: &Path, p: &SharePlan, audio_tracks: usize, encoder: &str) -> Vec<String> {
    let mut a: Vec<String> = ["-hide_banner", "-y", "-nostats", "-loglevel", "error", "-i"].iter().map(|s| s.to_string()).collect();
    a.push(input.to_string_lossy().to_string());
    let mut filter = format!("[0:v:0]fps={},scale={}:{}:flags=bicubic,format=yuv420p[v]", p.fps, p.width, p.height);
    if audio_tracks >= 2 {
        // Discord plays only the first audio track: the microphone is mixed into the game's.
        filter.push_str(";[0:a:0][0:a:1]amix=inputs=2:duration=first:normalize=0[a]");
    }
    a.extend(["-filter_complex".into(), filter, "-map".into(), "[v]".into()]);
    match audio_tracks {
        0 => {}
        1 => a.extend(["-map".into(), "0:a:0".into()]),
        _ => a.extend(["-map".into(), "[a]".into()]),
    }
    a.extend(encoder_args(encoder, p.video_kbps));
    a.extend(["-g".into(), (p.fps * 2).to_string()]);
    if audio_tracks > 0 {
        a.extend(["-c:a".into(), "aac".into(), "-b:a".into(), format!("{}k", p.audio_kbps), "-ac".into(), "2".into()]);
    }
    a.extend(["-movflags".into(), "+faststart".into(), out.to_string_lossy().to_string()]);
    a
}

fn run(exe: &Path, args: &[String]) -> anyhow::Result<()> {
    let out = crate::overlay_export::command(exe).args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).output()?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let tail: String = err.lines().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join(" | ");
        anyhow::bail!("ffmpeg failed: {tail}");
    }
    Ok(())
}

/// The encoder for each attempt: the chosen one twice, then (when it's a hardware encoder that
/// keeps missing the bitrate) libx264, which holds a bitrate closely.
pub fn attempt_encoders(encoder: &str) -> Vec<&str> {
    if encoder == "libx264" {
        vec![encoder; 3]
    } else {
        vec![encoder, encoder, "libx264"]
    }
}

/// Encodes the Discord copy of `input` into `out` and checks it fits `limit`: if the encoder
/// overshot, again at a bitrate scaled down by the overshoot (the last try in libx264). Never
/// hands back a file over `limit`. Returns the plan used and the file's size.
pub fn make(exe: &Path, input: &Path, out: &Path, plan: SharePlan, audio_tracks: usize, encoder: &str, limit: u64) -> anyhow::Result<(SharePlan, u64)> {
    let part: PathBuf = out.with_extension("part.mp4");
    let mut p = plan;
    for (attempt, enc) in attempt_encoders(encoder).into_iter().enumerate() {
        if let Err(e) = run(exe, &ffmpeg_args(input, &part, &p, audio_tracks, enc)) {
            // libx264 missing from this ffmpeg build: nothing more to try.
            if attempt > 0 && enc != encoder {
                break;
            }
            return Err(e);
        }
        let size = std::fs::metadata(&part)?.len();
        if size <= limit {
            std::fs::rename(&part, out)?;
            return Ok((p, size));
        }
        log::info!("share: {} bytes at {} kbps is over {limit} (attempt {})", size, p.video_kbps, attempt + 1);
        let scaled = p.video_kbps as f64 * (limit as f64 / size as f64) * 0.88;
        p.video_kbps = (scaled as u32).max(MIN_VIDEO_KBPS / 2);
    }
    let _ = std::fs::remove_file(&part);
    anyhow::bail!("the encoder couldn't get this clip under {:.1} MB", limit as f64 / 1e6)
}

/// The Discord copy's file name, which the friend sees: "<champion> - <clip title>.mp4", with
/// what Windows doesn't allow in a file name left out.
pub fn file_name(champion: Option<&str>, title: &str) -> String {
    let clean = |s: &str| -> String {
        let c: String = s.chars().filter(|c| !c.is_control() && !r#"<>:"/\|?*"#.contains(*c)).collect();
        c.split_whitespace().collect::<Vec<_>>().join(" ").trim_matches(|c| c == '.' || c == ' ').to_string()
    };
    let title = clean(title);
    let mut name = match champion.map(clean).filter(|c| !c.is_empty()) {
        Some(c) if !title.is_empty() => format!("{c} - {title}"),
        Some(c) => c,
        None if !title.is_empty() => title,
        None => "Clip".into(),
    };
    if name.chars().count() > 80 {
        name = name.chars().take(80).collect::<String>().trim_end().to_string();
    }
    format!("{name}.mp4")
}

/// Removes the Discord copies (folders or files in `root`) older than `keep` (`Duration::ZERO`: all of them, at start-up).
pub fn sweep(root: &Path, keep: std::time::Duration) {
    let Ok(rd) = std::fs::read_dir(root) else { return };
    let now = std::time::SystemTime::now();
    for e in rd.flatten() {
        // A time in the future (the clock moved back) counts as just made.
        let age = e.metadata().and_then(|m| m.modified()).map(|t| now.duration_since(t).unwrap_or_default());
        if !age.is_ok_and(|a| a < keep) {
            let p = e.path();
            let _ = if p.is_dir() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const L: u64 = DISCORD_TARGET_BYTES;

    #[test]
    fn ladder_steps_down_with_length() {
        let p = |secs: f64| plan(secs, 1920, 1080, 60, L).unwrap();
        // A 14 s auto clip: room for 1080p60.
        assert_eq!((p(14.0).height, p(14.0).fps), (1080, 60));
        // A 30 s hotkey clip: 1080p at 30 fps (about 4.2 Mbps).
        let h = p(30.0);
        assert_eq!((h.width, h.height, h.fps), (1920, 1080, 30));
        assert!((4_000..4_500).contains(&h.video_kbps), "{h:?}");
        // A minute: 720p30; two minutes: 540p30.
        assert_eq!((p(60.0).height, p(60.0).fps), (720, 30));
        assert_eq!((p(120.0).width, p(120.0).height, p(120.0).fps), (960, 540, 30));
        // Too long.
        assert!(plan(max_secs(L) + 1.0, 1920, 1080, 60, L).is_none());
        assert!(max_secs(L) > 180.0 && max_secs(L) < 200.0, "{}", max_secs(L));
    }

    #[test]
    fn never_upscales_and_keeps_the_aspect() {
        let p = plan(5.0, 1280, 720, 30, L).unwrap();
        assert_eq!((p.width, p.height, p.fps), (1280, 720, 30));
        assert_eq!(p.video_kbps, MAX_VIDEO_KBPS);
        // Ultrawide 3440 × 1440 at 720p: 1720 × 720.
        let p = plan(60.0, 3440, 1440, 60, L).unwrap();
        assert_eq!((p.width, p.height), (1720, 720));
    }

    #[test]
    fn every_plan_fits_on_paper() {
        for secs in [1.0, 5.0, 14.0, 30.0, 45.0, 90.0, 150.0, 180.0] {
            let p = plan(secs, 1920, 1080, 60, L).unwrap();
            let bytes = (p.video_kbps + p.audio_kbps) as f64 * 1000.0 / 8.0 * secs;
            assert!(bytes <= L as f64 * USABLE + 1.0, "{secs} s: {bytes}");
        }
    }

    #[test]
    fn sweep_keeps_recent_copies_and_clears_all_at_start() {
        let root = std::env::temp_dir().join(format!("cv-share-sweep-{}", std::process::id()));
        std::fs::create_dir_all(root.join("g1_a.mp4")).unwrap();
        std::fs::write(root.join("g1_a.mp4").join("Ahri - a.mp4"), b"x").unwrap();
        sweep(&root, std::time::Duration::from_secs(24 * 3600));
        assert!(root.join("g1_a.mp4").exists(), "a fresh copy stays for the paste");
        sweep(&root, std::time::Duration::ZERO);
        assert!(!root.join("g1_a.mp4").exists(), "all gone at the next start");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn friendly_file_names() {
        assert_eq!(file_name(Some("Kai'Sa"), "Pentakill"), "Kai'Sa - Pentakill.mp4");
        assert_eq!(file_name(Some("Jhin"), "Clip 12:34 / ult?"), "Jhin - Clip 1234 ult.mp4");
        assert_eq!(file_name(None, "  ..  "), "Clip.mp4");
        assert_eq!(file_name(Some("Ahri"), ""), "Ahri.mp4");
        assert_eq!(file_name(None, &"a".repeat(200)).len(), 84);
    }

    #[test]
    fn as_is_only_when_small_h264_and_one_track() {
        assert!(fits_as_is(10_000_000, Some("avc1.640028"), 1, L));
        assert!(!fits_as_is(25_000_000, Some("avc1.640028"), 1, L));
        assert!(!fits_as_is(10_000_000, Some("hvc1"), 1, L));
        assert!(!fits_as_is(10_000_000, Some("avc1.640028"), 2, L), "the mic track would be lost");
        assert!(!fits_as_is(10_000_000, None, 1, L));
    }

    #[test]
    fn args_mix_the_microphone_and_cap_the_rate() {
        let p = SharePlan { width: 1920, height: 1080, fps: 30, video_kbps: 4800, audio_kbps: 128 };
        let a = ffmpeg_args(Path::new("in.mp4"), Path::new("out.mp4"), &p, 2, "h264_nvenc").join(" ");
        assert!(a.contains("[0:v:0]fps=30,scale=1920:1080:flags=bicubic,format=yuv420p[v]"), "{a}");
        assert!(a.contains("amix=inputs=2") && a.contains("-map [a]"), "{a}");
        assert!(a.contains("-c:v h264_nvenc -b:v 4800k -maxrate 6000k -bufsize 9600k -g 60"), "{a}");
        assert!(a.contains("-c:a aac -b:a 128k -ac 2 -movflags +faststart out.mp4"), "{a}");
        let a = ffmpeg_args(Path::new("in.mp4"), Path::new("out.mp4"), &p, 1, "libx264").join(" ");
        assert!(a.contains("-map 0:a:0") && !a.contains("amix") && a.contains("-preset veryfast"), "{a}");
        let a = ffmpeg_args(Path::new("in.mp4"), Path::new("out.mp4"), &p, 0, "libx264").join(" ");
        assert!(!a.contains("-c:a") && !a.contains("0:a"), "{a}");
    }
}
