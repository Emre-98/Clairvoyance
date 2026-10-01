//! Mouse and keyboard input recorded during a game, for the replay overlay (cursor trail,
//! clicks, keys, heatmap) and the post-game "Mechanics" stats.
//!
//! Game-agnostic: a game module only says whether its game is played with the cursor
//! ([`crate::GameIntegration::input_tracking`]). The Windows capture lives in `cv-capture`
//! (cursor polling + Raw Input, never a hook); keys come through the engine so the game's chat
//! state can be respected (no keys are recorded while the chat is open, key codes only).
//!
//! Timestamps are the recording's own video clock: QPC (100 ns) minus the recorder's clock base,
//! the same base its video frames use, so a sample at `t` belongs to the frame shown at `t`.
//!
//! ## File format (`<recording id>.input`, next to the video)
//! ```text
//! header  "CVINPUT\0" | u16 version | u16 flags (bit 0: compressed) | u32 cursor rate (Hz)
//! blocks  u8 kind | u32 payload length | u32 CRC-32 of the payload | payload
//!   kind 1  records (written every 10 s during the game; a torn last block is ignored)
//!   kind 2  records, deflate-compressed (after the game)
//!   kind 3  heatmap of the whole game (u16 w, u16 h, w*h u16 values)
//!   kind 4  metadata (JSON)
//! ```
//! Records (varints; times are zigzag deltas in microseconds from the previous record):
//! `0 time(abs)`, `1 move(dx, dy)`, `2 pos(x, y)`, `3/4 key down/up(vk)`, `5/6 button
//! down/up(b)`, `7 wheel(delta)`, `8 window(client x,y,w,h, frame x,y,w,h, dpi)`,
//! `9 focus(0/1)`, `10 chat(0/1)`, `11 end`. Positions are relative to the game window's client
//! area in units of 1/65536 of its width/height (0..65536 = inside). Every block starts with an
//! absolute time and position, so each block decodes on its own.

pub mod stats;

use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

pub const MAGIC: &[u8; 8] = b"CVINPUT\0";
pub const VERSION: u16 = 1;
pub const EXT: &str = "input";
/// Units per client width/height.
pub const UNIT: f64 = 65536.0;
/// Cursor sample rates offered in the settings.
pub const RATES: [u32; 3] = [125, 250, 500];
pub const DEFAULT_RATE: u32 = 250;
/// Raw records are written to disk at least this often (a crash loses only the last seconds).
pub const FLUSH_SECS: f64 = 10.0;

const FLAG_COMPRESSED: u16 = 1;
const K_RECORDS: u8 = 1;
const K_DEFLATE: u8 = 2;
const K_HEATMAP: u8 = 3;
const K_META: u8 = 4;

const T_TIME: u8 = 0;
const T_MOVE: u8 = 1;
const T_POS: u8 = 2;
const T_KEY_DOWN: u8 = 3;
const T_KEY_UP: u8 = 4;
const T_BTN_DOWN: u8 = 5;
const T_BTN_UP: u8 = 6;
const T_WHEEL: u8 = 7;
const T_WINDOW: u8 = 8;
const T_FOCUS: u8 = 9;
const T_CHAT: u8 = 10;
const T_END: u8 = 11;

/// Mouse buttons (as recorded).
pub const BTN_LEFT: u8 = 1;
pub const BTN_RIGHT: u8 = 2;
pub const BTN_MIDDLE: u8 = 3;
pub const BTN_X1: u8 = 4;
pub const BTN_X2: u8 = 5;

/// Screen rectangle in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// Where the game window is: its client area (what the cursor is relative to) and the window's
/// frame (what window capture records), in physical screen pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct WindowInfo {
    pub client: Rect,
    pub frame: Rect,
    /// DPI of the window's monitor (96 = 100%).
    pub dpi: u32,
}

/// One input record with its time (video clock, microseconds).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Record {
    /// Cursor position (client units, see [`UNIT`]).
    Cursor { t: i64, x: i32, y: i32 },
    Key { t: i64, vk: u8, down: bool },
    Button { t: i64, button: u8, down: bool },
    Wheel { t: i64, delta: i32 },
    Window { t: i64, info: WindowInfo },
    Focus { t: i64, focused: bool },
    Chat { t: i64, open: bool },
    End { t: i64 },
}

impl Record {
    pub fn t(&self) -> i64 {
        match *self {
            Record::Cursor { t, .. }
            | Record::Key { t, .. }
            | Record::Button { t, .. }
            | Record::Wheel { t, .. }
            | Record::Window { t, .. }
            | Record::Focus { t, .. }
            | Record::Chat { t, .. }
            | Record::End { t } => t,
        }
    }
}

// ---- varints -------------------------------------------------------------------------------

fn put_u(buf: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        buf.push((v as u8) | 0x80);
        v >>= 7;
    }
    buf.push(v as u8);
}

fn put_i(buf: &mut Vec<u8>, v: i64) {
    put_u(buf, ((v << 1) ^ (v >> 63)) as u64);
}

struct Rd<'a> {
    b: &'a [u8],
    i: usize,
}

impl Rd<'_> {
    fn u(&mut self) -> Option<u64> {
        let mut v = 0u64;
        let mut shift = 0;
        loop {
            let byte = *self.b.get(self.i)?;
            self.i += 1;
            v |= ((byte & 0x7f) as u64) << shift;
            if byte & 0x80 == 0 {
                return Some(v);
            }
            shift += 7;
            if shift > 63 {
                return None;
            }
        }
    }
    fn i(&mut self) -> Option<i64> {
        let u = self.u()?;
        Some(((u >> 1) as i64) ^ -((u & 1) as i64))
    }
    fn byte(&mut self) -> Option<u8> {
        let b = *self.b.get(self.i)?;
        self.i += 1;
        Some(b)
    }
}

// ---- encoding ------------------------------------------------------------------------------

/// Encodes records into a block payload (delta times and positions).
#[derive(Default)]
struct Encoder {
    buf: Vec<u8>,
    last_t: Option<i64>,
    last_pos: Option<(i32, i32)>,
}

impl Encoder {
    fn time(&mut self, t: i64) {
        match self.last_t {
            None => {
                self.buf.push(T_TIME);
                put_i(&mut self.buf, t);
                put_i(&mut self.buf, 0);
            }
            Some(p) => put_i(&mut self.buf, t - p),
        }
        self.last_t = Some(t);
    }
    fn tag(&mut self, tag: u8, t: i64) {
        if self.last_t.is_none() {
            self.time(t);
        }
        self.buf.push(tag);
        self.time(t);
    }
    fn push(&mut self, r: &Record) {
        match *r {
            Record::Cursor { t, x, y } => match self.last_pos {
                Some((px, py)) => {
                    self.tag(T_MOVE, t);
                    put_i(&mut self.buf, (x - px) as i64);
                    put_i(&mut self.buf, (y - py) as i64);
                    self.last_pos = Some((x, y));
                }
                None => {
                    self.tag(T_POS, t);
                    put_i(&mut self.buf, x as i64);
                    put_i(&mut self.buf, y as i64);
                    self.last_pos = Some((x, y));
                }
            },
            Record::Key { t, vk, down } => {
                self.tag(if down { T_KEY_DOWN } else { T_KEY_UP }, t);
                self.buf.push(vk);
            }
            Record::Button { t, button, down } => {
                self.tag(if down { T_BTN_DOWN } else { T_BTN_UP }, t);
                self.buf.push(button);
            }
            Record::Wheel { t, delta } => {
                self.tag(T_WHEEL, t);
                put_i(&mut self.buf, delta as i64);
            }
            Record::Window { t, info } => {
                self.tag(T_WINDOW, t);
                for v in [info.client.x, info.client.y, info.client.w, info.client.h, info.frame.x, info.frame.y, info.frame.w, info.frame.h] {
                    put_i(&mut self.buf, v as i64);
                }
                put_u(&mut self.buf, info.dpi as u64);
            }
            Record::Focus { t, focused } => {
                self.tag(T_FOCUS, t);
                self.buf.push(focused as u8);
            }
            Record::Chat { t, open } => {
                self.tag(T_CHAT, t);
                self.buf.push(open as u8);
            }
            Record::End { t } => self.tag(T_END, t),
        }
    }
    /// Starts a new block: the next record carries an absolute time and position.
    fn take(&mut self) -> Vec<u8> {
        self.last_t = None;
        self.last_pos = None;
        std::mem::take(&mut self.buf)
    }
}

/// Decodes a records payload (one or several blocks' worth, concatenated).
fn decode_records(b: &[u8], out: &mut Vec<Record>) -> Option<()> {
    let mut r = Rd { b, i: 0 };
    let mut t: i64 = 0;
    let mut pos = (0i32, 0i32);
    while r.i < b.len() {
        let tag = r.byte()?;
        if tag == T_TIME {
            t = r.i()?;
            let _ = r.i()?; // reserved
            continue;
        }
        t += r.i()?;
        let rec = match tag {
            T_MOVE => {
                pos = (pos.0 + r.i()? as i32, pos.1 + r.i()? as i32);
                Record::Cursor { t, x: pos.0, y: pos.1 }
            }
            T_POS => {
                pos = (r.i()? as i32, r.i()? as i32);
                Record::Cursor { t, x: pos.0, y: pos.1 }
            }
            T_KEY_DOWN | T_KEY_UP => Record::Key { t, vk: r.byte()?, down: tag == T_KEY_DOWN },
            T_BTN_DOWN | T_BTN_UP => Record::Button { t, button: r.byte()?, down: tag == T_BTN_DOWN },
            T_WHEEL => Record::Wheel { t, delta: r.i()? as i32 },
            T_WINDOW => {
                let mut v = [0i32; 8];
                for x in v.iter_mut() {
                    *x = r.i()? as i32;
                }
                let dpi = r.u()? as u32;
                Record::Window {
                    t,
                    info: WindowInfo { client: Rect { x: v[0], y: v[1], w: v[2], h: v[3] }, frame: Rect { x: v[4], y: v[5], w: v[6], h: v[7] }, dpi },
                }
            }
            T_FOCUS => Record::Focus { t, focused: r.byte()? != 0 },
            T_CHAT => Record::Chat { t, open: r.byte()? != 0 },
            T_END => Record::End { t },
            _ => return None,
        };
        out.push(rec);
    }
    Some(())
}

fn header(rate: u32, compressed: bool) -> [u8; 16] {
    let mut h = [0u8; 16];
    h[..8].copy_from_slice(MAGIC);
    h[8..10].copy_from_slice(&VERSION.to_le_bytes());
    h[10..12].copy_from_slice(&(if compressed { FLAG_COMPRESSED } else { 0 }).to_le_bytes());
    h[12..16].copy_from_slice(&rate.to_le_bytes());
    h
}

fn block(kind: u8, payload: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(payload.len() + 9);
    v.push(kind);
    v.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    v.extend_from_slice(&crc32fast::hash(payload).to_le_bytes());
    v.extend_from_slice(payload);
    v
}

// ---- writer (during the game) -------------------------------------------------------------

/// Metadata kept in the file (kind 4).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Meta {
    pub game_id: String,
    pub rate: u32,
    #[serde(default)]
    pub app_version: String,
}

struct WriterInner {
    file: Option<File>,
    enc: Encoder,
    /// Video-clock time of the last flush (µs).
    last_flush: i64,
    bytes: u64,
    records: u64,
    /// Mouse-button presses (time µs, button), for checks after the game (League: ult binds).
    button_downs: Vec<(i64, u8)>,
    chat_open: bool,
    /// Keys whose press was recorded (a release is only recorded after its press).
    held: [bool; 256],
    ended: bool,
    error_logged: bool,
}

/// Collects input records during a game and appends them to the file every [`FLUSH_SECS`].
/// Shared between the cursor thread (cursor, buttons, window, focus) and the engine (keys).
pub struct InputWriter {
    path: PathBuf,
    /// Recorder clock base: QPC in 100 ns units at video time 0.
    clock_base: i64,
    rate: u32,
    focused: AtomicBool,
    inner: Mutex<WriterInner>,
}

impl InputWriter {
    pub fn create(path: &Path, clock_base_hns: i64, rate: u32, meta: &Meta) -> std::io::Result<InputWriter> {
        let mut f = File::create(path)?;
        f.write_all(&header(rate, false))?;
        f.write_all(&block(K_META, &serde_json::to_vec(meta).unwrap_or_default()))?;
        f.flush()?;
        Ok(InputWriter {
            path: path.to_path_buf(),
            clock_base: clock_base_hns,
            rate,
            focused: AtomicBool::new(false),
            inner: Mutex::new(WriterInner {
                file: Some(f),
                enc: Encoder::default(),
                last_flush: 0,
                bytes: 0,
                records: 0,
                button_downs: Vec::new(),
                chat_open: false,
                held: [false; 256],
                ended: false,
                error_logged: false,
            }),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn rate(&self) -> u32 {
        self.rate
    }
    pub fn clock_base(&self) -> i64 {
        self.clock_base
    }

    /// Video-clock time (µs) of a QPC timestamp in 100 ns units.
    pub fn video_us(&self, qpc_hns: i64) -> i64 {
        (qpc_hns - self.clock_base) / 10
    }

    /// The game window is focused (set by the cursor thread; keys are only kept while it is).
    pub fn is_focused(&self) -> bool {
        self.focused.load(Ordering::Relaxed)
    }

    pub fn chat_open(&self) -> bool {
        self.inner.lock().unwrap().chat_open
    }

    pub fn push(&self, r: Record) {
        let mut g = self.inner.lock().unwrap();
        if g.ended {
            return;
        }
        match r {
            Record::Focus { focused, .. } => self.focused.store(focused, Ordering::Relaxed),
            Record::Chat { open, .. } => {
                if g.chat_open == open {
                    return;
                }
                g.chat_open = open;
            }
            Record::Button { t, button, down: true } => g.button_downs.push((t, button)),
            Record::Key { vk, down, .. } => {
                let was = std::mem::replace(&mut g.held[vk as usize], down);
                if !down && !was {
                    return;
                }
            }
            _ => {}
        }
        g.enc.push(&r);
        g.records += 1;
        if r.t() - g.last_flush >= (FLUSH_SECS * 1e6) as i64 {
            g.last_flush = r.t();
            Self::flush_locked(&mut g);
        }
    }

    /// Writes what's buffered (also called by the cursor thread every [`FLUSH_SECS`]).
    pub fn flush_if_due(&self, now_us: i64) {
        let mut g = self.inner.lock().unwrap();
        if now_us - g.last_flush >= (FLUSH_SECS * 1e6) as i64 {
            g.last_flush = now_us;
            Self::flush_locked(&mut g);
        }
    }

    fn flush_locked(g: &mut WriterInner) {
        if g.enc.buf.is_empty() {
            return;
        }
        let payload = g.enc.take();
        let b = block(K_RECORDS, &payload);
        let res = match g.file.as_mut() {
            Some(f) => f.write_all(&b).and_then(|_| f.flush()),
            None => Ok(()),
        };
        match res {
            Ok(()) => g.bytes += b.len() as u64,
            Err(e) => {
                if !g.error_logged {
                    log::warn!("input file: {e}");
                    g.error_logged = true;
                }
            }
        }
    }

    /// Ends the recording: last records written, file closed. Returns (file bytes, records).
    pub fn finish(&self, t_us: i64) -> (u64, u64) {
        let mut g = self.inner.lock().unwrap();
        if !g.ended {
            g.enc.push(&Record::End { t: t_us });
            g.ended = true;
            Self::flush_locked(&mut g);
            g.file = None;
        }
        self.focused.store(false, Ordering::Relaxed);
        (g.bytes + 16, g.records)
    }

    /// Bytes buffered in memory right now (for the RAM check).
    pub fn buffered(&self) -> usize {
        let g = self.inner.lock().unwrap();
        g.enc.buf.capacity() + g.button_downs.capacity() * 16
    }

    /// Mouse-button presses so far: (video seconds, button).
    pub fn button_downs(&self) -> Vec<(f64, u8)> {
        self.inner.lock().unwrap().button_downs.iter().map(|&(t, b)| (t as f64 / 1e6, b)).collect()
    }
}

/// What the platform needs to record the cursor and mouse for a game.
#[derive(Clone)]
pub struct CaptureRequest {
    pub writer: std::sync::Arc<InputWriter>,
    /// Lower-case executable names of the game (its window is the one tracked).
    pub process_names: Vec<String>,
}

/// What capturing cost, measured by the capture thread itself (for the log and the reports).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CaptureStats {
    pub wall_secs: f64,
    /// CPU time of the capture thread.
    pub cpu_ms: f64,
    pub ticks: u64,
    pub cursor_samples: u64,
    /// Raw Input mouse messages read (movement included).
    pub raw_mouse_msgs: u64,
    pub buttons: u64,
}

impl CaptureStats {
    /// Share of one CPU core (percent).
    pub fn core_percent(&self) -> f64 {
        if self.wall_secs > 0.0 { self.cpu_ms / 10.0 / self.wall_secs } else { 0.0 }
    }
}

// ---- reading -------------------------------------------------------------------------------

/// A decoded input file.
#[derive(Debug, Clone, Default)]
pub struct InputFile {
    pub version: u16,
    pub compressed: bool,
    pub rate: u32,
    pub meta: Option<Meta>,
    /// All records in file order (times in µs of video clock).
    pub records: Vec<Record>,
    /// Precomputed heatmap of the whole game (compressed files).
    pub heatmap: Option<Heatmap>,
    /// The file ended with a damaged block (crash while writing): everything before it is here.
    pub truncated: bool,
}

/// Cursor dwell heatmap over the client area (row-major, values 0..1).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Heatmap {
    pub w: u32,
    pub h: u32,
    pub values: Vec<f32>,
}

pub fn read(path: &Path) -> anyhow::Result<InputFile> {
    let mut b = Vec::new();
    File::open(path)?.read_to_end(&mut b)?;
    parse(&b)
}

pub fn parse(b: &[u8]) -> anyhow::Result<InputFile> {
    if b.len() < 16 || &b[..8] != MAGIC {
        anyhow::bail!("not an input file");
    }
    let version = u16::from_le_bytes([b[8], b[9]]);
    if version > VERSION {
        anyhow::bail!("input file version {version} is newer than this app");
    }
    let flags = u16::from_le_bytes([b[10], b[11]]);
    let rate = u32::from_le_bytes([b[12], b[13], b[14], b[15]]);
    let mut f = InputFile { version, compressed: flags & FLAG_COMPRESSED != 0, rate, ..Default::default() };
    let mut i = 16;
    while i < b.len() {
        if i + 9 > b.len() {
            f.truncated = true;
            break;
        }
        let kind = b[i];
        let len = u32::from_le_bytes(b[i + 1..i + 5].try_into().unwrap()) as usize;
        let crc = u32::from_le_bytes(b[i + 5..i + 9].try_into().unwrap());
        let Some(payload) = b.get(i + 9..i + 9 + len) else {
            f.truncated = true;
            break;
        };
        if crc32fast::hash(payload) != crc {
            f.truncated = true;
            break;
        }
        i += 9 + len;
        match kind {
            K_RECORDS => {
                let n = f.records.len();
                if decode_records(payload, &mut f.records).is_none() {
                    f.records.truncate(n);
                    f.truncated = true;
                }
            }
            K_DEFLATE => {
                let raw = miniz_oxide::inflate::decompress_to_vec(payload).map_err(|e| anyhow::anyhow!("input file: bad compressed block ({e:?})"))?;
                if decode_records(&raw, &mut f.records).is_none() {
                    anyhow::bail!("input file: damaged records");
                }
            }
            K_HEATMAP if payload.len() >= 4 => {
                let w = u16::from_le_bytes([payload[0], payload[1]]) as u32;
                let h = u16::from_le_bytes([payload[2], payload[3]]) as u32;
                let vals: Vec<f32> = payload[4..].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]]) as f32 / 65535.0).collect();
                if vals.len() == (w * h) as usize {
                    f.heatmap = Some(Heatmap { w, h, values: vals });
                }
            }
            K_META => f.meta = serde_json::from_slice(payload).ok(),
            _ => {} // newer block kinds: skipped
        }
    }
    Ok(f)
}

/// Rewrites a finished (raw) input file compressed, with the whole-game heatmap. Written next to
/// it first and checked, then swapped in. Returns (bytes before, bytes after).
pub fn compress_in_place(path: &Path, heatmap: Option<&Heatmap>) -> anyhow::Result<(u64, u64)> {
    let before = std::fs::metadata(path)?.len();
    let f = read(path)?;
    let mut raw = Vec::new();
    {
        let mut enc = Encoder::default();
        let mut n = 0usize;
        for r in &f.records {
            enc.push(r);
            n += 1;
            // Keep blocks self-contained (absolute time + position) every ~1M records.
            if n % 1_000_000 == 0 {
                raw.push(enc.take());
            }
        }
        raw.push(enc.take());
    }
    let mut out = Vec::with_capacity(before as usize / 2);
    out.extend_from_slice(&header(f.rate, true));
    if let Some(m) = &f.meta {
        out.extend_from_slice(&block(K_META, &serde_json::to_vec(m)?));
    }
    for r in raw.iter().filter(|r| !r.is_empty()) {
        out.extend_from_slice(&block(K_DEFLATE, &miniz_oxide::deflate::compress_to_vec(r, 9)));
    }
    if let Some(h) = heatmap {
        let mut p = Vec::with_capacity(4 + h.values.len() * 2);
        p.extend_from_slice(&(h.w as u16).to_le_bytes());
        p.extend_from_slice(&(h.h as u16).to_le_bytes());
        for v in &h.values {
            p.extend_from_slice(&((v.clamp(0.0, 1.0) * 65535.0).round() as u16).to_le_bytes());
        }
        out.extend_from_slice(&block(K_HEATMAP, &p));
    }
    // Check before replacing: same records.
    let check = parse(&out)?;
    if check.records != f.records {
        anyhow::bail!("compressed input file doesn't match the original");
    }
    let tmp = path.with_extension("input.tmp");
    std::fs::write(&tmp, &out)?;
    std::fs::rename(&tmp, path)?;
    Ok((before, out.len() as u64))
}

/// Writes a complete file at once (tests and tools).
pub fn write_file(path: &Path, rate: u32, meta: &Meta, records: &[Record]) -> std::io::Result<()> {
    let mut out = Vec::new();
    out.extend_from_slice(&header(rate, false));
    out.extend_from_slice(&block(K_META, &serde_json::to_vec(meta).unwrap_or_default()));
    let mut enc = Encoder::default();
    for r in records {
        enc.push(r);
    }
    out.extend_from_slice(&block(K_RECORDS, &enc.take()));
    std::fs::write(path, out)
}

/// The settings a game's input recording uses (Settings > Games > <game>), from the game's
/// config: `record_input` (default on) and `input_rate` ("125" / "250" / "500").
pub fn settings_from(cfg: &serde_json::Value) -> (bool, u32) {
    let on = cfg.get("record_input").and_then(|v| v.as_bool()).unwrap_or(true);
    let rate = cfg
        .get("input_rate")
        .and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())))
        .map(|r| r as u32)
        .filter(|r| RATES.contains(r))
        .unwrap_or(DEFAULT_RATE);
    (on, rate)
}

/// The settings every cursor-based game gets (Settings > Games > <game>), added by the core.
pub fn config_fields() -> Vec<crate::game::ConfigField> {
    vec![
        crate::game::ConfigField {
            key: "record_input",
            label: "Record mouse & keyboard input",
            kind: "bool",
            help: "For the replay's input overlay (cursor trail, clicks, keys, heatmap) and the Mechanics stats. Only while the game window is focused, never while the chat is open, key codes only; stays on this PC. Nothing is shown during the game.",
            options: &[],
        },
        crate::game::ConfigField {
            key: "input_rate",
            label: "Cursor sample rate",
            kind: "select",
            help: "How often the cursor position is read. 250 Hz is smooth at 60 fps; 500 Hz for very fast flicks (bigger files).",
            options: &[("125", "125 Hz"), ("250", "250 Hz (default)"), ("500", "500 Hz")],
        },
    ]
}

/// Defaults of [`config_fields`].
pub fn default_config() -> serde_json::Value {
    serde_json::json!({ "record_input": true, "input_rate": DEFAULT_RATE.to_string() })
}

/// Converts a screen position (physical pixels) to client units.
pub fn to_client_units(px: i32, py: i32, client: Rect) -> (i32, i32) {
    let w = client.w.max(1) as f64;
    let h = client.h.max(1) as f64;
    (((px - client.x) as f64 * UNIT / w).round() as i32, ((py - client.y) as f64 * UNIT / h).round() as i32)
}

/// Virtual-key names for the keys strip and reports (key codes only, never text).
pub fn vk_name(vk: u8) -> String {
    match vk {
        0x41..=0x5A | 0x30..=0x39 => (vk as char).to_string(),
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        0x60..=0x69 => format!("Num{}", vk - 0x60),
        0x08 => "Backspace".into(),
        0x09 => "Tab".into(),
        0x0D => "Enter".into(),
        0x10 | 0xA0 | 0xA1 => "Shift".into(),
        0x11 | 0xA2 | 0xA3 => "Ctrl".into(),
        0x12 | 0xA4 | 0xA5 => "Alt".into(),
        0x14 => "Caps".into(),
        0x1B => "Esc".into(),
        0x20 => "Space".into(),
        0x25 => "Left".into(),
        0x26 => "Up".into(),
        0x27 => "Right".into(),
        0x28 => "Down".into(),
        0xC0 => "`".into(),
        other => format!("Key{other}"),
    }
}

#[cfg(test)]
mod tests;
