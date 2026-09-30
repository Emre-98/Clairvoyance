//! Muxes real H.264/AAC (made by ffmpeg) and checks the files with ffprobe/ffmpeg.
//! Skipped when ffmpeg isn't installed.

use cv_capture::mp4::*;
use std::process::Command;

fn have_ffmpeg() -> bool {
    Command::new("ffmpeg").arg("-version").output().is_ok()
}

fn tmp(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("grcap-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d.join(name)
}

/// (video config, video packets, audio packets) for `secs` seconds.
fn sources(secs: u32) -> (VideoConfig, Vec<Packet>, Vec<Packet>) {
    let h264 = tmp("in.h264");
    let aac = tmp("in.aac");
    let st = Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "lavfi", "-i", "testsrc2=size=640x360:rate=30", "-t", &secs.to_string()])
        .args(["-c:v", "libx264", "-bf", "0", "-g", "60", "-profile:v", "high", "-x264-params", "aud=1", "-f", "h264"])
        .arg(&h264)
        .status()
        .unwrap();
    assert!(st.success());
    let st = Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000", "-ac", "2", "-t", &secs.to_string(), "-c:a", "aac", "-f", "adts"])
        .arg(&aac)
        .status()
        .unwrap();
    assert!(st.success());
    // Split video into access units at AUD NALs.
    let data = std::fs::read(&h264).unwrap();
    let mut aus: Vec<Vec<u8>> = Vec::new();
    let mut i = 0;
    let mut cur_start = 0;
    while i + 4 < data.len() {
        let sc = data[i] == 0 && data[i + 1] == 0 && data[i + 2] == 1;
        if sc && (data[i + 3] & 0x1F) == 9 && i > 0 {
            let s = if i > 0 && data[i - 1] == 0 { i - 1 } else { i };
            if s > cur_start {
                aus.push(data[cur_start..s].to_vec());
                cur_start = s;
            }
        }
        i += 1;
    }
    aus.push(data[cur_start..].to_vec());
    let mut sps = None;
    let mut pps = None;
    let mut video = Vec::new();
    for (n, au) in aus.iter().enumerate() {
        let a = annexb_to_avcc(au);
        if a.sps.is_some() {
            sps = a.sps.clone();
        }
        if a.pps.is_some() {
            pps = a.pps.clone();
        }
        if a.avcc.is_empty() {
            continue;
        }
        video.push(Packet { track: 0, pts: n as i64 * HNS / 30, data: a.avcc, key: a.key });
    }
    // ADTS -> raw AAC frames.
    let data = std::fs::read(&aac).unwrap();
    let mut audio = Vec::new();
    let mut p = 0;
    let mut n = 0i64;
    while p + 7 <= data.len() {
        let len = (((data[p + 3] & 3) as usize) << 11) | ((data[p + 4] as usize) << 3) | ((data[p + 5] as usize) >> 5);
        let hdr = if data[p + 1] & 1 == 1 { 7 } else { 9 };
        audio.push(Packet { track: 1, pts: n * 1024 * HNS / 48000, data: data[p + hdr..p + len].to_vec(), key: true });
        n += 1;
        p += len;
    }
    let v = VideoConfig { width: 640, height: 360, sps: sps.unwrap(), pps: pps.unwrap(), fps: 30 };
    (v, video, audio)
}

fn probe(path: &std::path::Path, args: &[&str]) -> String {
    let o = Command::new("ffprobe").args(["-v", "error"]).args(args).arg(path).output().unwrap();
    String::from_utf8_lossy(&o.stdout).to_string()
}

fn decode_errors(path: &std::path::Path) -> String {
    let o = Command::new("ffmpeg").args(["-v", "error", "-i"]).arg(path).args(["-map", "0", "-f", "null", "-"]).output().unwrap();
    String::from_utf8_lossy(&o.stderr).to_string()
}

#[test]
fn fragmented_recording_and_clip() {
    if !have_ffmpeg() {
        eprintln!("ffmpeg not installed, skipping");
        return;
    }
    let (vc, video, audio) = sources(6);
    let mic: Vec<Packet> = audio.iter().map(|p| Packet { track: 2, ..p.clone() }).collect();
    let acfg = vec![AudioConfig::aac_lc(48000, 2, "Game audio"), AudioConfig::aac_lc(48000, 2, "Microphone")];
    // Interleave by time like the live pipeline, with a small start delay for the video.
    let mut all: Vec<Packet> = video.iter().map(|p| Packet { pts: p.pts + 200_000, ..p.clone() }).chain(audio.clone()).chain(mic).collect();
    all.sort_by_key(|p| p.pts);

    let path = tmp("rec.mp4");
    let f = std::fs::File::create(&path).unwrap();
    let mut w = FragmentedWriter::new(std::io::BufWriter::new(f), vc.clone(), acfg.clone()).unwrap();
    let mut next_flush = HNS;
    for p in all.iter().cloned() {
        if p.pts >= next_flush {
            w.flush_fragment().unwrap();
            next_flush += HNS;
        }
        w.push(p);
    }
    w.finish().unwrap();

    let streams = probe(&path, &["-show_entries", "stream=codec_name", "-of", "csv=p=0"]);
    assert_eq!(streams.lines().collect::<Vec<_>>(), vec!["h264", "aac", "aac"], "{streams}");
    let dur: f64 = probe(&path, &["-show_entries", "format=duration", "-of", "csv=p=0"]).trim().parse().unwrap();
    assert!((dur - 6.0).abs() < 0.4, "duration {dur}");
    let errs = decode_errors(&path);
    assert!(errs.trim().is_empty(), "decode errors: {errs}");
    let frames = probe(&path, &["-count_frames", "-select_streams", "v:0", "-show_entries", "stream=nb_read_frames", "-of", "csv=p=0"]);
    assert_eq!(frames.trim(), "180");
    let vstart: f64 = probe(&path, &["-select_streams", "v:0", "-show_entries", "stream=start_time", "-of", "csv=p=0"]).trim().parse().unwrap();
    assert!((vstart - 0.02).abs() < 0.01, "video starts at its capture time: {vstart}");

    // Crash in the middle of the file: everything before the last fragment still plays.
    let bytes = std::fs::read(&path).unwrap();
    let cut = tmp("crashed.mp4");
    std::fs::write(&cut, &bytes[..bytes.len() * 6 / 10]).unwrap();
    let frames = probe(&cut, &["-count_frames", "-select_streams", "v:0", "-show_entries", "stream=nb_read_frames", "-of", "csv=p=0"]);
    let n: u32 = frames.trim().parse().unwrap_or(0);
    assert!(n >= 90, "only {n} frames survived the crash");

    // Replay-buffer clip: the last ~3 s, as a normal MP4.
    let clip = tmp("clip.mp4");
    let mut rb = cv_capture::replay::ReplayBuffer::new(3, usize::MAX);
    for p in all.iter().cloned() {
        rb.push(p);
    }
    let (packets, start) = rb.snapshot(3);
    let f = std::fs::File::create(&clip).unwrap();
    let d = write_clip(std::io::BufWriter::new(f), &vc, &acfg, &packets, start).unwrap();
    assert!(d >= 3.0 && d <= 5.1, "clip {d}");
    let errs = decode_errors(&clip);
    assert!(errs.trim().is_empty(), "clip decode errors: {errs}");
    let streams = probe(&clip, &["-show_entries", "stream=codec_name", "-of", "csv=p=0"]);
    assert_eq!(streams.lines().count(), 3);
    // faststart: moov before mdat
    let head = std::fs::read(&clip).unwrap();
    assert_eq!(&head[4..8], b"ftyp");
    let moov_at = head.windows(4).position(|w| w == b"moov").unwrap();
    let mdat_at = head.windows(4).position(|w| w == b"mdat").unwrap();
    assert!(moov_at < mdat_at);
}
