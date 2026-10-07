//! Exporting a clip with the input overlay burned in, end to end with the real ffmpeg: a
//! variable-frame-rate recording whose frames show their own index (14 squares, top row), an
//! overlay whose frame k shows the index `overlay_export::plan` says output frame k shows (second
//! row), and the exported file checked frame by frame: both rows must agree on every frame.
//! Skipped when ffmpeg isn't installed.

use cv_capture::overlay_export::{self, ExportParams, OverlayJob};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const W: usize = 320;
const H: usize = 180;
const BITS: usize = 14;

fn have_ffmpeg() -> bool {
    Command::new("ffmpeg").arg("-version").output().is_ok()
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("cv-ovx-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d.join(name)
}

/// Squares (12 px, 20 px apart) at row `y`: white = 1.
fn paint(px: &mut [u8], channels: usize, y: usize, value: u32) {
    for b in 0..BITS {
        let on = value >> b & 1 == 1;
        for yy in y..y + 12 {
            for xx in 4 + b * 20..4 + b * 20 + 12 {
                let i = (yy * W + xx) * channels;
                let c = if on { 255 } else { 0 };
                px[i] = c;
                px[i + 1] = c;
                px[i + 2] = c;
                if channels == 4 {
                    px[i + 3] = 255;
                }
            }
        }
    }
}

fn read(px: &[u8], y: usize) -> u32 {
    (0..BITS).filter(|b| px[((y + 6) * W + 4 + b * 20 + 6) * 3] > 128).fold(0, |k, b| k | 1 << b)
}

/// A minimal PNG (RGBA, stored deflate blocks: big but exact).
fn png(rgba: &[u8]) -> Vec<u8> {
    fn crc(data: &[u8]) -> u32 {
        let mut c = 0xffff_ffffu32;
        for &b in data {
            c ^= b as u32;
            for _ in 0..8 {
                c = if c & 1 == 1 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
            }
        }
        !c
    }
    fn chunk(out: &mut Vec<u8>, kind: &[u8], data: &[u8]) {
        out.extend((data.len() as u32).to_be_bytes());
        let mut c = kind.to_vec();
        c.extend(data);
        out.extend(&c);
        out.extend(crc(&c).to_be_bytes());
    }
    let mut raw = Vec::with_capacity((W * 4 + 1) * H);
    for row in rgba.chunks(W * 4) {
        raw.push(0);
        raw.extend(row);
    }
    let mut z = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65535).collect();
    for (i, b) in blocks.iter().enumerate() {
        z.push((i + 1 == blocks.len()) as u8);
        z.extend((b.len() as u16).to_le_bytes());
        z.extend((!(b.len() as u16)).to_le_bytes());
        z.extend(*b);
    }
    let (mut a, mut s) = (1u32, 0u32);
    for &x in &raw {
        a = (a + x as u32) % 65521;
        s = (s + a) % 65521;
    }
    z.extend(((s << 16) | a).to_be_bytes());
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend((W as u32).to_be_bytes());
    ihdr.extend((H as u32).to_be_bytes());
    ihdr.extend([8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

/// A recording made the way the app makes them (our fragmented-MP4 muxer, then the in-place
/// index): 10 s at 60 fps with every 7th frame missing (variable frame rate, like a game that
/// doesn't deliver every frame), the video starting 20 ms after the sound, each frame showing its
/// index; with a sound track.
fn recording(path: &Path) {
    use cv_capture::mp4::*;
    let h264 = tmp("in.h264");
    let aac = tmp("in.aac");
    let kept: Vec<u32> = (0..600u32).filter(|i| i % 7 != 0).collect();
    let mut ff = Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgb24", "-s", &format!("{W}x{H}"), "-framerate", "60", "-i", "pipe:0"])
        .args(["-c:v", "libx264", "-preset", "ultrafast", "-crf", "12", "-g", "60", "-bf", "0", "-pix_fmt", "yuv420p", "-x264-params", "aud=1", "-f", "h264"])
        .arg(&h264)
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = ff.stdin.take().unwrap();
    let mut frame = vec![90u8; W * H * 3];
    for &i in &kept {
        paint(&mut frame, 3, 4, i);
        stdin.write_all(&frame).unwrap();
    }
    drop(stdin);
    assert!(ff.wait().unwrap().success());
    assert!(Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000", "-ac", "2", "-t", "10", "-c:a", "aac", "-f", "adts"])
        .arg(&aac)
        .status()
        .unwrap()
        .success());
    // Access units (split at the AUD NALs), each with its frame's own capture time.
    let data = std::fs::read(&h264).unwrap();
    let mut cuts = vec![0usize];
    for i in 1..data.len().saturating_sub(4) {
        if data[i] == 0 && data[i + 1] == 0 && data[i + 2] == 1 && (data[i + 3] & 0x1f) == 9 {
            cuts.push(if data[i - 1] == 0 { i - 1 } else { i });
        }
    }
    cuts.push(data.len());
    let (mut sps, mut pps, mut packets) = (None, None, Vec::new());
    let mut n = 0;
    for w in cuts.windows(2) {
        let a = annexb_to_avcc(&data[w[0]..w[1]]);
        sps = a.sps.clone().or(sps);
        pps = a.pps.clone().or(pps);
        if a.avcc.is_empty() {
            continue;
        }
        packets.push(Packet { track: 0, pts: 200_000 + kept[n] as i64 * HNS / 60, data: a.avcc, key: a.key });
        n += 1;
    }
    assert_eq!(n, kept.len());
    let data = std::fs::read(&aac).unwrap();
    let (mut p, mut k) = (0, 0i64);
    while p + 7 <= data.len() {
        let len = (((data[p + 3] & 3) as usize) << 11) | ((data[p + 4] as usize) << 3) | ((data[p + 5] as usize) >> 5);
        let hdr = if data[p + 1] & 1 == 1 { 7 } else { 9 };
        packets.push(Packet { track: 1, pts: k * 1024 * HNS / 48000, data: data[p + hdr..p + len].to_vec(), key: true });
        k += 1;
        p += len;
    }
    packets.sort_by_key(|p| p.pts);
    let v = VideoConfig { width: W as u32, height: H as u32, sps: sps.unwrap(), pps: pps.unwrap(), fps: 60 };
    let f = std::fs::File::create(path).unwrap();
    let mut w = FragmentedWriter::new(std::io::BufWriter::new(f), v, vec![AudioConfig::aac_lc(48000, 2, "Game audio")]).unwrap();
    let mut next = HNS;
    for p in packets {
        if p.pts >= next {
            w.flush_fragment().unwrap();
            next += HNS;
        }
        w.push(p);
    }
    w.finish().unwrap();
    cv_capture::remux::index_in_place(path).unwrap();
}

fn decode(path: &Path) -> Vec<Vec<u8>> {
    let out = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(path)
        .args(["-fps_mode", "passthrough", "-f", "rawvideo", "-pix_fmt", "rgb24", "-"])
        .output()
        .unwrap();
    out.stdout.chunks(W * H * 3).map(|c| c.to_vec()).collect()
}

#[test]
fn overlay_frames_land_on_the_frames_they_were_drawn_for() {
    if !have_ffmpeg() {
        eprintln!("ffmpeg not installed: skipped");
        return;
    }
    let src = tmp("recording.mp4");
    recording(&src);
    // The recording's own frame times, as the app reads them (the MP4 index): the same as
    // ffmpeg's (what the plan relies on).
    let ft = cv_capture::remux::frame_times(&src).unwrap();
    let probe = Command::new("ffprobe")
        .args(["-v", "error", "-select_streams", "v", "-show_entries", "frame=pts_time", "-of", "csv=p=0"])
        .arg(&src)
        .output()
        .unwrap();
    let pts: Vec<f64> = String::from_utf8_lossy(&probe.stdout).split_whitespace().map(|x| x.trim_matches(',').parse().unwrap()).collect();
    assert_eq!(ft.len(), pts.len());
    let worst = ft.iter().zip(&pts).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max);
    assert!(worst < 0.0005, "the app's frame times vs ffmpeg's differ by up to {worst} s");
    let fps = overlay_export::nominal_fps(&ft);
    assert_eq!(fps, 60);
    let enc: Vec<String> = ["-c:v", "libx264", "-preset", "ultrafast", "-crf", "12"].iter().map(|s| s.to_string()).collect();
    assert!(overlay_export::encoder_works(Path::new("ffmpeg"), &enc));
    assert!(!overlay_export::encoder_works(Path::new("ffmpeg"), &["-c:v".into(), "no_such_encoder".into()]));

    // Starts on, between and right after missing frames; 30 fps output too.
    for (n, (start, end, out_fps)) in [(3.31, 5.0, fps), (2.0, 3.0, fps), (7.0 / 60.0 * 20.0, 4.2, fps), (6.05, 7.4, 30)].into_iter().enumerate() {
        let times = overlay_export::plan(&ft, start, end, out_fps);
        let out = tmp(&format!("clip{n}.mp4"));
        let p = ExportParams { video: src.clone(), start, end, fps: out_fps, encoder: enc.clone(), out: out.clone() };
        let mut job = OverlayJob::start(Path::new("ffmpeg"), &p, times.len()).unwrap();
        let mut layer = vec![0u8; W * H * 4];
        for t in &times {
            paint(&mut layer, 4, 30, ((t - 0.02) * 60.0).round() as u32);
            job.push(&png(&layer)).unwrap();
        }
        assert_eq!(job.finish().unwrap(), out);
        let frames = decode(&out);
        assert_eq!(frames.len(), times.len(), "clip {n}: one output frame per planned overlay frame");
        let wrong: Vec<(usize, u32, u32)> = frames.iter().enumerate().map(|(k, f)| (k, read(f, 4), read(f, 30))).filter(|(_, v, o)| v != o).collect();
        assert!(wrong.is_empty(), "clip {n} ({start}..{end} at {out_fps} fps): video vs overlay index differ on {} frames: {:?}", wrong.len(), &wrong[..wrong.len().min(8)]);
        // The sound is kept.
        let probe = Command::new("ffprobe")
            .args(["-v", "error", "-select_streams", "a", "-show_entries", "stream=codec_name", "-of", "csv=p=0"])
            .arg(&out)
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&probe.stdout).trim(), "aac", "clip {n}: audio");
    }

    // Cancelling removes the unfinished file; an error is reported, not hidden.
    let out = tmp("cancelled.mp4");
    let p = ExportParams { video: src.clone(), start: 1.0, end: 3.0, fps: 60, encoder: enc.clone(), out: out.clone() };
    let mut job = OverlayJob::start(Path::new("ffmpeg"), &p, 120).unwrap();
    job.push(&png(&vec![0u8; W * H * 4])).unwrap();
    job.cancel();
    assert!(!out.exists());
    let p = ExportParams { video: tmp("missing.mp4"), start: 1.0, end: 2.0, fps: 60, encoder: enc, out: tmp("never.mp4") };
    let job = OverlayJob::start(Path::new("ffmpeg"), &p, 60).unwrap();
    assert!(job.finish().unwrap_err().to_string().contains("ffmpeg failed"));
    std::fs::remove_dir_all(src.parent().unwrap()).ok();
}
