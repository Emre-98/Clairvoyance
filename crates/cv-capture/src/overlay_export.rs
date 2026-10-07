//! Exporting a clip with the replay's input overlay (cursor trail, clicks, ability bubbles...)
//! burned in. The overlay is drawn by the UI with the player's own drawing code, one transparent
//! PNG per output frame, and streamed into ffmpeg, which lays it over the clip and re-encodes it.
//! Portable: needs only an ffmpeg executable (tested with the system ffmpeg).
//!
//! **Which moment each overlay frame shows** ([`plan`]): the output has a constant frame rate,
//! made from the recording's variable-rate frames by ffmpeg's `fps` filter. Measured with a
//! recording whose frames show their own index (tests/overlay_export.rs): with an absolute seek
//! (`-seek_timestamp 1 -ss <start>`), ffmpeg keeps the frames from `start` on, puts frame j in
//! output slot `floor((t_j - start) * fps + 0.5)`, and output frame k shows the last frame whose
//! slot is <= k (the first kept frame before any). The overlay for output frame k is drawn at that
//! frame's own time, so it matches what the player shows on the same frame.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};

/// Output frames of a clip from `start` to `end` at `fps` (ffmpeg is told exactly this many).
pub fn frame_count(start: f64, end: f64, fps: u32) -> usize {
    ((end - start) * fps as f64).round().max(1.0) as usize
}

/// The video time (seconds) the overlay must be drawn at for each output frame of a clip from
/// `start` to `end` at `fps`. `frame_times` = the recording's frame start times, sorted.
pub fn plan(frame_times: &[f64], start: f64, end: f64, fps: u32) -> Vec<f64> {
    let fps_f = fps as f64;
    let n = frame_count(start, end, fps);
    let first = frame_times.partition_point(|&t| t < start - 1e-9);
    let kept = &frame_times[first..];
    let Some(&first_t) = kept.first() else { return vec![start; n] };
    let slot = |t: f64| ((t - start) * fps_f + 0.5).floor() as i64;
    let mut out = Vec::with_capacity(n);
    let mut j = 0usize; // last kept frame with slot <= k (once there is one)
    let mut shown = first_t;
    for k in 0..n as i64 {
        while j < kept.len() && slot(kept[j]) <= k {
            shown = kept[j];
            j += 1;
        }
        out.push(shown);
    }
    out
}

/// The recording's frame rate for the export: the median frame interval, rounded (games don't
/// always deliver every frame, so the average would be too low).
pub fn nominal_fps(frame_times: &[f64]) -> u32 {
    let mut d: Vec<f64> = frame_times.windows(2).map(|w| w[1] - w[0]).filter(|d| *d > 1e-4).collect();
    if d.is_empty() {
        return 60;
    }
    d.sort_by(|a, b| a.total_cmp(b));
    let med = d[d.len() / 2];
    ((1.0 / med).round() as u32).clamp(10, 240)
}

#[derive(Debug, Clone)]
pub struct ExportParams {
    pub video: PathBuf,
    pub start: f64,
    pub end: f64,
    pub fps: u32,
    /// Encoder arguments, e.g. `["-c:v", "h264_nvenc", "-b:v", "16M"]` (see [`encoder_works`]).
    pub encoder: Vec<String>,
    pub out: PathBuf,
}

/// ffmpeg's arguments: the clip as input 0, the overlay PNGs from stdin as input 1.
pub fn ffmpeg_args(p: &ExportParams) -> Vec<String> {
    let fps = p.fps;
    let frames = frame_count(p.start, p.end, fps);
    // Exactly `frames` video frames; the sound runs to the end of the last one.
    let dur = (frames as f64 + 0.5) / fps as f64;
    let mut a: Vec<String> = ["-hide_banner", "-y", "-nostats", "-loglevel", "error", "-seek_timestamp", "1", "-ss"].iter().map(|s| s.to_string()).collect();
    a.push(format!("{:.6}", p.start));
    a.extend(["-i".into(), p.video.to_string_lossy().to_string()]);
    a.extend(["-f", "image2pipe", "-framerate"].iter().map(|s| s.to_string()));
    a.push(fps.to_string());
    a.extend(["-c:v", "png", "-i", "pipe:0", "-filter_complex"].iter().map(|s| s.to_string()));
    a.push(format!("[0:v]fps={fps}[b];[1:v]format=rgba[o];[b][o]overlay=0:0:eof_action=pass:format=auto,format=yuv420p[v]"));
    a.extend(["-map", "[v]", "-map", "0:a?"].iter().map(|s| s.to_string()));
    a.extend(["-frames:v".to_string(), frames.to_string()]);
    a.extend(p.encoder.iter().cloned());
    a.extend(["-c:a", "aac", "-b:a", "160k", "-t"].iter().map(|s| s.to_string()));
    a.push(format!("{dur:.6}"));
    a.extend(["-movflags".into(), "+faststart".into(), p.out.to_string_lossy().to_string()]);
    a
}

fn command(exe: &Path) -> Command {
    #[cfg_attr(not(windows), allow(unused_mut))] // flags are set on Windows only
    let mut c = Command::new(exe);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;
        c.creation_flags(CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS);
    }
    c
}

/// Whether this ffmpeg can encode with these arguments here (a GPU encoder needs that GPU):
/// encodes a tenth of a second of a blank picture.
pub fn encoder_works(exe: &Path, encoder: &[String]) -> bool {
    command(exe)
        .args(["-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i", "color=c=black:s=256x144:r=30", "-t", "0.1", "-pix_fmt", "yuv420p"])
        .args(encoder)
        .args(["-f", "null", "-"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// A running export: feed it one PNG per planned frame, then [`OverlayJob::finish`].
pub struct OverlayJob {
    child: Child,
    stdin: Option<ChildStdin>,
    stderr: Option<std::thread::JoinHandle<String>>,
    pub out: PathBuf,
    pub frames: usize,
    pub sent: usize,
}

impl OverlayJob {
    pub fn start(exe: &Path, p: &ExportParams, frames: usize) -> anyhow::Result<OverlayJob> {
        let mut child = command(exe).args(ffmpeg_args(p)).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped()).spawn()?;
        let stdin = child.stdin.take();
        // Drained on its own thread so ffmpeg never blocks on a full stderr pipe.
        let mut err = child.stderr.take().expect("stderr");
        let stderr = std::thread::spawn(move || {
            let mut s = String::new();
            let _ = err.read_to_string(&mut s);
            s
        });
        Ok(OverlayJob { child, stdin, stderr: Some(stderr), out: p.out.clone(), frames, sent: 0 })
    }

    /// The next overlay frame (a PNG of the video's size, transparent where nothing is drawn).
    /// Blocks while ffmpeg is busy: the caller's pace follows the encoder.
    pub fn push(&mut self, png: &[u8]) -> anyhow::Result<()> {
        if self.sent >= self.frames {
            return Ok(()); // ffmpeg already has every frame it uses
        }
        let w = self.stdin.as_mut().ok_or_else(|| anyhow::anyhow!("the export is closed"))?;
        if let Err(e) = w.write_all(png) {
            // ffmpeg stopped reading: it ended (an error, said by finish()).
            self.stdin = None;
            return Err(anyhow::anyhow!("ffmpeg stopped: {e}"));
        }
        self.sent += 1;
        Ok(())
    }

    /// Ends the overlay input and waits for ffmpeg to write the file.
    pub fn finish(mut self) -> anyhow::Result<PathBuf> {
        self.stdin = None; // EOF: frames still missing simply show no overlay
        let status = self.child.wait()?;
        let err = self.stderr.take().and_then(|h| h.join().ok()).unwrap_or_default();
        if !status.success() || !self.out.is_file() {
            let tail: String = err.lines().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join(" | ");
            let _ = std::fs::remove_file(&self.out);
            anyhow::bail!("ffmpeg failed: {tail}");
        }
        Ok(self.out.clone())
    }

    /// Stops ffmpeg and removes the unfinished file.
    pub fn cancel(mut self) {
        self.stdin = None;
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_follows_ffmpegs_frame_choice() {
        // 60 fps with frames 3 and 7 missing, first frame at 1/60 s.
        let ft: Vec<f64> = (1..20).filter(|i| i % 4 != 3).map(|i| i as f64 / 60.0).collect();
        // From 2/60: frame 2 is slot 0, 4 is slot 2 (3 is missing: slot 1 repeats frame 2)...
        let p = plan(&ft, 2.0 / 60.0, 8.0 / 60.0, 60);
        let idx: Vec<i64> = p.iter().map(|t| (t * 60.0).round() as i64).collect();
        assert_eq!(idx, vec![2, 2, 4, 5, 6, 6]);
        // A start between frames (2.4/60): frame 4 is slot 2 (floor(1.6 + 0.5)), so it also
        // fills slots 0 and 1; frame 5 is slot 3.
        let p = plan(&ft, 2.4 / 60.0, 6.4 / 60.0, 60);
        let idx: Vec<i64> = p.iter().map(|t| (t * 60.0).round() as i64).collect();
        assert_eq!(idx, vec![4, 4, 4, 5]);
        // Nothing after the start: the start itself.
        assert_eq!(plan(&ft, 5.0, 5.1, 60).len(), 6);
    }

    #[test]
    fn nominal_fps_ignores_dropped_frames() {
        let ft: Vec<f64> = (0..600).filter(|i| i % 5 != 0).map(|i| i as f64 / 60.0).collect();
        assert_eq!(nominal_fps(&ft), 60);
        assert_eq!(nominal_fps(&[0.0]), 60);
        let ft: Vec<f64> = (0..100).map(|i| i as f64 / 30.0).collect();
        assert_eq!(nominal_fps(&ft), 30);
    }

    #[test]
    fn args_seek_absolutely_and_keep_the_audio() {
        let p = ExportParams { video: "in.mp4".into(), start: 12.5, end: 20.0, fps: 60, encoder: vec!["-c:v".into(), "libx264".into()], out: "out.mp4".into() };
        let a = ffmpeg_args(&p).join(" ");
        assert!(a.contains("-seek_timestamp 1 -ss 12.500000 -i in.mp4"), "{a}");
        assert!(a.contains("-framerate 60 -c:v png -i pipe:0"), "{a}");
        assert!(a.contains("[0:v]fps=60[b]") && a.contains("-map 0:a?") && a.contains("-frames:v 450") && a.contains("-t 7.508333"), "{a}");
    }
}
