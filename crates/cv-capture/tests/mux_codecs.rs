//! v1.8: HEVC and AV1 recordings. Real streams made by ffmpeg (x265 Annex B with AUDs, SVT-AV1
//! in IVF: what the hardware encoders hand the muxer) go through the recorder's writer, the
//! in-place index, the copying finalize and a replay-buffer clip; ffprobe / ffmpeg must read
//! every frame without errors. Skipped when ffmpeg (or the encoder) isn't installed.

use cv_capture::mp4::*;
use cv_capture::remux;
use std::process::Command;

fn tmp(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("grcap-codecs-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d.join(name)
}

fn ffmpeg(args: &[&str]) -> bool {
    Command::new("ffmpeg").args(["-v", "error", "-y"]).args(args).status().map(|s| s.success()).unwrap_or(false)
}

fn probe(path: &std::path::Path, args: &[&str]) -> String {
    let o = Command::new("ffprobe").args(["-v", "error"]).args(args).arg(path).output().unwrap();
    String::from_utf8_lossy(&o.stdout).to_string()
}

fn decode_errors(path: &std::path::Path) -> String {
    let o = Command::new("ffmpeg").args(["-v", "error", "-i"]).arg(path).args(["-map", "0", "-f", "null", "-"]).output().unwrap();
    String::from_utf8_lossy(&o.stderr).to_string()
}

const SECS: u32 = 6;
const FPS: i64 = 30;

/// Encoder output buffers (one per frame) and keyframe flags.
fn hevc_frames() -> Option<Vec<(Vec<u8>, bool)>> {
    let f = tmp("in.hevc");
    let ok = ffmpeg(&["-f", "lavfi", "-i", &format!("testsrc2=size=640x360:rate={FPS}"), "-t", &SECS.to_string(), "-c:v", "libx265", "-x265-params", "aud=1:bframes=0:keyint=30:min-keyint=30:repeat-headers=1:log-level=error", "-f", "hevc", f.to_str().unwrap()]);
    if !ok {
        return None;
    }
    let data = std::fs::read(&f).unwrap();
    // Split at AUD NAL units (type 35).
    let mut cuts = vec![];
    for i in 0..data.len().saturating_sub(4) {
        if data[i] == 0 && data[i + 1] == 0 && data[i + 2] == 1 && (data[i + 3] >> 1) & 0x3F == 35 {
            cuts.push(if i > 0 && data[i - 1] == 0 { i - 1 } else { i });
        }
    }
    cuts.push(data.len());
    Some(cuts.windows(2).map(|w| (data[w[0]..w[1]].to_vec(), false)).collect())
}

fn av1_frames() -> Option<Vec<(Vec<u8>, bool)>> {
    let f = tmp("in.ivf");
    let ok = ffmpeg(&["-f", "lavfi", "-i", &format!("testsrc2=size=640x360:rate={FPS}"), "-t", &SECS.to_string(), "-c:v", "libsvtav1", "-preset", "12", "-g", "30", "-svtav1-params", "keyint=30:pred-struct=1", "-f", "ivf", f.to_str().unwrap()]);
    if !ok {
        return None;
    }
    let keys: Vec<bool> = probe(&f, &["-select_streams", "v:0", "-show_entries", "packet=flags", "-of", "csv=p=0"]).lines().map(|l| l.starts_with('K')).collect();
    let data = std::fs::read(&f).unwrap();
    let mut p = u16::from_le_bytes([data[6], data[7]]) as usize;
    let mut out = vec![];
    while p + 12 <= data.len() {
        let n = u32::from_le_bytes(data[p..p + 4].try_into().unwrap()) as usize;
        let k = keys.get(out.len()).copied().unwrap_or(false);
        out.push((data[p + 12..p + 12 + n].to_vec(), k));
        p += 12 + n;
    }
    Some(out)
}

fn check(codec: Codec, frames: Vec<(Vec<u8>, bool)>, ff_name: &str, entry: &str) {
    let mut params = ParamSets::default();
    let mut video = vec![];
    for (n, (buf, enc_key)) in frames.iter().enumerate() {
        let au = to_sample(codec, buf);
        params.update(&au);
        if au.avcc.is_empty() {
            continue;
        }
        video.push(Packet { track: 0, pts: n as i64 * HNS / FPS + 200_000, data: au.avcc, key: au.key || *enc_key });
    }
    assert!(params.ready(codec), "{codec:?}: parameter sets found");
    assert!(video[0].key, "starts with a keyframe");
    let keys = video.iter().filter(|p| p.key).count();
    assert_eq!(keys, (SECS as usize * FPS as usize).div_ceil(30), "{codec:?}: a keyframe every second");
    let vc = params.config(codec, 640, 360, FPS as u32);
    let acfg = vec![AudioConfig::aac_lc(48000, 2, "Game audio")];
    let audio: Vec<Packet> = (0..(SECS as i64 * 48000 / 1024)).map(|n| Packet { track: 1, pts: n * 1024 * HNS / 48000, data: vec![0x21, 0x10, 0x04, 0x60, 0x8C, 0x1C], key: true }).collect();
    let mut all: Vec<Packet> = video.iter().cloned().chain(audio).collect();
    all.sort_by_key(|p| p.pts);
    let name = codec.as_str();

    // The full recording as the recorder writes it, then indexed in place.
    let src = tmp(&format!("{name}-rec.mp4"));
    let mut w = FragmentedWriter::new(std::io::BufWriter::new(std::fs::File::create(&src).unwrap()), vc.clone(), acfg.clone()).unwrap();
    let mut next = HNS;
    for p in all.iter().cloned() {
        if p.pts >= next {
            w.flush_fragment().unwrap();
            next += HNS;
        }
        w.push(p);
    }
    w.finish().unwrap();
    let frames_of = |p: &std::path::Path| probe(p, &["-count_frames", "-select_streams", "v:0", "-show_entries", "stream=nb_read_frames", "-of", "csv=p=0"]).trim().to_string();
    let want = (SECS as i64 * FPS).to_string();
    assert_eq!(probe(&src, &["-select_streams", "v:0", "-show_entries", "stream=codec_name", "-of", "csv=p=0"]).trim(), ff_name);
    let e = decode_errors(&src);
    assert!(e.trim().is_empty(), "{name} fragmented: {e}");
    assert_eq!(frames_of(&src), want, "{name} fragmented");

    let copy = tmp(&format!("{name}-copy.mp4"));
    remux::finalize(&src, &copy, &|| false).unwrap();
    let e = decode_errors(&copy);
    assert!(e.trim().is_empty(), "{name} finalized: {e}");
    assert_eq!(frames_of(&copy), want, "{name} finalized");
    let info = remux::info(&copy).unwrap();
    assert_eq!(info.codec.as_deref(), Some(entry), "{info:?}");
    assert!((info.keyframe_interval_avg - 1.0).abs() < 0.05, "{info:?}");

    let rep = remux::index_in_place(&src).unwrap();
    assert!(rep.indexed, "{rep:?}");
    assert_eq!(remux::probe(&src).unwrap(), remux::Layout::Faststart);
    let e = decode_errors(&src);
    assert!(e.trim().is_empty(), "{name} indexed in place: {e}");
    assert_eq!(frames_of(&src), want, "{name} indexed in place");

    // A replay-buffer clip (the last 3 s).
    let mut rb = cv_capture::replay::ReplayBuffer::new(3, usize::MAX);
    for p in all {
        rb.push(p);
    }
    let (packets, start) = rb.snapshot(3);
    let clip = tmp(&format!("{name}-clip.mp4"));
    write_clip(std::io::BufWriter::new(std::fs::File::create(&clip).unwrap()), &vc, &acfg, &packets, start).unwrap();
    let e = decode_errors(&clip);
    assert!(e.trim().is_empty(), "{name} clip: {e}");
    let n: usize = frames_of(&clip).parse().unwrap();
    assert!((90..=150).contains(&n), "{name} clip frames {n}");
    // Stream copy (the clip editor's normal export) keeps the codec.
    let cut = tmp(&format!("{name}-cut.mp4"));
    assert!(ffmpeg(&["-ss", "2", "-i", copy.to_str().unwrap(), "-t", "2", "-map", "0", "-c", "copy", "-avoid_negative_ts", "make_zero", cut.to_str().unwrap()]));
    let e = decode_errors(&cut);
    assert!(e.trim().is_empty(), "{name} stream-copied clip: {e}");
}

#[test]
fn hevc_recording_index_and_clip() {
    match hevc_frames() {
        Some(f) => check(Codec::Hevc, f, "hevc", "hvc1"),
        None => eprintln!("ffmpeg with libx265 not installed, skipping"),
    }
}

#[test]
fn av1_recording_index_and_clip() {
    match av1_frames() {
        Some(f) => check(Codec::Av1, f, "av1", "av01"),
        None => eprintln!("ffmpeg with libsvtav1 not installed, skipping"),
    }
}

#[test]
fn config_records() {
    // hvcC from a real x265 SPS: the same profile / level ffprobe reads from the stream.
    if let Some(f) = hevc_frames() {
        let au = annexb_to_hvcc(&f[0].0);
        let vc = VideoConfig { codec: Codec::Hevc, vps: au.vps.unwrap(), sps: au.sps.unwrap(), pps: au.pps.unwrap(), width: 640, height: 360, fps: 30, ..Default::default() };
        let c = hvcc(&vc);
        assert_eq!(c[0], 1);
        assert_eq!(c[1] & 0x1F, 1, "Main profile");
        let pl = probe(&tmp("in.hevc"), &["-show_entries", "stream=profile,level", "-of", "csv=p=0"]);
        assert_eq!(format!("Main,{}", c[12]), pl.trim(), "level as ffprobe reads it");
        assert_eq!(c[22], 3, "VPS, SPS, PPS");
    }
    if let Some(f) = av1_frames() {
        let au = obus_to_sample(&f[0].0);
        let c = av1c(au.seq.as_ref().unwrap());
        assert_eq!(c[0], 0x81);
        assert_eq!(c[1] >> 5, 0, "Main profile");
        let pl = probe(&tmp("in.ivf"), &["-show_entries", "stream=level", "-of", "csv=p=0"]);
        assert_eq!((c[1] & 0x1F).to_string(), pl.trim(), "seq_level_idx as ffprobe reads it");
        assert_eq!(&c[4..], &au.seq.unwrap()[..]);
    }
}
