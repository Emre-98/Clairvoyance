//! Developer tool for recordings (also shipped to the owner's PC for the replay benchmark).
//!
//!   mp4tool info <file.mp4>                      layout, codec, keyframes, fragments (JSON)
//!   mp4tool finalize <in.mp4> <out.mp4>          fragmented -> faststart (no re-encode)
//!   mp4tool loop <in.mp4> <out.mp4> <seconds>    repeat/cut a fragmented recording to a length
//!   mp4tool benchsession <src session.json> <dst dir> <video file name> <seconds>
//!                                                 session.json for a benchmark copy of a game
//!   mp4tool fromes <in.h264> <in.aac> <fps> <w> <h> <out.mp4>
//!                                                 Annex-B H.264 (with AUDs) + ADTS AAC -> a
//!                                                 recording written by the recorder's own muxer

use cv_capture::mp4::*;
use cv_capture::remux;
use std::path::Path;
use std::time::Instant;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let t = Instant::now();
    let r: Result<String, String> = match a.get(1).map(|s| s.as_str()) {
        Some("info") if a.len() == 3 => remux::info(Path::new(&a[2])).map(|i| serde_json::to_string_pretty(&i).unwrap()).map_err(|e| e.to_string()),
        Some("finalize") if a.len() == 4 => remux::finalize(Path::new(&a[2]), Path::new(&a[3]), &|| false)
            .map(|r| serde_json::to_string_pretty(&r).unwrap())
            .map_err(|e| e.to_string()),
        Some("loop") if a.len() == 5 => remux::loop_recording(Path::new(&a[2]), Path::new(&a[3]), a[4].parse().unwrap_or(60.0))
            .map(|d| format!("wrote {d:.1} s"))
            .map_err(|e| e.to_string()),
        Some("benchsession") if a.len() == 6 => bench_session(&a[2], &a[3], &a[4], a[5].parse().unwrap_or(300.0)),
        Some("fromes") if a.len() == 8 => from_es(&a[2], &a[3], a[4].parse().unwrap(), a[5].parse().unwrap(), a[6].parse().unwrap(), &a[7]),
        _ => Err("usage: mp4tool info|finalize|loop|fromes ... (see the source)".into()),
    };
    match r {
        Ok(s) => {
            println!("{s}");
            eprintln!("took {} ms", t.elapsed().as_millis());
        }
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}

fn from_es(h264: &str, aac: &str, fps: i64, w: u32, h: u32, out: &str) -> Result<String, String> {
    let data = std::fs::read(h264).map_err(|e| e.to_string())?;
    // Split into access units at AUD NAL units.
    let mut aus: Vec<&[u8]> = Vec::new();
    let mut cur = 0;
    let mut i = 0;
    while i + 4 < data.len() {
        if data[i] == 0 && data[i + 1] == 0 && data[i + 2] == 1 && (data[i + 3] & 0x1F) == 9 && i > 0 {
            let s = if data[i - 1] == 0 { i - 1 } else { i };
            if s > cur {
                aus.push(&data[cur..s]);
                cur = s;
            }
        }
        i += 1;
    }
    aus.push(&data[cur..]);
    let (mut sps, mut pps) = (None, None);
    let mut pk = Vec::new();
    let mut n = 0i64;
    for au in aus {
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
        pk.push(Packet { track: 0, pts: n * HNS / fps, data: a.avcc, key: a.key });
        n += 1;
    }
    let data = std::fs::read(aac).map_err(|e| e.to_string())?;
    let mut p = 0;
    let mut k = 0i64;
    while p + 7 <= data.len() {
        let len = (((data[p + 3] & 3) as usize) << 11) | ((data[p + 4] as usize) << 3) | ((data[p + 5] as usize) >> 5);
        let hdr = if data[p + 1] & 1 == 1 { 7 } else { 9 };
        if len < hdr || p + len > data.len() {
            break;
        }
        pk.push(Packet { track: 1, pts: k * 1024 * HNS / 48000, data: data[p + hdr..p + len].to_vec(), key: true });
        k += 1;
        p += len;
    }
    pk.sort_by_key(|p| p.pts);
    let vc = VideoConfig { width: w, height: h, sps: sps.ok_or("no SPS")?, pps: pps.ok_or("no PPS")?, fps: fps as u32 };
    let f = std::fs::File::create(out).map_err(|e| e.to_string())?;
    let mut wr = FragmentedWriter::new(std::io::BufWriter::new(f), vc, vec![AudioConfig::aac_lc(48000, 2, "Game audio")]).map_err(|e| e.to_string())?;
    let mut next = HNS;
    for p in pk {
        if p.pts >= next {
            wr.flush_fragment().map_err(|e| e.to_string())?;
            next += HNS;
        }
        wr.push(p);
    }
    wr.finish().map_err(|e| e.to_string())?;
    Ok(format!("{n} frames, {k} audio packets"))
}

fn bench_session(src: &str, dst_dir: &str, video: &str, secs: f64) -> Result<String, String> {
    let text = std::fs::read_to_string(src).map_err(|e| e.to_string())?;
    let mut v: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let id = Path::new(dst_dir).file_name().unwrap().to_string_lossy().to_string();
    let offset = v["video_offset"].as_f64().unwrap_or(0.0);
    v["id"] = id.clone().into();
    v["video_file"] = video.into();
    v["video_duration"] = secs.into();
    v["game_duration"] = (secs - offset).max(0.0).into();
    v["favorite"] = true.into(); // never touched by the storage clean-up
    v["clips"] = serde_json::json!([]);
    v["mode_name"] = format!("Benchmark ({} min)", (secs / 60.0).round()).into();
    if let Some(ev) = v["events"].as_array_mut() {
        ev.retain(|e| e["game_time"].as_f64().unwrap_or(0.0) + offset <= secs - 6.0);
        // Few real events this early in a game: add markers spread over the video, so marker
        // jumps are measured all over it.
        if ev.len() < 8 {
            for i in 1..=8 {
                let at = secs * i as f64 / 9.0 - offset;
                ev.push(serde_json::json!({ "id": format!("bench{i}"), "kind": "manual_marker", "game_time": at, "title": format!("Benchmark marker {i}") }));
            }
        }
    }
    if let Some(tl) = v["timeline"].as_array_mut() {
        tl.retain(|p| p["t"].as_f64().unwrap_or(0.0) <= secs - offset);
    }
    std::fs::create_dir_all(dst_dir).map_err(|e| e.to_string())?;
    std::fs::write(Path::new(dst_dir).join("session.json"), serde_json::to_string_pretty(&v).unwrap()).map_err(|e| e.to_string())?;
    Ok(format!("{id}: {} events", v["events"].as_array().map(|a| a.len()).unwrap_or(0)))
}
