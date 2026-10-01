use super::stats::*;
use super::*;

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("cv-input-{}-{name}", std::process::id()));
    let _ = std::fs::create_dir_all(&d);
    d.join("g.input")
}

fn win(w: i32, h: i32) -> WindowInfo {
    WindowInfo { client: Rect { x: 0, y: 0, w, h }, frame: Rect { x: 0, y: 0, w, h }, dpi: 144 }
}

fn u(v: f64) -> i32 {
    (v * UNIT) as i32
}

/// A small game: focus at 1 s, a square path, clicks and keys, chat in the middle.
fn sample() -> Vec<Record> {
    let mut r = vec![Record::Window { t: 0, info: win(3840, 2160) }, Record::Focus { t: 1_000_000, focused: true }];
    // Cursor at 250 Hz from 1 s to 9 s, moving along x then y.
    for i in 0..2000 {
        let t = 1_000_000 + i * 4000;
        let p = i as f64 / 2000.0;
        let (x, y) = if p < 0.5 { (0.2 + p, 0.2) } else { (0.7, 0.2 + (p - 0.5)) };
        r.push(Record::Cursor { t, x: u(x), y: u(y) });
    }
    r.push(Record::Button { t: 2_000_000, button: BTN_RIGHT, down: true });
    r.push(Record::Button { t: 2_050_000, button: BTN_RIGHT, down: false });
    r.push(Record::Button { t: 3_000_000, button: BTN_LEFT, down: true });
    r.push(Record::Button { t: 3_080_000, button: BTN_LEFT, down: false });
    r.push(Record::Key { t: 4_000_000, vk: b'Q', down: true });
    r.push(Record::Key { t: 4_100_000, vk: b'Q', down: false });
    r.push(Record::Wheel { t: 4_500_000, delta: -120 });
    r.push(Record::Chat { t: 5_000_000, open: true });
    r.push(Record::Chat { t: 6_000_000, open: false });
    r.push(Record::Focus { t: 9_000_000, focused: false });
    r.push(Record::End { t: 10_000_000 });
    r
}

#[test]
fn varints_roundtrip() {
    let mut b = Vec::new();
    for v in [0i64, 1, -1, 63, -64, 1 << 40, -(1 << 40), i64::MAX / 2] {
        put_i(&mut b, v);
    }
    let mut r = Rd { b: &b, i: 0 };
    for v in [0i64, 1, -1, 63, -64, 1 << 40, -(1 << 40), i64::MAX / 2] {
        assert_eq!(r.i(), Some(v));
    }
}

#[test]
fn file_roundtrip_raw_and_compressed() {
    let p = tmp("rt");
    let recs = sample();
    write_file(&p, 250, &Meta { game_id: "league".into(), rate: 250, app_version: "t".into() }, &recs).unwrap();
    let f = read(&p).unwrap();
    assert_eq!(f.records, recs);
    assert!(!f.compressed && !f.truncated);
    assert_eq!(f.rate, 250);
    assert_eq!(f.meta.as_ref().unwrap().game_id, "league");
    let raw_len = std::fs::metadata(&p).unwrap().len();
    let an = Analysis::from_file(&f);
    let h = heatmap(&an, 0.0, 10.0, HEAT_W, HEAT_H);
    let p2 = tmp("rt2");
    std::fs::copy(&p, &p2).unwrap();
    let (a, b) = compress_in_place(&p2, None).unwrap();
    assert_eq!(a, raw_len);
    assert!(b < a / 2, "compressed {b} vs raw {a}");
    compress_in_place(&p, Some(&h)).unwrap();
    let c = read(&p).unwrap();
    assert!(c.compressed);
    assert_eq!(c.records, recs);
    assert_eq!(c.heatmap.as_ref().unwrap().w, HEAT_W);
    let hv = &c.heatmap.as_ref().unwrap().values;
    assert!(hv.iter().cloned().fold(0.0, f32::max) > 0.99);
}

#[test]
fn writer_flushes_blocks_and_survives_a_torn_tail() {
    let p = tmp("writer");
    let w = InputWriter::create(&p, 1_000, 250, &Meta::default()).unwrap();
    assert_eq!(w.video_us(1_000 + 10 * 1234), 1234);
    w.push(Record::Window { t: 0, info: win(1920, 1080) });
    w.push(Record::Focus { t: 0, focused: true });
    assert!(w.is_focused());
    for i in 0..3000i64 {
        // 25 s at ~120 Hz: at least two flushed blocks.
        w.push(Record::Cursor { t: i * 8333, x: u(0.5) + (i as i32 % 100) * 10, y: u(0.5) });
    }
    w.push(Record::Button { t: 12_000_000, button: BTN_X2, down: true });
    assert_eq!(w.button_downs(), vec![(12.0, BTN_X2)]);
    // Before finish: whatever was flushed reads back (a crash keeps that).
    let mid = read(&p).unwrap();
    assert!(mid.records.len() >= 2000, "{} records flushed", mid.records.len());
    let (bytes, n) = w.finish(26_000_000);
    assert_eq!(n, 3003);
    let full = read(&p).unwrap();
    assert_eq!(full.records.len(), 3004);
    assert_eq!(bytes, std::fs::metadata(&p).unwrap().len() - serde_json::to_vec(&Meta::default()).unwrap().len() as u64 - 9);
    // Torn tail (power cut while writing the last block): the earlier blocks still read.
    let mut b = std::fs::read(&p).unwrap();
    b.truncate(b.len() - 5);
    let torn = parse(&b).unwrap();
    assert!(torn.truncated);
    assert!(torn.records.len() >= 2000 && torn.records.len() < 3004);
    // Pushes after finish are ignored.
    w.push(Record::Key { t: 27_000_000, vk: 1, down: true });
    assert_eq!(read(&p).unwrap().records.len(), 3004);
}

#[test]
fn chat_records_only_on_change() {
    let p = tmp("chat");
    let w = InputWriter::create(&p, 0, 250, &Meta::default()).unwrap();
    w.push(Record::Chat { t: 1, open: false });
    w.push(Record::Chat { t: 2, open: true });
    assert!(w.chat_open());
    w.push(Record::Chat { t: 3, open: true });
    w.push(Record::Chat { t: 4, open: false });
    w.finish(5);
    let f = read(&p).unwrap();
    assert_eq!(f.records.iter().filter(|r| matches!(r, Record::Chat { .. })).count(), 2);
}

#[test]
fn mechanics_numbers() {
    let an = Analysis::from_records(&sample());
    assert_eq!(an.focus, vec![(1.0, 9.0)]);
    assert_eq!(an.chat, vec![(5.0, 6.0)]);
    let m = mechanics(&an, 0.0, 10.0, 0.0);
    assert_eq!((m.clicks, m.right_clicks, m.key_presses), (2, 1, 1));
    assert_eq!(m.focused_secs, 8.0);
    // 3 actions in 8 focused seconds.
    assert!((m.apm - 22.5).abs() < 0.01, "{}", m.apm);
    assert!((m.right_click_hz - 0.13).abs() < 0.01);
    // Path: 0.5 widths along x, then 0.5 heights = 0.5 * 9/16 widths along y.
    assert!((m.cursor_distance - (0.5 + 0.5 * 0.5625)).abs() < 0.05, "{}", m.cursor_distance);
    // Clicks at 2 s and 3 s are on a straight stretch: efficiency ~1.
    assert!(m.path_efficiency.unwrap() > 0.98, "{:?}", m.path_efficiency);
    // The cursor moved the whole time: no idle.
    assert_eq!(m.idle_secs, 0.0);
    // Per-minute: one bin.
    assert_eq!(m.apm_per_min.len(), 1);
    // Range: 2.5-10 s has the left click and the key press.
    let r = mechanics(&an, 2.5, 10.0, 0.0);
    assert_eq!((r.clicks, r.right_clicks, r.key_presses), (1, 0, 1));
}

#[test]
fn idle_and_efficiency_detours() {
    let mut r = vec![Record::Window { t: 0, info: win(1000, 1000) }, Record::Focus { t: 0, focused: true }];
    // Click at (0.1, 0.5), detour up to y=0.1, then click at (0.9, 0.5): efficiency < 1.
    r.push(Record::Cursor { t: 0, x: u(0.1), y: u(0.5) });
    r.push(Record::Button { t: 10_000, button: BTN_RIGHT, down: true });
    r.push(Record::Cursor { t: 500_000, x: u(0.5), y: u(0.1) });
    r.push(Record::Cursor { t: 1_000_000, x: u(0.9), y: u(0.5) });
    r.push(Record::Button { t: 1_010_000, button: BTN_RIGHT, down: true });
    // Then nothing for 4 s: idle.
    r.push(Record::Key { t: 5_010_000, vk: b'W', down: true });
    r.push(Record::End { t: 6_000_000 });
    let an = Analysis::from_records(&r);
    let m = mechanics(&an, 0.0, 6.0, 0.0);
    let path = 2.0 * (0.4f64 * 0.4 + 0.4 * 0.4).sqrt();
    let eff = 0.8 / path;
    assert!((m.path_efficiency.unwrap() - eff).abs() < 0.01, "{:?} vs {eff}", m.path_efficiency);
    // Idle: 1.01..5.01 (4 s) and 5.01..6.0 is < 1 s.
    assert!((m.idle_secs - 4.0).abs() < 0.01, "{}", m.idle_secs);
}

#[test]
fn per_minute_with_negative_offset() {
    // App started 30 s into the game: video 0 = game 0:30.
    let mut r = vec![Record::Focus { t: 0, focused: true }];
    for s in 0..120 {
        r.push(Record::Key { t: s * 1_000_000, vk: b'Q', down: true });
    }
    r.push(Record::End { t: 120_000_000 });
    let an = Analysis::from_records(&r);
    let m = mechanics_whole(&an, -30.0);
    // Game minutes 0 (from 0:30 on), 1, 2 (to 2:30).
    assert_eq!(m.apm_per_min.len(), 3);
    assert_eq!(m.apm_per_min[1], Some(60.0));
    assert!((m.apm - 60.0).abs() < 0.6);
}

#[test]
fn gaps_break_the_trail_and_payload_layout() {
    let mut r = sample();
    r.push(Record::Focus { t: 9_500_000, focused: true });
    r.push(Record::Cursor { t: 9_600_000, x: u(0.1), y: u(0.1) });
    let an = Analysis::from_records(&r);
    let last = an.moves.last().unwrap();
    assert!(last.brk, "first sample after focus came back starts a new stroke");
    assert!(an.moves[0].brk && !an.moves[1].brk);
    let h = heatmap(&an, 0.0, 10.0, 8, 4);
    let b = ui_payload(&an, Some(&h), 250);
    assert_eq!(&b[..4], b"CVIV");
    let rd = |i: usize| u32::from_le_bytes(b[i..i + 4].try_into().unwrap());
    let (nm, nc, nk, ng, nw, hw, hh, rate) = (rd(8), rd(12), rd(16), rd(20), rd(24), rd(28), rd(32), rd(36));
    assert_eq!((nm as usize, nc as usize, nk as usize, nw, hw, hh, rate), (an.moves.len(), an.clicks.len(), an.keys.len(), 1, 8, 4, 250));
    assert_eq!(ng, 2, "before focus, and 9.0-9.5");
    let pad = |n: usize| (n + 3) / 4 * 4;
    let len = 40 + nm as usize * 12 + pad(nm as usize) + nc as usize * 12 + pad(nc as usize) + nk as usize * 4 + pad(nk as usize * 2) + ng as usize * 8 + nw as usize * 28 + (hw * hh) as usize * 4;
    assert_eq!(b.len(), len);
}

#[test]
fn settings_defaults() {
    assert_eq!(settings_from(&serde_json::json!({})), (true, 250));
    assert_eq!(settings_from(&serde_json::json!({"record_input": false, "input_rate": "500"})), (false, 500));
    assert_eq!(settings_from(&serde_json::json!({"input_rate": 125})), (true, 125));
    assert_eq!(settings_from(&serde_json::json!({"input_rate": "999"})), (true, 250));
}

#[test]
fn client_units_at_150_percent() {
    // 4K monitor at 150%: everything is physical pixels; a window at (100, 50).
    let c = Rect { x: 100, y: 50, w: 3840, h: 2160 };
    assert_eq!(to_client_units(100 + 1920, 50 + 1080, c), (32768, 32768));
    assert_eq!(to_client_units(100, 50, c), (0, 0));
    assert_eq!(to_client_units(99, 50, c).0, -17);
}

#[test]
fn size_targets_for_a_35_minute_game() {
    // Worst case: the cursor moving the whole time at 250 Hz, 3 clicks and 2 keys a second.
    let mut r = vec![Record::Window { t: 0, info: win(3840, 2160) }, Record::Focus { t: 0, focused: true }];
    let secs = 35 * 60;
    let mut seed = 12345u64;
    let mut rnd = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (seed >> 33) as f64 / (1u64 << 31) as f64
    };
    let (mut x, mut y) = (0.5, 0.5);
    for i in 0..secs as i64 * 250 {
        x = (x + (rnd() - 0.5) * 0.01).clamp(0.0, 1.0);
        y = (y + (rnd() - 0.5) * 0.01).clamp(0.0, 1.0);
        r.push(Record::Cursor { t: i * 4000, x: u(x), y: u(y) });
        if i % 83 == 0 {
            r.push(Record::Button { t: i * 4000 + 1000, button: BTN_RIGHT, down: true });
            r.push(Record::Button { t: i * 4000 + 61000, button: BTN_RIGHT, down: false });
        }
        if i % 125 == 0 {
            r.push(Record::Key { t: i * 4000 + 2000, vk: b'Q', down: true });
            r.push(Record::Key { t: i * 4000 + 90000, vk: b'Q', down: false });
        }
    }
    let p = tmp("size");
    write_file(&p, 250, &Meta::default(), &r).unwrap();
    let raw = std::fs::metadata(&p).unwrap().len();
    let an = Analysis::from_file(&read(&p).unwrap());
    let t = std::time::Instant::now();
    let m = mechanics_whole(&an, 0.0);
    let stats_ms = t.elapsed().as_millis();
    let h = heatmap(&an, 0.0, an.end, HEAT_W, HEAT_H);
    let (_, comp) = compress_in_place(&p, Some(&h)).unwrap();
    eprintln!("35 min worst case: raw {:.2} MB, compressed {:.2} MB, stats {stats_ms} ms, apm {}", raw as f64 / 1e6, comp as f64 / 1e6, m.apm);
    assert!(raw < 10_000_000, "raw {raw}");
    assert!(comp < 3_000_000, "compressed {comp}");
}
