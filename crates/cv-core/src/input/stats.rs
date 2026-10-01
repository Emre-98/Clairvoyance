//! What the input file says about a game: the "Mechanics" stats (APM, right-click rate,
//! cursor distance, path efficiency, idle time), the cursor heatmap, and the compact binary
//! form the replay overlay draws from. Computed after the game (maintenance pass) and cached in
//! `session.json`; a time range is computed on demand.

use super::{InputFile, Record, WindowInfo, BTN_RIGHT, UNIT};
use serde::{Deserialize, Serialize};

/// Bump when the numbers change: cached stats of older versions are recomputed.
pub const STATS_VERSION: u32 = 1;
pub const HEAT_W: u32 = 96;
pub const HEAT_H: u32 = 54;
/// No input for longer than this counts as idle.
pub const IDLE_SECS: f64 = 1.0;
/// Clicks further apart than this aren't compared for path efficiency.
const EFFICIENCY_MAX_GAP: f64 = 3.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Move {
    pub t: f64,
    pub x: f32,
    pub y: f32,
    /// First sample after a gap (focus lost, window moved/resized): the trail breaks here.
    pub brk: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Click {
    pub t: f64,
    pub button: u8,
    pub down: bool,
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KeyEv {
    pub t: f64,
    pub vk: u8,
    pub down: bool,
}

/// The records of a file as time series (seconds of video clock, positions 0..1 of the client).
#[derive(Debug, Clone, Default)]
pub struct Analysis {
    pub moves: Vec<Move>,
    pub clicks: Vec<Click>,
    pub keys: Vec<KeyEv>,
    pub wheel: Vec<(f64, i32)>,
    /// Focused intervals (start, end).
    pub focus: Vec<(f64, f64)>,
    pub windows: Vec<(f64, WindowInfo)>,
    /// Chat open intervals.
    pub chat: Vec<(f64, f64)>,
    /// Last record time.
    pub end: f64,
}

impl Analysis {
    pub fn from_file(f: &InputFile) -> Analysis {
        Self::from_records(&f.records)
    }

    pub fn from_records(records: &[Record]) -> Analysis {
        let mut recs = records.to_vec();
        // Two threads write records (cursor thread, engine): order by time.
        recs.sort_by_key(|r| r.t());
        let mut a = Analysis::default();
        let mut focused_since: Option<f64> = None;
        let mut chat_since: Option<f64> = None;
        let mut pos = (0.5f32, 0.5f32);
        let mut brk = true;
        let s = |t: i64| t as f64 / 1e6;
        for r in &recs {
            let t = s(r.t());
            a.end = a.end.max(t);
            match *r {
                Record::Cursor { x, y, .. } => {
                    pos = ((x as f64 / UNIT) as f32, (y as f64 / UNIT) as f32);
                    a.moves.push(Move { t, x: pos.0, y: pos.1, brk });
                    brk = false;
                }
                Record::Button { button, down, .. } => a.clicks.push(Click { t, button, down, x: pos.0, y: pos.1 }),
                Record::Key { vk, down, .. } => a.keys.push(KeyEv { t, vk, down }),
                Record::Wheel { delta, .. } => a.wheel.push((t, delta)),
                Record::Window { info, .. } => {
                    a.windows.push((t, info));
                    brk = true;
                }
                Record::Focus { focused, .. } => {
                    if focused {
                        focused_since.get_or_insert(t);
                    } else if let Some(s0) = focused_since.take() {
                        a.focus.push((s0, t));
                    }
                    brk = true;
                }
                Record::Chat { open, .. } => {
                    if open {
                        chat_since.get_or_insert(t);
                    } else if let Some(s0) = chat_since.take() {
                        a.chat.push((s0, t));
                    }
                }
                Record::End { .. } => {}
            }
        }
        if let Some(s0) = focused_since {
            a.focus.push((s0, a.end));
        }
        if let Some(s0) = chat_since {
            a.chat.push((s0, a.end));
        }
        a
    }

    /// Client aspect (height / width) at time `t` (for distances in screen widths).
    fn aspect_at(&self, t: f64) -> f64 {
        let w = self.windows.iter().rev().find(|(wt, _)| *wt <= t).or(self.windows.first()).map(|(_, w)| w.client);
        match w {
            Some(c) if c.w > 0 && c.h > 0 => c.h as f64 / c.w as f64,
            _ => 9.0 / 16.0,
        }
    }

    /// Focused seconds inside [a, b].
    pub fn focused_secs(&self, a: f64, b: f64) -> f64 {
        self.focus.iter().map(|&(s, e)| (e.min(b) - s.max(a)).max(0.0)).sum()
    }

    fn is_focused(&self, t: f64) -> bool {
        self.focus.iter().any(|&(s, e)| t >= s && t <= e)
    }
}

/// The "Mechanics" numbers of a game or a time range.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Mechanics {
    pub version: u32,
    /// Video seconds covered.
    pub from: f64,
    pub to: f64,
    /// Seconds the game window was focused (the denominator for the rates).
    pub focused_secs: f64,
    pub clicks: u32,
    pub right_clicks: u32,
    pub key_presses: u32,
    /// Actions (clicks + key presses, chat excluded) per minute.
    pub apm: f64,
    /// APM per minute of game clock (minute 0 = 0:00-0:59); null = not enough focused time.
    #[serde(default)]
    pub apm_per_min: Vec<Option<f64>>,
    /// Right-clicks per second.
    pub right_click_hz: f64,
    /// Cursor travel in screen widths.
    pub cursor_distance: f64,
    /// Straight line between consecutive clicks ÷ the cursor's actual path (0..1).
    #[serde(default)]
    pub path_efficiency: Option<f64>,
    /// Time with no input for more than a second (focused time only).
    pub idle_secs: f64,
    /// Samples behind the numbers (for the report).
    #[serde(default)]
    pub cursor_samples: u32,
}

/// Stats for [a, b] (video seconds); `offset` = video position of game time 0:00 (for the
/// per-minute chart).
pub fn mechanics(an: &Analysis, a: f64, b: f64, offset: f64) -> Mechanics {
    let inr = |t: f64| t >= a && t <= b;
    let focused = an.focused_secs(a, b);
    let downs: Vec<&Click> = an.clicks.iter().filter(|c| c.down && inr(c.t)).collect();
    let keys: Vec<&KeyEv> = an.keys.iter().filter(|k| k.down && inr(k.t)).collect();
    let clicks = downs.len() as u32;
    let right = downs.iter().filter(|c| c.button == BTN_RIGHT).count() as u32;
    let key_presses = keys.len() as u32;
    let per_min = |n: f64, secs: f64| if secs > 0.5 { n / secs * 60.0 } else { 0.0 };

    // Per game minute.
    let mut apm_per_min = Vec::new();
    let g0 = (a - offset).max(0.0);
    let g1 = b - offset;
    if g1 > g0 {
        let first = (g0 / 60.0).floor() as usize;
        let last = ((g1 - 1e-9) / 60.0).floor().max(0.0) as usize;
        for m in 0..=last {
            if m < first {
                apm_per_min.push(None);
                continue;
            }
            let (ma, mb) = ((m as f64 * 60.0 + offset).max(a), ((m + 1) as f64 * 60.0 + offset).min(b));
            let f = an.focused_secs(ma, mb);
            let n = downs.iter().filter(|c| c.t >= ma && c.t < mb).count() + keys.iter().filter(|k| k.t >= ma && k.t < mb).count();
            apm_per_min.push((f >= 10.0).then(|| (per_min(n as f64, f) * 10.0).round() / 10.0));
        }
    }

    // Cursor distance and path efficiency.
    let moves: Vec<&Move> = an.moves.iter().filter(|m| inr(m.t)).collect();
    let mut dist = 0.0;
    // Cumulative path length at each move, for efficiency between clicks.
    let mut cum: Vec<f64> = Vec::with_capacity(moves.len());
    for (i, m) in moves.iter().enumerate() {
        if i > 0 && !m.brk {
            let p = moves[i - 1];
            let asp = an.aspect_at(m.t);
            let (dx, dy) = ((m.x - p.x) as f64, (m.y - p.y) as f64 * asp);
            dist += (dx * dx + dy * dy).sqrt();
        }
        cum.push(dist);
    }
    let path_at = |t: f64| -> f64 {
        // Path length up to time t (last move at or before t).
        match moves.partition_point(|m| m.t <= t) {
            0 => 0.0,
            i => cum[i - 1],
        }
    };
    // Breaks so far at each move (prefix count), to see if two clicks are on one stroke.
    let mut brk_cum: Vec<u32> = Vec::with_capacity(moves.len());
    let mut nb = 0;
    for m in &moves {
        nb += m.brk as u32;
        brk_cum.push(nb);
    }
    let brks_at = |t: f64| match moves.partition_point(|m| m.t <= t) {
        0 => 0,
        i => brk_cum[i - 1],
    };
    let breaks_between = |t0: f64, t1: f64| brks_at(t1) > brks_at(t0);
    let mut straight = 0.0;
    let mut actual = 0.0;
    let pairs: Vec<&&Click> = downs.iter().filter(|c| c.button == super::BTN_LEFT || c.button == BTN_RIGHT).collect();
    for w in pairs.windows(2) {
        let (c0, c1) = (w[0], w[1]);
        if c1.t - c0.t > EFFICIENCY_MAX_GAP || breaks_between(c0.t, c1.t) {
            continue;
        }
        let asp = an.aspect_at(c1.t);
        let (dx, dy) = ((c1.x - c0.x) as f64, (c1.y - c0.y) as f64 * asp);
        let s = (dx * dx + dy * dy).sqrt();
        let p = path_at(c1.t) - path_at(c0.t);
        if p > 0.005 {
            straight += s.min(p);
            actual += p;
        }
    }

    // Idle: gaps without input (> 1 s) inside focused time.
    let mut times: Vec<f64> = moves.iter().map(|m| m.t).chain(downs.iter().map(|c| c.t)).chain(keys.iter().map(|k| k.t)).chain(an.wheel.iter().map(|w| w.0).filter(|t| inr(*t))).collect();
    times.sort_by(|x, y| x.total_cmp(y));
    let mut idle = 0.0;
    for &(fs, fe) in &an.focus {
        let (s, e) = (fs.max(a), fe.min(b));
        if e <= s {
            continue;
        }
        let mut prev = s;
        let start = times.partition_point(|&t| t < s);
        for &t in times[start..].iter().take_while(|&&t| t <= e) {
            if t - prev > IDLE_SECS {
                idle += t - prev;
            }
            prev = t;
        }
        if e - prev > IDLE_SECS {
            idle += e - prev;
        }
    }

    Mechanics {
        version: STATS_VERSION,
        from: a,
        to: b,
        focused_secs: round(focused, 1),
        clicks,
        right_clicks: right,
        key_presses,
        apm: round(per_min((clicks + key_presses) as f64, focused), 1),
        apm_per_min,
        right_click_hz: if focused > 0.5 { round(right as f64 / focused, 2) } else { 0.0 },
        cursor_distance: round(dist, 1),
        path_efficiency: (actual > 0.0).then(|| round(straight / actual, 3)),
        idle_secs: round(idle, 1),
        cursor_samples: moves.len() as u32,
    }
}

fn round(v: f64, d: i32) -> f64 {
    let p = 10f64.powi(d);
    (v * p).round() / p
}

/// Whole game: from the game clock's 0:00 (or the first record) to the end of the recording.
pub fn mechanics_whole(an: &Analysis, offset: f64) -> Mechanics {
    mechanics(an, offset.max(0.0), an.end, offset)
}

/// Cursor dwell heatmap for [a, b]: time spent in each cell (focused only), normalized to 0..1.
pub fn heatmap(an: &Analysis, a: f64, b: f64, w: u32, h: u32) -> super::Heatmap {
    let mut v = vec![0f32; (w * h) as usize];
    let ms = &an.moves;
    let start = ms.partition_point(|m| m.t < a);
    for i in start..ms.len() {
        let m = ms[i];
        if m.t > b {
            break;
        }
        let next = ms.get(i + 1).filter(|n| !n.brk).map(|n| n.t).unwrap_or(m.t + 0.05).min(b);
        let dwell = (next - m.t).clamp(0.0, 0.5);
        if dwell <= 0.0 || !(0.0..1.0).contains(&m.x) || !(0.0..1.0).contains(&m.y) || !an.is_focused(m.t) {
            continue;
        }
        let cx = ((m.x * w as f32) as u32).min(w - 1);
        let cy = ((m.y * h as f32) as u32).min(h - 1);
        v[(cy * w + cx) as usize] += dwell as f32;
    }
    let max = v.iter().cloned().fold(0.0f32, f32::max);
    if max > 0.0 {
        for x in v.iter_mut() {
            *x /= max;
        }
    }
    super::Heatmap { w, h, values: v }
}

/// The replay overlay's data: flat little-endian arrays the UI maps straight into typed
/// arrays (times = video seconds as f32, positions 0..1 of the game's client area).
///
/// ```text
/// "CVIV" u32 version
/// u32 moves, clicks, keys, gaps, windows, heat_w, heat_h, rate
/// moves:   t[f32] x[f32] y[f32] brk[u8, padded to 4]
/// clicks:  t[f32] x[f32] y[f32] code[u8: button | 0x80 if down, padded]
/// keys:    t[f32] code[u16: vk | 0x8000 if down, padded]
/// gaps:    start[f32] end[f32]           (not focused)
/// windows: t[f32] ox oy sx sy fw fh[f32] (client inside the captured frame, frame size px)
/// heat:    [f32; w*h]
/// ```
pub fn ui_payload(an: &Analysis, heat: Option<&super::Heatmap>, rate: u32) -> Vec<u8> {
    let mut gaps = Vec::new();
    let mut prev = 0.0;
    for &(s, e) in &an.focus {
        if s > prev {
            gaps.push((prev, s));
        }
        prev = e;
    }
    if an.end > prev {
        gaps.push((prev, f64::MAX));
    }
    let empty = super::Heatmap::default();
    let heat = heat.unwrap_or(&empty);
    let n = an.moves.len();
    let mut b: Vec<u8> = Vec::with_capacity(64 + n * 13 + an.clicks.len() * 13 + heat.values.len() * 4);
    let u32le = |b: &mut Vec<u8>, v: u32| b.extend_from_slice(&v.to_le_bytes());
    let f32le = |b: &mut Vec<u8>, v: f64| b.extend_from_slice(&(v as f32).to_le_bytes());
    let pad = |b: &mut Vec<u8>| {
        while b.len() % 4 != 0 {
            b.push(0)
        }
    };
    b.extend_from_slice(b"CVIV");
    u32le(&mut b, 1);
    for v in [n, an.clicks.len(), an.keys.len(), gaps.len(), an.windows.len(), heat.w as usize, heat.h as usize] {
        u32le(&mut b, v as u32);
    }
    u32le(&mut b, rate);
    for m in &an.moves {
        f32le(&mut b, m.t);
    }
    for m in &an.moves {
        f32le(&mut b, m.x as f64);
    }
    for m in &an.moves {
        f32le(&mut b, m.y as f64);
    }
    b.extend(an.moves.iter().map(|m| m.brk as u8));
    pad(&mut b);
    for c in &an.clicks {
        f32le(&mut b, c.t);
    }
    for c in &an.clicks {
        f32le(&mut b, c.x as f64);
    }
    for c in &an.clicks {
        f32le(&mut b, c.y as f64);
    }
    b.extend(an.clicks.iter().map(|c| c.button | if c.down { 0x80 } else { 0 }));
    pad(&mut b);
    for k in &an.keys {
        f32le(&mut b, k.t);
    }
    for k in &an.keys {
        b.extend_from_slice(&(k.vk as u16 | if k.down { 0x8000 } else { 0 }).to_le_bytes());
    }
    pad(&mut b);
    for g in &gaps {
        f32le(&mut b, g.0);
        f32le(&mut b, g.1.min(1e9));
    }
    for (t, w) in &an.windows {
        let (f, c) = (w.frame, w.client);
        let (fw, fh) = if f.w > 0 && f.h > 0 { (f.w as f64, f.h as f64) } else { (c.w.max(1) as f64, c.h.max(1) as f64) };
        let (ox, oy) = if f.w > 0 { ((c.x - f.x) as f64 / fw, (c.y - f.y) as f64 / fh) } else { (0.0, 0.0) };
        for v in [*t, ox, oy, c.w as f64 / fw, c.h as f64 / fh, fw, fh] {
            f32le(&mut b, v);
        }
    }
    for v in &heat.values {
        b.extend_from_slice(&v.to_le_bytes());
    }
    b
}

/// Overview of a file for the test report (no positions or keys).
#[derive(Debug, Clone, Serialize)]
pub struct FileSummary {
    pub bytes: u64,
    pub compressed: bool,
    pub truncated: bool,
    pub rate: u32,
    pub records: usize,
    pub cursor_samples: usize,
    pub clicks: usize,
    pub key_events: usize,
    pub window_changes: usize,
    pub focus_changes: usize,
    pub duration_secs: f64,
}

pub fn summarize(f: &InputFile, bytes: u64) -> FileSummary {
    let an = Analysis::from_file(f);
    FileSummary {
        bytes,
        compressed: f.compressed,
        truncated: f.truncated,
        rate: f.rate,
        records: f.records.len(),
        cursor_samples: an.moves.len(),
        clicks: an.clicks.iter().filter(|c| c.down).count(),
        key_events: an.keys.len(),
        window_changes: an.windows.len(),
        focus_changes: f.records.iter().filter(|r| matches!(r, Record::Focus { .. })).count(),
        duration_secs: an.end,
    }
}
