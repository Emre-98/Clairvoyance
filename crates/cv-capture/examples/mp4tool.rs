//! Developer tool for recordings (also shipped to the owner's PC for the replay benchmark).
//!
//!   mp4tool info <file.mp4>                      layout, codec, keyframes, fragments (JSON)
//!   mp4tool finalize <in.mp4> <out.mp4>          fragmented -> faststart (no re-encode)
//!   mp4tool loop <in.mp4> <out.mp4> <seconds>    repeat/cut a fragmented recording to a length
//!   mp4tool cut <in.mp4> <out.mp4> <start s> <seconds>   a piece of a recording (no re-encode)
//!   mp4tool benchsession <src session.json> <dst dir> <video file name> <seconds>
//!                                                 session.json for a benchmark copy of a game
//!   mp4tool fromes <in.h264> <in.aac> <fps> <w> <h> <out.mp4>
//!                                                 Annex-B H.264 (with AUDs) + ADTS AAC -> a
//!                                                 recording written by the recorder's own muxer
//!   mp4tool fakeinput <session dir> [rate]        a realistic input recording (cursor moving the
//!                                                 whole game, clicks, keys) for the overlay
//!                                                 benchmark, compressed + stats, linked in session.json
//!   mp4tool inputinfo <file.input>               summary of an input recording (no positions)
//!   mp4tool ultcheck <session dir> [--write] [--offset <s>]
//!                                                 League ult check of a recorded game (Windows):
//!                                                 the same code the maintenance pass runs

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
        Some("cut") if a.len() == 6 => remux::cut(Path::new(&a[2]), Path::new(&a[3]), a[4].parse().unwrap_or(0.0), a[5].parse().unwrap_or(20.0))
            .map(|t| format!("cut from {t:.3} s"))
            .map_err(|e| e.to_string()),
        Some("benchsession") if a.len() == 6 => bench_session(&a[2], &a[3], &a[4], a[5].parse().unwrap_or(300.0)),
        Some("fromes") if a.len() == 8 => from_es(&a[2], &a[3], a[4].parse().unwrap(), a[5].parse().unwrap(), a[6].parse().unwrap(), &a[7]),
        Some("fakeinput") if a.len() >= 3 => fake_input(Path::new(&a[2]), a.get(3).and_then(|r| r.parse().ok()).unwrap_or(250)),
        Some("inputinfo") if a.len() == 3 => cv_core::input::read(Path::new(&a[2]))
            .map(|f| serde_json::to_string_pretty(&cv_core::input::stats::summarize(&f, std::fs::metadata(&a[2]).map(|m| m.len()).unwrap_or(0))).unwrap())
            .map_err(|e| e.to_string()),
        Some("ultcheck") if a.len() >= 3 => {
            let offset = a.iter().position(|x| x == "--offset").and_then(|i| a.get(i + 1)).and_then(|x| x.parse().ok());
            ult_check(Path::new(&a[2]), a.iter().any(|x| x == "--write"), offset)
        }
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

/// A worst-case-like input recording for a benchmark game: the cursor moves the whole time at
/// `rate` Hz (random walk with flicks), ~2.5 right-clicks/s, ~1.5 keys/s, a 4K client area.
fn fake_input(dir: &Path, rate: u32) -> Result<String, String> {
    use cv_core::input::{self, stats, Record, Rect, WindowInfo};
    let mut s = cv_core::session::GameSession::load(dir).map_err(|e| e.to_string())?;
    let dur = s.video_duration.ok_or("no video duration")?;
    let mut seed = 0x2545F4914F6CDD1Du64;
    let mut rnd = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed >> 11) as f64 / (1u64 << 53) as f64
    };
    let full = Rect { x: 0, y: 0, w: 3840, h: 2160 };
    let mut r = vec![Record::Window { t: 0, info: WindowInfo { client: full, frame: full, dpi: 144 } }, Record::Focus { t: 0, focused: true }];
    let (mut x, mut y, mut vx, mut vy) = (0.5f64, 0.5f64, 0.0f64, 0.0f64);
    let step = 1_000_000 / rate as i64;
    let n = (dur * rate as f64) as i64;
    let keys = [b'Q', b'W', b'E', b'R', b'D', b'F', b'1', b'4', b'B'];
    for i in 0..n {
        let t = i * step;
        if rnd() < 0.01 {
            vx = (rnd() - 0.5) * 0.04;
            vy = (rnd() - 0.5) * 0.04;
        }
        vx *= 0.97;
        vy *= 0.97;
        x = (x + vx + (rnd() - 0.5) * 0.002).clamp(0.02, 0.98);
        y = (y + vy + (rnd() - 0.5) * 0.002).clamp(0.02, 0.98);
        r.push(Record::Cursor { t, x: (x * input::UNIT) as i32, y: (y * input::UNIT) as i32 });
        if rnd() < 2.5 / rate as f64 {
            r.push(Record::Button { t: t + 100, button: input::BTN_RIGHT, down: true });
            r.push(Record::Button { t: t + 60_000, button: input::BTN_RIGHT, down: false });
        }
        if rnd() < 0.3 / rate as f64 {
            r.push(Record::Button { t: t + 200, button: input::BTN_LEFT, down: true });
            r.push(Record::Button { t: t + 70_000, button: input::BTN_LEFT, down: false });
        }
        if rnd() < 1.5 / rate as f64 {
            let k = keys[(rnd() * keys.len() as f64) as usize % keys.len()];
            r.push(Record::Key { t: t + 300, vk: k, down: true });
            r.push(Record::Key { t: t + 90_000, vk: k, down: false });
        }
    }
    r.push(Record::End { t: (dur * 1e6) as i64 });
    let name = format!("{}.input", s.id);
    let path = dir.join(&name);
    input::write_file(&path, rate, &input::Meta { game_id: "league".into(), rate, app_version: "bench".into() }, &r).map_err(|e| e.to_string())?;
    let raw = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    let t = Instant::now();
    let f = input::read(&path).map_err(|e| e.to_string())?;
    let an = stats::Analysis::from_file(&f);
    let mech = stats::mechanics_whole(&an, s.video_offset);
    let heat = stats::heatmap(&an, 0.0, an.end, stats::HEAT_W, stats::HEAT_H);
    let (_, comp) = input::compress_in_place(&path, Some(&heat)).map_err(|e| e.to_string())?;
    let post_ms = t.elapsed().as_millis();
    let t = Instant::now();
    let f2 = input::read(&path).map_err(|e| e.to_string())?;
    let an2 = stats::Analysis::from_file(&f2);
    let payload = stats::ui_payload(&an2, f2.heatmap.as_ref(), f2.rate);
    let load_ms = t.elapsed().as_millis();
    s.input_file = Some(name);
    s.mechanics = Some(mech.clone());
    s.save(dir).map_err(|e| e.to_string())?;
    Ok(format!(
        "{:.0} s at {rate} Hz: {} records, raw {:.2} MB, compressed {:.2} MB; stats + heatmap + compress {post_ms} ms; overlay load (read + decode + payload {:.1} MB) {load_ms} ms; APM {:.0}",
        dur,
        r.len(),
        raw as f64 / 1e6,
        comp as f64 / 1e6,
        payload.len() as f64 / 1e6,
        mech.apm
    ))
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

#[cfg(windows)]
fn ult_check(dir: &Path, write: bool, offset: Option<f64>) -> Result<String, String> {
    use cv_core::game::FrameSource;
    let file = dir.join("session.json");
    let text = std::fs::read_to_string(&file).map_err(|e| e.to_string())?;
    let mut s: cv_core::session::GameSession = serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| e.to_string())?;
    if let Some(o) = offset {
        s.video_offset = o;
    }
    let before: Vec<f64> = s.events.iter().filter(|e| e.kind == cv_core::EventKind::UltPressed).map(|e| e.game_time + s.video_offset).collect();
    let video = dir.join(s.video_file.clone().ok_or("no video")?);
    let t = Instant::now();
    let mut v = cv_capture::win::frames::VideoFrames::open(&video).map_err(|e| format!("{e:#}"))?;
    let open_ms = t.elapsed().as_millis();
    let rules = cv_game_league::ult::UltRules::builtin();
    cv_game_league::verify::verify(&mut s, &mut v, &rules, &|| false).map_err(|e| format!("{e:#}"))?;
    let events: Vec<serde_json::Value> = s
        .events
        .iter()
        .filter(|e| e.id.starts_with("ult"))
        .map(|e| serde_json::json!({"video_t": (e.game_time + s.video_offset), "kind": e.kind, "title": e.title, "details": e.details}))
        .collect();
    let report = serde_json::json!({
        "video": video,
        "hardware_decoding": v.hardware,
        "frames_decoded": v.frames_decoded,
        "seeks": v.seeks,
        "keyframes": v.keyframes().len(),
        "open_ms": open_ms,
        "total_ms": t.elapsed().as_millis(),
        "before_ult_pressed_video_t": before,
        "verification": s.verification,
        "events": events,
    });
    if write {
        std::fs::write(&file, serde_json::to_string_pretty(&s).unwrap()).map_err(|e| e.to_string())?;
    }
    Ok(serde_json::to_string_pretty(&report).unwrap())
}

#[cfg(not(windows))]
fn ult_check(_dir: &Path, _write: bool, _offset: Option<f64>) -> Result<String, String> {
    Err("ultcheck needs Windows (Media Foundation)".into())
}
