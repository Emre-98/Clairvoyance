//! Finalizing recordings for playback: turns the crash-safe fragmented MP4 written during the
//! game into a "faststart" MP4 (one index at the front, then the media), without re-encoding.
//!
//! Why: a fragmented file has no index up front. To learn the duration and where frames are,
//! a player has to walk every `moof` in the file (one per second of video, i.e. ~2,000 reads
//! spread over a 3 GB file for a 35 min game) before the first frame can show. A faststart
//! file needs one read of the `moov` at the start.
//!
//! The same code also moves a `moov` that sits at the end of a regular MP4 to the front.
//! Timestamps are kept exactly: a track whose first sample starts after 0 gets that gap added
//! to its first sample's duration, so every later sample keeps its original time.
//!
//! Pure Rust, no platform code; tested with ffmpeg in `tests/remux_ffmpeg.rs`.

use std::fs::File;
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

/// How an MP4 file is laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Layout {
    /// `moov` (with the full index) before `mdat`: ready for instant playback.
    Faststart,
    /// `moov` without samples + `moof`/`mdat` fragments (what the recorder writes).
    Fragmented,
    /// Regular MP4 with the index at the end (e.g. some ffmpeg outputs).
    MoovAtEnd,
    Unknown,
}

impl Layout {
    pub fn needs_finalize(self) -> bool {
        matches!(self, Layout::Fragmented | Layout::MoovAtEnd)
    }
}

/// What a video file looks like inside (for logs, the test report and the replay benchmark).
#[derive(Debug, Clone, serde::Serialize)]
pub struct VideoInfo {
    pub layout: Layout,
    pub bytes: u64,
    pub duration_secs: f64,
    /// `moof` boxes a player must read before it knows the whole file (0 for faststart).
    pub fragments: usize,
    pub moov_bytes: u64,
    /// e.g. "avc1.640028" (H.264 High, level 4.0).
    pub codec: Option<String>,
    pub width: u32,
    pub height: u32,
    pub video_frames: usize,
    pub keyframes: usize,
    pub keyframe_interval_avg: f64,
    pub keyframe_interval_max: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct FinalizeReport {
    pub from: Layout,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub fragments: usize,
    pub samples: usize,
    pub duration_secs: f64,
}

// ---------- reading boxes ----------

#[derive(Debug, Clone, Copy)]
struct Hdr {
    kind: [u8; 4],
    start: u64,
    header: u64,
    size: u64,
}

impl Hdr {
    fn end(&self) -> u64 {
        self.start + self.size
    }
    fn body(&self) -> u64 {
        self.start + self.header
    }
}

/// Reads the box header at `pos`. `None` at the end of the file or for a box cut off by a crash.
fn read_hdr(f: &mut File, pos: u64, len: u64) -> io::Result<Option<Hdr>> {
    if pos + 8 > len {
        return Ok(None);
    }
    f.seek(SeekFrom::Start(pos))?;
    let mut b = [0u8; 16];
    f.read_exact(&mut b[..8])?;
    let mut size = u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as u64;
    let kind = [b[4], b[5], b[6], b[7]];
    let mut header = 8;
    if size == 1 {
        if pos + 16 > len {
            return Ok(None);
        }
        f.read_exact(&mut b[8..16])?;
        size = u64::from_be_bytes(b[8..16].try_into().unwrap());
        header = 16;
    } else if size == 0 {
        size = len - pos;
    }
    if size < header {
        return Ok(None);
    }
    Ok(Some(Hdr { kind, start: pos, header, size }))
}

/// An in-memory box: `kind`, the whole box bytes and where its body starts.
#[derive(Clone)]
struct Bx<'a> {
    kind: [u8; 4],
    whole: &'a [u8],
    body: &'a [u8],
}

fn boxes(data: &[u8]) -> Vec<Bx<'_>> {
    let mut out = Vec::new();
    let mut p = 0usize;
    while p + 8 <= data.len() {
        let mut size = u32::from_be_bytes(data[p..p + 4].try_into().unwrap()) as usize;
        let kind: [u8; 4] = data[p + 4..p + 8].try_into().unwrap();
        let mut hdr = 8;
        if size == 1 {
            if p + 16 > data.len() {
                break;
            }
            size = u64::from_be_bytes(data[p + 8..p + 16].try_into().unwrap()) as usize;
            hdr = 16;
        } else if size == 0 {
            size = data.len() - p;
        }
        if size < hdr || p + size > data.len() {
            break;
        }
        out.push(Bx { kind, whole: &data[p..p + size], body: &data[p + hdr..p + size] });
        p += size;
    }
    out
}

fn child<'a>(data: &'a [u8], kind: &[u8; 4]) -> Option<Bx<'a>> {
    boxes(data).into_iter().find(|b| &b.kind == kind)
}

fn find_path<'a>(data: &'a [u8], kinds: &[&[u8; 4]]) -> Option<Bx<'a>> {
    let mut cur: Option<Bx<'a>> = None;
    let mut d = data;
    for k in kinds {
        let b = child(d, k)?;
        d = b.body;
        cur = Some(b);
    }
    cur
}

fn be32(b: &[u8], at: usize) -> u32 {
    b.get(at..at + 4).map(|s| u32::from_be_bytes(s.try_into().unwrap())).unwrap_or(0)
}
fn be64(b: &[u8], at: usize) -> u64 {
    b.get(at..at + 8).map(|s| u64::from_be_bytes(s.try_into().unwrap())).unwrap_or(0)
}

/// Small cursor over a full box body (after version/flags).
struct Rd<'a> {
    b: &'a [u8],
    p: usize,
}
impl<'a> Rd<'a> {
    fn u32(&mut self) -> io::Result<u32> {
        let v = self.b.get(self.p..self.p + 4).ok_or_else(|| bad("box too short"))?;
        self.p += 4;
        Ok(u32::from_be_bytes(v.try_into().unwrap()))
    }
    fn u64(&mut self) -> io::Result<u64> {
        let v = self.b.get(self.p..self.p + 8).ok_or_else(|| bad("box too short"))?;
        self.p += 8;
        Ok(u64::from_be_bytes(v.try_into().unwrap()))
    }
}

fn bad(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_string())
}

// ---------- the sample model ----------

#[derive(Debug, Clone, Copy)]
struct Sample {
    #[cfg_attr(not(test), allow(dead_code))]
    off: u64,
    size: u32,
    dur: u32,
    sync: bool,
    cto: i32,
}

/// A run of samples of one track stored back to back in the source file.
#[derive(Debug, Clone, Copy)]
struct Chunk {
    track: usize,
    /// Index of the source `moof` (fragmented input), else 0.
    frag: usize,
    src: u64,
    first: usize,
    count: usize,
    bytes: u64,
}

struct Track {
    id: u32,
    trak: Vec<u8>,
    timescale: u32,
    video: bool,
    samples: Vec<Sample>,
    /// Decode time of the first sample (fragmented input: its `tfdt`).
    first_dts: u64,
    next_dts: u64,
    trex: (u32, u32, u32), // default duration, size, flags
}

struct Parsed {
    layout: Layout,
    len: u64,
    mvhd: Vec<u8>,
    /// Other `moov` children kept as they are (udta, meta, ...).
    extra: Vec<Vec<u8>>,
    tracks: Vec<Track>,
    chunks: Vec<Chunk>,
    fragments: usize,
    moov_bytes: u64,
    /// Every complete top-level box, in file order (for the in-place index).
    tops: Vec<Hdr>,
    /// The `moov` box used.
    moov_hdr: Hdr,
}

fn read_box(f: &mut File, h: &Hdr) -> io::Result<Vec<u8>> {
    if h.size > 256 << 20 {
        return Err(bad("index box too large"));
    }
    let mut v = vec![0u8; h.size as usize];
    f.seek(SeekFrom::Start(h.start))?;
    f.read_exact(&mut v)?;
    Ok(v)
}

/// Quick check of the layout from the first few top-level boxes (a few small reads).
pub fn probe(path: &Path) -> io::Result<Layout> {
    let mut f = File::open(path)?;
    let len = f.metadata()?.len();
    let mut pos = 0;
    let mut seen_moov: Option<Hdr> = None;
    for _ in 0..8 {
        let Some(h) = read_hdr(&mut f, pos, len)? else { break };
        match &h.kind {
            b"moov" => seen_moov = Some(h),
            b"moof" => return Ok(if seen_moov.is_some() { Layout::Fragmented } else { Layout::Unknown }),
            b"mdat" => {
                if let Some(m) = seen_moov {
                    // A moov with an `mvex` box announces fragments, even if the first one is an mdat.
                    // (Walks the moov's child headers only, without reading its tables.)
                    let mut c = m.body();
                    while let Some(ch) = read_hdr(&mut f, c, m.end())? {
                        if &ch.kind == b"mvex" {
                            return Ok(Layout::Fragmented);
                        }
                        c = ch.end();
                    }
                    return Ok(Layout::Faststart);
                }
                // mdat first: is there a moov after it?
                let next = read_hdr(&mut f, h.end(), len)?;
                return Ok(match next.map(|n| n.kind) {
                    Some(k) if &k == b"moov" => Layout::MoovAtEnd,
                    Some(k) if &k == b"moof" => Layout::Fragmented,
                    _ => Layout::Unknown,
                });
            }
            _ => {}
        }
        pos = h.end();
    }
    Ok(Layout::Unknown)
}

fn parse(path: &Path) -> io::Result<Parsed> {
    let mut f = File::open(path)?;
    let len = f.metadata()?.len();
    let mut pos = 0;
    let mut moov: Option<(Vec<u8>, Hdr)> = None;
    let mut moofs: Vec<Hdr> = Vec::new();
    let mut mdat_first = false;
    let mut tops: Vec<Hdr> = Vec::new();
    loop {
        let Some(h) = read_hdr(&mut f, pos, len)? else { break };
        if h.end() > len {
            // Cut off by a crash: whatever is complete before it is kept.
            if &h.kind == b"moof" {
                break;
            }
            if &h.kind != b"mdat" {
                break;
            }
        } else {
            tops.push(h);
        }
        match &h.kind {
            // The first `moov` counts (a player ignores a second one too).
            b"moov" if moov.is_none() => moov = Some((read_box(&mut f, &h)?, h)),
            b"moof" => moofs.push(h),
            b"mdat" if moov.is_none() => mdat_first = true,
            _ => {}
        }
        if h.end() >= len {
            break;
        }
        pos = h.end();
    }
    if moov.is_none() {
        // A write torn by a power cut in the middle of `index_in_place` step 2 can leave the
        // index in a box still marked `free`: use the first one that is really a moov.
        for h in tops.iter().filter(|h| &h.kind == b"free" && h.size > 24 && h.size < 256 << 20) {
            let b = read_box(&mut f, h)?;
            if b.get(h.header as usize + 4..h.header as usize + 8) == Some(b"mvhd") {
                moov = Some((b, *h));
                break;
            }
        }
    }
    let (moov, mh) = moov.ok_or_else(|| bad("no moov box (not an MP4 recording?)"))?;
    let mb = &moov[mh.header as usize..];
    let mvhd = child(mb, b"mvhd").ok_or_else(|| bad("no mvhd"))?.whole.to_vec();
    let fragmented = child(mb, b"mvex").is_some();
    let mut trex = std::collections::HashMap::new();
    if let Some(mvex) = child(mb, b"mvex") {
        for t in boxes(mvex.body).into_iter().filter(|b| &b.kind == b"trex") {
            let b = t.body;
            trex.insert(be32(b, 4), (be32(b, 12), be32(b, 16), be32(b, 20)));
        }
    }
    let mut tracks = Vec::new();
    let mut extra = Vec::new();
    for b in boxes(mb) {
        match &b.kind {
            b"trak" => {
                let tkhd = child(b.body, b"tkhd").ok_or_else(|| bad("no tkhd"))?;
                let id = if tkhd.body[0] == 1 { be32(tkhd.body, 20) } else { be32(tkhd.body, 12) };
                let mdhd = find_path(b.body, &[b"mdia", b"mdhd"]).ok_or_else(|| bad("no mdhd"))?;
                let timescale = if mdhd.body[0] == 1 { be32(mdhd.body, 20) } else { be32(mdhd.body, 12) };
                let hdlr = find_path(b.body, &[b"mdia", b"hdlr"]).ok_or_else(|| bad("no hdlr"))?;
                let video = &hdlr.body[8..12] == b"vide";
                tracks.push(Track {
                    id,
                    trak: b.whole.to_vec(),
                    timescale: timescale.max(1),
                    video,
                    samples: Vec::new(),
                    first_dts: 0,
                    next_dts: 0,
                    trex: trex.get(&id).copied().unwrap_or((0, 0, 0)),
                });
            }
            b"mvhd" | b"mvex" => {}
            _ => extra.push(b.whole.to_vec()),
        }
    }
    if tracks.is_empty() {
        return Err(bad("no tracks"));
    }
    let mut chunks = Vec::new();
    // Samples already in the moov (regular files, or fragmented files that carry some).
    for ti in 0..tracks.len() {
        let stbl = find_path(&tracks[ti].trak[8..], &[b"mdia", b"minf", b"stbl"]).map(|b| b.body.to_vec());
        if let Some(stbl) = stbl {
            read_tables(&stbl, ti, &mut tracks[ti], &mut chunks)?;
        }
    }
    let layout = if fragmented {
        Layout::Fragmented
    } else if mdat_first {
        Layout::MoovAtEnd
    } else {
        Layout::Faststart
    };
    // Fragments only count for a fragmented `moov`.
    let fragments = if fragmented { moofs.len() } else { 0 };
    if fragmented {
        for (i, h) in moofs.iter().enumerate() {
            let moof = read_box(&mut f, h)?;
            read_moof(i + 1, &moof[h.header as usize..], h.start, &mut tracks, &mut chunks, len)?;
        }
    }
    chunks.sort_by_key(|c| c.src);
    Ok(Parsed { layout, len, mvhd, extra, tracks, chunks, fragments, moov_bytes: mh.size, tops, moov_hdr: mh })
}

/// Reads the sample tables of a regular `stbl`.
fn read_tables(stbl: &[u8], ti: usize, t: &mut Track, chunks: &mut Vec<Chunk>) -> io::Result<()> {
    let Some(stsz) = child(stbl, b"stsz") else { return Ok(()) };
    let fixed = be32(stsz.body, 4);
    let n = be32(stsz.body, 8) as usize;
    if n == 0 {
        return Ok(());
    }
    let mut sizes = Vec::with_capacity(n);
    for i in 0..n {
        sizes.push(if fixed != 0 { fixed } else { be32(stsz.body, 12 + 4 * i) });
    }
    let mut durs = Vec::with_capacity(n);
    if let Some(stts) = child(stbl, b"stts") {
        let e = be32(stts.body, 4) as usize;
        for i in 0..e {
            let (c, d) = (be32(stts.body, 8 + 8 * i), be32(stts.body, 12 + 8 * i));
            for _ in 0..c {
                durs.push(d);
            }
        }
    }
    durs.resize(n, *durs.last().unwrap_or(&1));
    let mut cto = vec![0i32; n];
    if let Some(ctts) = child(stbl, b"ctts") {
        let e = be32(ctts.body, 4) as usize;
        let mut k = 0;
        for i in 0..e {
            let (c, o) = (be32(ctts.body, 8 + 8 * i), be32(ctts.body, 12 + 8 * i) as i32);
            for _ in 0..c {
                if k < n {
                    cto[k] = o;
                    k += 1;
                }
            }
        }
    }
    let sync: Option<Vec<bool>> = child(stbl, b"stss").map(|s| {
        let mut v = vec![false; n];
        for i in 0..be32(s.body, 4) as usize {
            let k = be32(s.body, 8 + 4 * i) as usize;
            if k >= 1 && k <= n {
                v[k - 1] = true;
            }
        }
        v
    });
    let mut offs: Vec<u64> = Vec::new();
    if let Some(stco) = child(stbl, b"stco") {
        for i in 0..be32(stco.body, 4) as usize {
            offs.push(be32(stco.body, 8 + 4 * i) as u64);
        }
    } else if let Some(co64) = child(stbl, b"co64") {
        for i in 0..be32(co64.body, 4) as usize {
            offs.push(be64(co64.body, 8 + 8 * i));
        }
    }
    let mut stsc: Vec<(u32, u32)> = Vec::new();
    if let Some(s) = child(stbl, b"stsc") {
        for i in 0..be32(s.body, 4) as usize {
            stsc.push((be32(s.body, 8 + 12 * i), be32(s.body, 12 + 12 * i)));
        }
    }
    let mut k = 0usize;
    for (ci, &off) in offs.iter().enumerate() {
        let chunk_no = ci as u32 + 1;
        let per = stsc.iter().rev().find(|(first, _)| *first <= chunk_no).map(|e| e.1).unwrap_or(1) as usize;
        let first = t.samples.len();
        let mut o = off;
        for _ in 0..per {
            if k >= n {
                break;
            }
            t.samples.push(Sample { off: o, size: sizes[k], dur: durs[k], sync: sync.as_ref().map(|s| s[k]).unwrap_or(true), cto: cto[k] });
            o += sizes[k] as u64;
            k += 1;
        }
        let count = t.samples.len() - first;
        if count > 0 {
            chunks.push(Chunk { track: ti, frag: 0, src: off, first, count, bytes: o - off });
        }
    }
    t.next_dts = t.samples.iter().map(|s| s.dur as u64).sum();
    Ok(())
}

fn read_moof(frag: usize, moof: &[u8], moof_start: u64, tracks: &mut [Track], chunks: &mut Vec<Chunk>, file_len: u64) -> io::Result<()> {
    let mut prev_end = moof_start; // implicit base for trafs without an explicit one
    for traf in boxes(moof).into_iter().filter(|b| &b.kind == b"traf") {
        let Some(tfhd) = child(traf.body, b"tfhd") else { continue };
        let tb = tfhd.body;
        let flags = be32(tb, 0) & 0xFF_FFFF;
        let mut r = Rd { b: tb, p: 4 };
        let id = r.u32()?;
        let Some(ti) = tracks.iter().position(|t| t.id == id) else { continue };
        let (mut def_dur, mut def_size, mut def_flags) = tracks[ti].trex;
        let mut base = if flags & 0x2_0000 != 0 { moof_start } else { prev_end };
        if flags & 0x1 != 0 {
            base = r.u64()?;
        }
        if flags & 0x2 != 0 {
            r.u32()?;
        }
        if flags & 0x8 != 0 {
            def_dur = r.u32()?;
        }
        if flags & 0x10 != 0 {
            def_size = r.u32()?;
        }
        if flags & 0x20 != 0 {
            def_flags = r.u32()?;
        }
        if let Some(tfdt) = child(traf.body, b"tfdt") {
            let t = &mut tracks[ti];
            let dts = if tfdt.body[0] == 1 { be64(tfdt.body, 4) } else { be32(tfdt.body, 4) as u64 };
            if t.samples.is_empty() {
                t.first_dts = dts;
                t.next_dts = dts;
            } else if dts > t.next_dts {
                // A gap in the recording: the previous sample lasts until this fragment starts.
                let gap = dts - t.next_dts;
                if let Some(last) = t.samples.last_mut() {
                    last.dur = (last.dur as u64 + gap).min(u32::MAX as u64) as u32;
                }
                t.next_dts = dts;
            }
        }
        let mut data = base;
        for trun in boxes(traf.body).into_iter().filter(|b| &b.kind == b"trun") {
            let b = trun.body;
            let version = b[0];
            let tf = be32(b, 0) & 0xFF_FFFF;
            let mut r = Rd { b, p: 4 };
            let count = r.u32()? as usize;
            if tf & 0x1 != 0 {
                data = (base as i64 + r.u32()? as i32 as i64) as u64;
            }
            let first_flags = if tf & 0x4 != 0 { Some(r.u32()?) } else { None };
            let t = &mut tracks[ti];
            let first = t.samples.len();
            let start = data;
            for i in 0..count {
                let dur = if tf & 0x100 != 0 { r.u32()? } else { def_dur };
                let size = if tf & 0x200 != 0 { r.u32()? } else { def_size };
                let sf = if tf & 0x400 != 0 {
                    r.u32()?
                } else if i == 0 && first_flags.is_some() {
                    first_flags.unwrap()
                } else {
                    def_flags
                };
                let cto = if tf & 0x800 != 0 {
                    let v = r.u32()?;
                    if version == 0 {
                        v.min(i32::MAX as u32) as i32
                    } else {
                        v as i32
                    }
                } else {
                    0
                };
                if data + size as u64 > file_len {
                    break; // the data of a cut-off fragment
                }
                let sync = if t.video { sf & 0x0001_0000 == 0 } else { true };
                t.samples.push(Sample { off: data, size, dur, sync, cto });
                t.next_dts += dur as u64;
                data += size as u64;
            }
            let n = t.samples.len() - first;
            if n > 0 {
                chunks.push(Chunk { track: ti, frag, src: start, first, count: n, bytes: data - start });
            }
        }
        prev_end = data;
    }
    Ok(())
}

// ---------- info ----------

/// Reads a file's layout, codec and keyframe spacing (reads the index only).
pub fn info(path: &Path) -> io::Result<VideoInfo> {
    let p = parse(path)?;
    let mut info = VideoInfo {
        layout: p.layout,
        bytes: p.len,
        duration_secs: 0.0,
        fragments: p.fragments,
        moov_bytes: p.moov_bytes,
        codec: None,
        width: 0,
        height: 0,
        video_frames: 0,
        keyframes: 0,
        keyframe_interval_avg: 0.0,
        keyframe_interval_max: 0.0,
    };
    if let Some(v) = p.tracks.iter().find(|t| t.video) {
        let ts = v.timescale as f64;
        let total: u64 = v.samples.iter().map(|s| s.dur as u64).sum();
        info.duration_secs = (v.first_dts + total) as f64 / ts;
        info.video_frames = v.samples.len();
        let mut t = v.first_dts;
        let mut keys = Vec::new();
        for s in &v.samples {
            if s.sync {
                keys.push(t);
            }
            t += s.dur as u64;
        }
        info.keyframes = keys.len();
        if keys.len() > 1 {
            let gaps: Vec<f64> = keys.windows(2).map(|w| (w[1] - w[0]) as f64 / ts).collect();
            info.keyframe_interval_avg = gaps.iter().sum::<f64>() / gaps.len() as f64;
            info.keyframe_interval_max = gaps.iter().cloned().fold(0.0, f64::max);
        }
        if let Some(stsd) = find_path(&v.trak[8..], &[b"mdia", b"minf", b"stbl", b"stsd"]) {
            // stsd: version/flags, count, then the first sample entry.
            let entry = &stsd.body[8..];
            if entry.len() > 8 + 78 {
                let kind = String::from_utf8_lossy(&entry[4..8]).to_string();
                info.width = u16::from_be_bytes([entry[8 + 24], entry[8 + 25]]) as u32;
                info.height = u16::from_be_bytes([entry[8 + 26], entry[8 + 27]]) as u32;
                info.codec = Some(match child(&entry[8 + 78..], b"avcC") {
                    Some(c) if c.body.len() >= 4 => format!("{kind}.{:02x}{:02x}{:02x}", c.body[1], c.body[2], c.body[3]),
                    _ => kind,
                });
            }
        }
    }
    Ok(info)
}

/// Times (seconds, as the player sees them) of the video's keyframes. A jump to a keyframe
/// shows at once; a jump between two keyframes has to decode every frame from the one before.
pub fn keyframes(path: &Path) -> io::Result<Vec<f64>> {
    let p = parse(path)?;
    let v = p.tracks.iter().find(|t| t.video).ok_or_else(|| bad("no video"))?;
    let ts = v.timescale as f64;
    let mut out = Vec::new();
    let mut t = v.first_dts;
    for (i, s) in v.samples.iter().enumerate() {
        if s.sync {
            // A finalized file starts its first frame at 0 (see the module docs).
            out.push(if i == 0 && p.layout == Layout::Faststart { 0.0 } else { (t as i64 + s.cto as i64) as f64 / ts });
        }
        t += s.dur as u64;
    }
    Ok(out)
}

/// Start times (seconds, as the player sees them) of every video frame, in order: the frame on
/// screen at time `t` is the last one starting at or before `t`. Reads the index only.
pub fn frame_times(path: &Path) -> io::Result<Vec<f64>> {
    let p = parse(path)?;
    let v = p.tracks.iter().find(|t| t.video).ok_or_else(|| bad("no video"))?;
    let ts = v.timescale as f64;
    let mut out = Vec::with_capacity(v.samples.len());
    let mut t = v.first_dts;
    for (i, s) in v.samples.iter().enumerate() {
        // A finalized file starts its first frame at 0 (see the module docs).
        out.push(if i == 0 && p.layout == Layout::Faststart { 0.0 } else { (t as i64 + s.cto as i64) as f64 / ts });
        t += s.dur as u64;
    }
    // Presentation order (no B-frames are written, but stay safe).
    if !out.windows(2).all(|w| w[0] <= w[1]) {
        out.sort_by(|a, b| a.total_cmp(b));
    }
    Ok(out)
}

// ---------- writing ----------

fn put32(v: &mut Vec<u8>, x: u32) {
    v.extend_from_slice(&x.to_be_bytes());
}

fn mk(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(body.len() + 8);
    put32(&mut v, (body.len() + 8) as u32);
    v.extend_from_slice(kind);
    v.extend_from_slice(body);
    v
}

fn mk_full(kind: &[u8; 4], version: u8, flags: u32, body: &[u8]) -> Vec<u8> {
    let mut b = Vec::with_capacity(body.len() + 4);
    b.push(version);
    b.extend_from_slice(&flags.to_be_bytes()[1..]);
    b.extend_from_slice(body);
    mk(kind, &b)
}

/// Sets the duration field of an mvhd / tkhd / mdhd box (version 0 or 1).
fn with_duration(whole: &[u8], kind: &[u8; 4], dur: u64) -> Vec<u8> {
    let mut v = whole.to_vec();
    let hdr = if be32(&v, 0) == 1 { 16 } else { 8 };
    let version = v[hdr];
    let at = hdr
        + 4
        + match (kind, version) {
            (b"mvhd", 1) | (b"mdhd", 1) => 20,
            (b"mvhd", _) | (b"mdhd", _) => 12,
            (b"tkhd", 1) => 24,
            _ => 16, // tkhd v0
        };
    if version == 1 {
        if at + 8 <= v.len() {
            v[at..at + 8].copy_from_slice(&dur.to_be_bytes());
        }
    } else if at + 4 <= v.len() {
        v[at..at + 4].copy_from_slice(&(dur.min(u32::MAX as u64) as u32).to_be_bytes());
    }
    v
}

fn movie_timescale(mvhd: &[u8]) -> u32 {
    let hdr = 8;
    if mvhd[hdr] == 1 { be32(mvhd, hdr + 4 + 16) } else { be32(mvhd, hdr + 4 + 8) }.max(1)
}

/// Durations per sample as written: the first sample also covers any gap before it.
fn out_durations(t: &Track) -> Vec<u32> {
    let mut d: Vec<u32> = t.samples.iter().map(|s| s.dur.max(1)).collect();
    if let Some(f) = d.first_mut() {
        *f = (*f as u64 + t.first_dts).min(u32::MAX as u64) as u32;
    }
    d
}

fn new_stbl(stsd: &[u8], t: &Track, chunks: &[(usize, usize, u64)], co64: bool) -> Vec<u8> {
    let durs = out_durations(t);
    let mut parts = vec![stsd.to_vec()];
    // stts
    let mut runs: Vec<(u32, u32)> = Vec::new();
    for d in &durs {
        match runs.last_mut() {
            Some((n, v)) if v == d => *n += 1,
            _ => runs.push((1, *d)),
        }
    }
    let mut b = Vec::new();
    put32(&mut b, runs.len() as u32);
    for (n, v) in &runs {
        put32(&mut b, *n);
        put32(&mut b, *v);
    }
    parts.push(mk_full(b"stts", 0, 0, &b));
    // ctts (only if needed)
    if t.samples.iter().any(|s| s.cto != 0) {
        let neg = t.samples.iter().any(|s| s.cto < 0);
        let mut runs: Vec<(u32, i32)> = Vec::new();
        for s in &t.samples {
            match runs.last_mut() {
                Some((n, v)) if *v == s.cto => *n += 1,
                _ => runs.push((1, s.cto)),
            }
        }
        let mut b = Vec::new();
        put32(&mut b, runs.len() as u32);
        for (n, v) in &runs {
            put32(&mut b, *n);
            put32(&mut b, *v as u32);
        }
        parts.push(mk_full(b"ctts", if neg { 1 } else { 0 }, 0, &b));
    }
    // stss (video, when not every frame is a keyframe)
    if t.video && t.samples.iter().any(|s| !s.sync) {
        let keys: Vec<u32> = t.samples.iter().enumerate().filter(|(_, s)| s.sync).map(|(i, _)| i as u32 + 1).collect();
        let mut b = Vec::new();
        put32(&mut b, keys.len() as u32);
        for k in keys {
            put32(&mut b, k);
        }
        parts.push(mk_full(b"stss", 0, 0, &b));
    }
    // stsc: samples per chunk, run-length coded
    let mut runs: Vec<(u32, u32)> = Vec::new(); // (first chunk, samples per chunk)
    for (i, (_, n, _)) in chunks.iter().enumerate() {
        if runs.last().map(|r| r.1) != Some(*n as u32) {
            runs.push((i as u32 + 1, *n as u32));
        }
    }
    let mut b = Vec::new();
    put32(&mut b, runs.len() as u32);
    for (f, n) in &runs {
        put32(&mut b, *f);
        put32(&mut b, *n);
        put32(&mut b, 1);
    }
    parts.push(mk_full(b"stsc", 0, 0, &b));
    // stsz
    let mut b = Vec::with_capacity(8 + 4 * t.samples.len());
    put32(&mut b, 0);
    put32(&mut b, t.samples.len() as u32);
    for s in &t.samples {
        put32(&mut b, s.size);
    }
    parts.push(mk_full(b"stsz", 0, 0, &b));
    // stco / co64
    let mut b = Vec::new();
    put32(&mut b, chunks.len() as u32);
    for (_, _, off) in chunks {
        if co64 {
            b.extend_from_slice(&off.to_be_bytes());
        } else {
            put32(&mut b, *off as u32);
        }
    }
    parts.push(mk_full(if co64 { b"co64" } else { b"stco" }, 0, 0, &b));
    mk(b"stbl", &parts.concat())
}

/// Rebuilds one `trak`: new durations and a new `stbl`; everything else is copied.
fn new_trak(t: &Track, movie_ts: u32, chunks: &[(usize, usize, u64)], co64: bool) -> io::Result<Vec<u8>> {
    let media: u64 = out_durations(t).iter().map(|d| *d as u64).sum();
    let movie = media * movie_ts as u64 / t.timescale as u64;
    let body = &t.trak[8..];
    let mut out = Vec::new();
    for b in boxes(body) {
        match &b.kind {
            b"tkhd" => out.push(with_duration(b.whole, b"tkhd", movie)),
            b"edts" => {} // timing is carried by the sample durations (see the module docs)
            b"mdia" => {
                let mut m = Vec::new();
                for c in boxes(b.body) {
                    match &c.kind {
                        b"mdhd" => m.push(with_duration(c.whole, b"mdhd", media)),
                        b"minf" => {
                            let mut n = Vec::new();
                            for d in boxes(c.body) {
                                if &d.kind == b"stbl" {
                                    let stsd = child(d.body, b"stsd").ok_or_else(|| bad("no stsd"))?;
                                    n.push(new_stbl(stsd.whole, t, chunks, co64));
                                } else {
                                    n.push(d.whole.to_vec());
                                }
                            }
                            m.push(mk(b"minf", &n.concat()));
                        }
                        _ => m.push(c.whole.to_vec()),
                    }
                }
                out.push(mk(b"mdia", &m.concat()));
            }
            _ => out.push(b.whole.to_vec()),
        }
    }
    Ok(mk(b"trak", &out.concat()))
}

fn ftyp() -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(b"isom");
    put32(&mut b, 0x200);
    for brand in [b"isom", b"iso2", b"avc1", b"mp41"] {
        b.extend_from_slice(brand);
    }
    mk(b"ftyp", &b)
}

/// Builds the new header (ftyp + moov) for data that starts right after it.
fn build_head(p: &Parsed, co64: bool) -> io::Result<(Vec<u8>, Vec<Vec<(usize, usize, u64)>>, u64)> {
    let movie_ts = movie_timescale(&p.mvhd);
    // Data offsets inside mdat, relative to its first byte.
    let mut rel: Vec<Vec<(usize, usize, u64)>> = vec![Vec::new(); p.tracks.len()];
    let mut o = 0u64;
    for c in &p.chunks {
        rel[c.track].push((c.first, c.count, o));
        o += c.bytes;
    }
    let data_len = o;
    let build = |shift: u64| -> io::Result<Vec<u8>> {
        let mut parts = Vec::new();
        let longest = p.tracks.iter().map(|t| out_durations(t).iter().map(|d| *d as u64).sum::<u64>() * movie_ts as u64 / t.timescale as u64).max().unwrap_or(0);
        parts.push(with_duration(&p.mvhd, b"mvhd", longest));
        for (i, t) in p.tracks.iter().enumerate() {
            if t.samples.is_empty() {
                continue;
            }
            let shifted: Vec<(usize, usize, u64)> = rel[i].iter().map(|(f, n, o)| (*f, *n, o + shift)).collect();
            parts.push(new_trak(t, movie_ts, &shifted, co64)?);
        }
        parts.extend(p.extra.iter().cloned());
        Ok([ftyp(), mk(b"moov", &parts.concat())].concat())
    };
    // The header size doesn't depend on the offset values (fixed-width fields).
    let head_len = build(0)?.len() as u64;
    let mdat_hdr = 16; // 64-bit size: works for any length
    let head = build(head_len + mdat_hdr)?;
    debug_assert_eq!(head.len() as u64, head_len);
    let final_rel = rel.iter().map(|v| v.iter().map(|(f, n, o)| (*f, *n, o + head_len + mdat_hdr)).collect()).collect();
    Ok((head, final_rel, data_len))
}

/// Writes `dst` as a faststart MP4 made from `src` (fragmented or moov-at-end). Copies the media
/// as is. `cancel` is checked between chunks; when it returns true the copy stops with
/// `ErrorKind::Interrupted` (the caller deletes `dst`).
pub fn finalize(src: &Path, dst: &Path, cancel: &dyn Fn() -> bool) -> io::Result<FinalizeReport> {
    let p = parse(src)?;
    write_faststart(&p, src, dst, cancel)
}

fn write_faststart(p: &Parsed, src: &Path, dst: &Path, cancel: &dyn Fn() -> bool) -> io::Result<FinalizeReport> {
    let samples: usize = p.tracks.iter().map(|t| t.samples.len()).sum();
    if p.tracks.iter().find(|t| t.video).map(|t| t.samples.is_empty()).unwrap_or(true) {
        return Err(bad("the recording has no video frames"));
    }
    let data_len: u64 = p.chunks.iter().map(|c| c.bytes).sum();
    let co64 = data_len + (16 << 20) > u32::MAX as u64;
    let (head, _rel, data_len) = build_head(p, co64)?;
    let mut inp = File::open(src)?;
    let mut out = BufWriter::with_capacity(4 << 20, File::create(dst)?);
    out.write_all(&head)?;
    out.write_all(&1u32.to_be_bytes())?;
    out.write_all(b"mdat")?;
    out.write_all(&(data_len + 16).to_be_bytes())?;
    let mut buf = vec![0u8; 4 << 20];
    // Neighbouring chunks are usually back to back in the source: copy them as one range.
    let mut i = 0;
    while i < p.chunks.len() {
        if cancel() {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
        }
        let start = p.chunks[i].src;
        let mut end = start + p.chunks[i].bytes;
        let mut j = i + 1;
        while j < p.chunks.len() && p.chunks[j].src == end && end - start < 64 << 20 {
            end += p.chunks[j].bytes;
            j += 1;
        }
        inp.seek(SeekFrom::Start(start))?;
        let mut left = end - start;
        while left > 0 {
            let n = left.min(buf.len() as u64) as usize;
            inp.read_exact(&mut buf[..n])?;
            out.write_all(&buf[..n])?;
            left -= n as u64;
        }
        i = j;
    }
    let f = out.into_inner().map_err(|e| e.into_error())?;
    f.sync_all()?;
    let bytes_out = f.metadata()?.len();
    drop(f);
    // Check the result before anyone replaces the original with it.
    let check = parse(dst)?;
    let n_out: usize = check.tracks.iter().map(|t| t.samples.len()).sum();
    if check.layout != Layout::Faststart || n_out != samples || bytes_out != head.len() as u64 + 16 + data_len {
        return Err(bad("the finished file didn't check out"));
    }
    let v = p.tracks.iter().find(|t| t.video).unwrap();
    let dur = out_durations(v).iter().map(|d| *d as u64).sum::<u64>() as f64 / v.timescale as f64;
    Ok(FinalizeReport { from: p.layout, bytes_in: p.len, bytes_out, fragments: p.fragments, samples, duration_secs: dur })
}

/// Finalizes `video` in place: writes `<video>.finalizing` next to it, checks it, then replaces
/// the original. On any error (or cancel) the original is left untouched.
pub fn finalize_in_place(video: &Path, cancel: &dyn Fn() -> bool) -> io::Result<FinalizeReport> {
    let tmp = video.with_extension("mp4.finalizing");
    let r = finalize(video, &tmp, cancel);
    match r {
        Ok(rep) => {
            if let Err(e) = replace(&tmp, video) {
                let _ = std::fs::remove_file(&tmp);
                return Err(e);
            }
            Ok(rep)
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

/// What [`index_in_place`] did.
#[derive(Debug, Clone, serde::Serialize)]
pub struct InPlaceReport {
    /// Layout before.
    pub from: Layout,
    /// The full index was written into the reserved room (false: the file was already
    /// faststart, nothing done).
    pub indexed: bool,
    pub moov_bytes: u64,
    /// Room that was reserved for it.
    pub reserve_bytes: u64,
    /// Fragments now inside the one `mdat`.
    pub fragments: usize,
    pub samples: usize,
    pub duration_secs: f64,
    pub ms: u64,
}

/// Turns a recording written by [`crate::mp4::FragmentedWriter`] (fragments + an empty `free`
/// box right after its header) into a faststart MP4 **in place**, without copying the media:
///
/// 1. the full index (`moov` with every sample; chunk offsets point at the media where it
///    already is, inside the fragments) is written into the reserved `free` box, followed by
///    the header of one `mdat` box that runs to the end of the file, all still hidden (the
///    reserved box stays `free`);
/// 2. one small write at the start of the file (within the first 4 KB, one disk sector) turns the
///    fragmented `moov` into `free` and the reserved box into the new `moov`.
///
/// The result is laid out like any faststart MP4: `ftyp`, `free`, `moov`, then one `mdat` with
/// all the media (the old `moof` boxes are just bytes inside it). A player reads the few boxes
/// at the start and is done: it doesn't walk the file (FFmpeg / Chromium walk every top-level
/// box of a file until its end, which is what makes a fragmented recording slow to open).
///
/// Crash-safe: until step 2 the file is the untouched fragmented recording (a second run starts
/// over); step 2 is a single write within one sector; after it the file is final. A recording
/// cut off by a crash is indexed up to the cut (the cut piece is inside the `mdat`).
///
/// Errors with `ErrorKind::Unsupported` when there's no reserved room (recordings made before
/// v1.7.1) or the index doesn't fit (a game of more than ~3 hours): use [`finalize_in_place`].
pub fn index_in_place(path: &Path) -> io::Result<InPlaceReport> {
    index_steps(path, 2)
}

/// [`index_in_place`], stopping after step `last` (tests of power cuts between the steps).
fn index_steps(path: &Path, last: u8) -> io::Result<InPlaceReport> {
    let t0 = std::time::Instant::now();
    let p = parse(path)?;
    let samples: usize = p.tracks.iter().map(|t| t.samples.len()).sum();
    let video = p.tracks.iter().find(|t| t.video).ok_or_else(|| bad("no video track"))?;
    if video.samples.is_empty() {
        return Err(bad("the recording has no video frames"));
    }
    let duration_secs = out_durations(video).iter().map(|d| *d as u64).sum::<u64>() as f64 / video.timescale as f64;
    let mut rep = InPlaceReport { from: p.layout, indexed: false, moov_bytes: 0, reserve_bytes: 0, fragments: p.fragments, samples, duration_secs, ms: 0 };
    let mh = p.moov_hdr;
    let fragmented = p.fragments > 0 || {
        // A fragmented moov without any complete fragment yet.
        let mut f = File::open(path)?;
        let m = read_box(&mut f, &mh)?;
        child(&m[mh.header as usize..], b"mvex").is_some()
    };
    if !fragmented {
        if p.layout == Layout::Faststart {
            rep.ms = t0.elapsed().as_millis() as u64;
            return Ok(rep); // already done
        }
        return Err(io::Error::new(io::ErrorKind::Unsupported, "not a fragmented recording"));
    }
    // The reserved room: the `free` box right after the fragmented moov (`moov` there: a torn
    // step 2 of an earlier run, the first moov still wins: redo).
    let reserve = p.tops.iter().find(|h| h.start == mh.end()).copied().filter(|h| (&h.kind == b"free" || &h.kind == b"moov") && h.header == 8);
    let Some(res) = reserve else {
        return Err(io::Error::new(io::ErrorKind::Unsupported, "no room reserved for the index (recorded before v1.7.1)"));
    };
    rep.reserve_bytes = res.size;
    let co64 = p.len > u32::MAX as u64;
    let movie_ts = movie_timescale(&p.mvhd);
    let longest = p.tracks.iter().map(|t| out_durations(t).iter().map(|d| *d as u64).sum::<u64>() * movie_ts as u64 / t.timescale as u64).max().unwrap_or(0);
    let mut parts = vec![with_duration(&p.mvhd, b"mvhd", longest)];
    for (i, t) in p.tracks.iter().enumerate() {
        if t.samples.is_empty() {
            continue;
        }
        // Chunks stay where they are: absolute offsets into the fragments.
        let chunks: Vec<(usize, usize, u64)> = p.chunks.iter().filter(|c| c.track == i).map(|c| (c.first, c.count, c.src)).collect();
        parts.push(new_trak(t, movie_ts, &chunks, co64)?);
    }
    parts.extend(p.extra.iter().cloned());
    let moov = mk(b"moov", &parts.concat());
    let m = moov.len() as u64;
    // After the moov: one mdat to the end of the file (64-bit size if it needs it).
    let mdat_at = res.start + m;
    let mdat_len = p.len - mdat_at;
    let mut mdat_hdr = Vec::with_capacity(16);
    if mdat_len <= u32::MAX as u64 {
        put32(&mut mdat_hdr, mdat_len as u32);
        mdat_hdr.extend_from_slice(b"mdat");
    } else {
        put32(&mut mdat_hdr, 1);
        mdat_hdr.extend_from_slice(b"mdat");
        mdat_hdr.extend_from_slice(&mdat_len.to_be_bytes());
    }
    if m + mdat_hdr.len() as u64 > res.size {
        return Err(io::Error::new(io::ErrorKind::Unsupported, format!("the index ({m} bytes) doesn't fit the reserved room ({} bytes)", res.size)));
    }
    rep.moov_bytes = m;
    let mut f = std::fs::OpenOptions::new().read(true).write(true).open(path)?;
    // 1. Hidden write of the new index and the mdat header (the box at res.start still says
    //    `free` and covers both).
    f.seek(SeekFrom::Start(res.start + 8))?;
    let mut body = moov[8..].to_vec();
    body.extend_from_slice(&mdat_hdr);
    f.write_all(&body)?;
    f.sync_data()?;
    if last < 2 {
        return Ok(rep);
    }
    // 2. One write from the old moov's header to the end of the new moov's header: the old
    //    one becomes `free`, the reserved box becomes the new `moov`.
    let mut head = read_box(&mut f, &mh)?;
    head[4..8].copy_from_slice(b"free");
    head.extend_from_slice(&moov[..8]);
    f.seek(SeekFrom::Start(mh.start))?;
    f.write_all(&head)?;
    f.sync_data()?;
    drop(f);
    rep.indexed = true;
    // Check the result.
    let check = parse(path)?;
    let n: usize = check.tracks.iter().map(|t| t.samples.len()).sum();
    let kinds: Vec<[u8; 4]> = check.tops.iter().map(|h| h.kind).collect();
    if check.layout != Layout::Faststart || n != samples || kinds.last() != Some(b"mdat") || check.tops.last().map(|h| h.end()) != Some(check.len) {
        return Err(bad(&format!("the indexed file didn't check out ({:?}, {n} of {samples} samples)", check.layout)));
    }
    rep.ms = t0.elapsed().as_millis() as u64;
    Ok(rep)
}

/// The quickest way to make `video` instantly playable: [`index_in_place`] when the recording
/// has room for it, else a copy ([`finalize_in_place`]).
pub fn make_playable(video: &Path, cancel: &dyn Fn() -> bool) -> io::Result<(bool, FinalizeReport)> {
    match index_in_place(video) {
        Ok(r) => Ok((
            true,
            FinalizeReport { from: r.from, bytes_in: 0, bytes_out: std::fs::metadata(video).map(|m| m.len()).unwrap_or(0), fragments: r.fragments, samples: r.samples, duration_secs: r.duration_secs },
        )),
        Err(e) if e.kind() == io::ErrorKind::Unsupported => finalize_in_place(video, cancel).map(|r| (false, r)),
        Err(e) => Err(e),
    }
}

fn replace(from: &Path, to: &Path) -> io::Result<()> {
    // `rename` replaces an existing file on Windows too (MoveFileEx + REPLACE_EXISTING); it fails
    // if the original is open without delete sharing, e.g. in an external player: retry later.
    std::fs::rename(from, to)
}

/// Leftover temp file from a finalize that was interrupted (crash, power cut).
pub fn is_temp_file(p: &Path) -> bool {
    p.to_string_lossy().ends_with(".mp4.finalizing")
}

/// Test tool: copies `secs` seconds starting at `start` (from the keyframe at or before it) into
/// a small faststart MP4, without re-encoding. Returns where the cut really starts (seconds).
#[doc(hidden)]
pub fn cut(src: &Path, dst: &Path, start: f64, secs: f64) -> io::Result<f64> {
    let mut p = parse(src)?;
    let vi = p.tracks.iter().position(|t| t.video).ok_or_else(|| bad("no video"))?;
    let orig: Vec<Vec<Sample>> = p.tracks.iter().map(|t| t.samples.clone()).collect();
    let dts: Vec<Vec<u64>> = p
        .tracks
        .iter()
        .map(|t| {
            let mut d = t.first_dts;
            t.samples
                .iter()
                .map(|s| {
                    let v = d;
                    d += s.dur as u64;
                    v
                })
                .collect()
        })
        .collect();
    let vts = p.tracks[vi].timescale as f64;
    let t0 = orig[vi].iter().zip(&dts[vi]).filter(|(s, d)| s.sync && **d as f64 / vts <= start).map(|(_, d)| *d as f64 / vts).last().unwrap_or(0.0);
    let t1 = start + secs;
    let mut maps: Vec<Vec<Option<usize>>> = Vec::new();
    for (ti, t) in p.tracks.iter_mut().enumerate() {
        let ts = t.timescale as f64;
        let mut map = vec![None; orig[ti].len()];
        let mut kept = Vec::new();
        let mut first = None;
        for (i, s) in orig[ti].iter().enumerate() {
            let at = dts[ti][i] as f64 / ts;
            if at >= t0 - 1e-9 && at < t1 {
                first.get_or_insert(dts[ti][i]);
                map[i] = Some(kept.len());
                kept.push(*s);
            }
        }
        t.first_dts = first.map(|f| (f as f64 - t0 * ts).max(0.0) as u64).unwrap_or(0);
        t.samples = kept;
        maps.push(map);
    }
    let mut chunks = Vec::new();
    for c in &p.chunks {
        let mut run: Option<Chunk> = None;
        let mut off = c.src;
        for i in c.first..c.first + c.count {
            let size = orig[c.track][i].size as u64;
            match maps[c.track][i] {
                Some(n) => match &mut run {
                    Some(r) => {
                        r.count += 1;
                        r.bytes += size;
                    }
                    None => run = Some(Chunk { track: c.track, frag: c.frag, src: off, first: n, count: 1, bytes: size }),
                },
                None => {
                    if let Some(done) = run.take() {
                        chunks.push(done);
                    }
                }
            }
            off += size;
        }
        if let Some(done) = run.take() {
            chunks.push(done);
        }
    }
    p.chunks = chunks;
    write_faststart(&p, src, dst, &|| false)?;
    Ok(t0)
}

/// Test tool (replay benchmark): writes a fragmented recording of `secs` seconds by repeating
/// (or cutting) the fragments of an existing fragmented recording `src`, the way the recorder
/// writes them (same header, one `moof`+`mdat` per source fragment, continuous timestamps).
#[doc(hidden)]
pub fn loop_recording(src: &Path, dst: &Path, secs: f64) -> io::Result<f64> {
    loop_recording_r(src, dst, secs, 0)
}

/// [`loop_recording`] with `reserve` bytes of room for the in-place index after the header, like
/// the recorder writes since v1.7.1 (0 = none).
#[doc(hidden)]
pub fn loop_recording_r(src: &Path, dst: &Path, secs: f64, reserve: u64) -> io::Result<f64> {
    let mut p = parse(src)?;
    let vi = p.tracks.iter().position(|t| t.video).ok_or_else(|| bad("no video"))?;
    if p.layout != Layout::Fragmented {
        // A finalized recording keeps the recorder's runs of samples as chunks, in order (one
        // video run, then the audio runs, per second): a new fragment at every video run.
        let mut frag = 0;
        for c in p.chunks.iter_mut() {
            if c.track == vi {
                frag += 1;
            }
            c.frag = frag.max(1);
        }
    }
    // Decode time of every sample, per track.
    let dts: Vec<Vec<u64>> = p
        .tracks
        .iter()
        .map(|t| {
            let mut d = t.first_dts;
            t.samples
                .iter()
                .map(|s| {
                    let v = d;
                    d += s.dur as u64;
                    v
                })
                .collect()
        })
        .collect();
    let period: Vec<u64> = p
        .tracks
        .iter()
        .enumerate()
        .map(|(i, t)| dts[i].last().copied().unwrap_or(0) + t.samples.last().map(|s| s.dur as u64).unwrap_or(0))
        .collect();
    let period_secs = period[vi] as f64 / p.tracks[vi].timescale as f64;
    // Header: the source's ftyp + moov as they are.
    let mut inp = File::open(src)?;
    let first_moof = p.chunks.iter().filter(|c| c.frag > 0).map(|c| c.src).min().unwrap_or(0);
    let mut head = Vec::new();
    if p.layout == Layout::Fragmented {
        let mut pos = 0;
        while let Some(h) = read_hdr(&mut inp, pos, p.len)? {
            if &h.kind == b"moof" || h.start >= first_moof {
                break;
            }
            if &h.kind == b"ftyp" || &h.kind == b"moov" {
                head.extend(read_box(&mut inp, &h)?);
            }
            pos = h.end();
        }
    } else {
        // The recorder's header for these tracks: ftyp + a moov without samples + mvex.
        head.extend(crate::mp4::fragmented_ftyp());
        let mut parts = vec![with_duration(&p.mvhd, b"mvhd", 0)];
        for t in &p.tracks {
            parts.push(empty_trak(t)?);
        }
        let mut trex = Vec::new();
        for t in &p.tracks {
            let mut b = Vec::new();
            for v in [t.id, 1, 0, 0, 0] {
                put32(&mut b, v);
            }
            trex.push(mk_full(b"trex", 0, 0, &b));
        }
        parts.push(mk(b"mvex", &trex.concat()));
        head.extend(mk(b"moov", &parts.concat()));
    }
    if reserve >= 8 {
        put32(&mut head, reserve as u32);
        head.extend_from_slice(b"free");
        head.resize(head.len() + reserve as usize - 8, 0);
    }
    let mut out = BufWriter::with_capacity(4 << 20, File::create(dst)?);
    out.write_all(&head)?;
    let mut by_frag: std::collections::BTreeMap<usize, Vec<Chunk>> = Default::default();
    for c in &p.chunks {
        by_frag.entry(c.frag).or_default().push(*c);
    }
    let mut seq = 0u32;
    let mut written = 0.0;
    'outer: for k in 0.. {
        for chunks in by_frag.values() {
            let vstart = chunks.iter().find(|c| c.track == vi).map(|c| dts[vi][c.first] + k * period[vi]);
            if let Some(v) = vstart {
                written = v as f64 / p.tracks[vi].timescale as f64;
                if written >= secs {
                    break 'outer;
                }
            }
            seq += 1;
            let mut trafs: Vec<(Vec<u8>, usize)> = Vec::new();
            for c in chunks {
                let t = &p.tracks[c.track];
                let mut tfhd = Vec::new();
                put32(&mut tfhd, t.id);
                let mut tfdt = Vec::new();
                tfdt.extend_from_slice(&(dts[c.track][c.first] + k * period[c.track]).to_be_bytes());
                let mut tr = Vec::new();
                put32(&mut tr, c.count as u32);
                put32(&mut tr, 0);
                for s in &t.samples[c.first..c.first + c.count] {
                    put32(&mut tr, s.dur);
                    put32(&mut tr, s.size);
                    put32(&mut tr, if t.video && !s.sync { 0x0101_0000 } else { 0x0200_0000 });
                }
                let tfhd = mk_full(b"tfhd", 0, 0x02_0000, &tfhd);
                let tfdt = mk_full(b"tfdt", 1, 0, &tfdt);
                let at = 8 + tfhd.len() + tfdt.len() + 16;
                trafs.push((mk(b"traf", &[tfhd, tfdt, mk_full(b"trun", 0, 0x000701, &tr)].concat()), at));
            }
            let mut mfhd = Vec::new();
            put32(&mut mfhd, seq);
            let mfhd = mk_full(b"mfhd", 0, 0, &mfhd);
            let moof_len = 8 + mfhd.len() + trafs.iter().map(|t| t.0.len()).sum::<usize>();
            let mut data_pos = moof_len + 8;
            for ((traf, at), c) in trafs.iter_mut().zip(chunks) {
                traf[*at..*at + 4].copy_from_slice(&(data_pos as u32).to_be_bytes());
                data_pos += c.bytes as usize;
            }
            let moof = mk(b"moof", &[vec![mfhd], trafs.into_iter().map(|t| t.0).collect()].concat().concat());
            out.write_all(&moof)?;
            put32_w(&mut out, (data_pos - moof_len) as u32)?;
            out.write_all(b"mdat")?;
            for c in chunks {
                let mut buf = vec![0u8; c.bytes as usize];
                inp.seek(SeekFrom::Start(c.src))?;
                inp.read_exact(&mut buf)?;
                out.write_all(&buf)?;
            }
        }
        if period_secs <= 0.0 {
            break;
        }
    }
    out.flush()?;
    Ok(written)
}

/// A track's `trak` with empty sample tables (the header of a fragmented recording).
fn empty_trak(t: &Track) -> io::Result<Vec<u8>> {
    let empty = Track { id: t.id, trak: t.trak.clone(), timescale: t.timescale, video: t.video, samples: Vec::new(), first_dts: 0, next_dts: 0, trex: t.trex };
    let body = &t.trak[8..];
    let mut out = Vec::new();
    for b in boxes(body) {
        match &b.kind {
            b"tkhd" => out.push(with_duration(b.whole, b"tkhd", 0)),
            b"edts" => {}
            b"mdia" => {
                let mut m = Vec::new();
                for c in boxes(b.body) {
                    match &c.kind {
                        b"mdhd" => m.push(with_duration(c.whole, b"mdhd", 0)),
                        b"minf" => {
                            let mut n = Vec::new();
                            for d in boxes(c.body) {
                                if &d.kind == b"stbl" {
                                    let stsd = child(d.body, b"stsd").ok_or_else(|| bad("no stsd"))?;
                                    n.push(new_stbl(stsd.whole, &empty, &[], false));
                                } else {
                                    n.push(d.whole.to_vec());
                                }
                            }
                            m.push(mk(b"minf", &n.concat()));
                        }
                        _ => m.push(c.whole.to_vec()),
                    }
                }
                out.push(mk(b"mdia", &m.concat()));
            }
            _ => out.push(b.whole.to_vec()),
        }
    }
    Ok(mk(b"trak", &out.concat()))
}

fn put32_w(w: &mut impl Write, x: u32) -> io::Result<()> {
    w.write_all(&x.to_be_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mp4::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("cvremux-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d.join(name)
    }

    fn write_fragmented(path: &Path, secs: i64, video_delay: i64) -> usize {
        write_fragmented_r(path, secs, video_delay, 0)
    }

    /// Like the recorder: `reserve` bytes of room after the header (0 = pre-v1.7.1 layout).
    fn write_fragmented_r(path: &Path, secs: i64, video_delay: i64, reserve: u64) -> usize {
        let vc = VideoConfig { width: 64, height: 36, sps: vec![0x67, 0x64, 0, 0x1f, 1], pps: vec![0x68, 1], fps: 60 };
        let ac = vec![AudioConfig::aac_lc(48000, 2, "Game audio")];
        let mut w = FragmentedWriter::with_reserve(std::io::BufWriter::new(File::create(path).unwrap()), vc, ac, reserve).unwrap();
        let mut pk: Vec<Packet> = Vec::new();
        for i in 0..secs * 60 {
            pk.push(Packet { track: 0, pts: video_delay + i * HNS / 60, data: vec![(i % 251) as u8; 300 + (i % 7) as usize], key: i % 60 == 0 });
        }
        for i in 0..secs * 48000 / 1024 {
            pk.push(Packet { track: 1, pts: i * 1024 * HNS / 48000, data: vec![7; 20], key: true });
        }
        pk.sort_by_key(|p| p.pts);
        let n = pk.len();
        let mut next = HNS;
        for p in pk {
            if p.pts >= next {
                w.flush_fragment().unwrap();
                next += HNS;
            }
            w.push(p);
        }
        w.finish().unwrap();
        n
    }

    #[test]
    fn fragmented_to_faststart_keeps_every_sample() {
        let src = tmp("a.mp4");
        let n = write_fragmented(&src, 12, 300_000);
        assert_eq!(probe(&src).unwrap(), Layout::Fragmented);
        let before = info(&src).unwrap();
        assert!(before.fragments >= 11, "{before:?}");
        assert!((before.keyframe_interval_avg - 1.0).abs() < 0.01);
        let dst = tmp("a-fast.mp4");
        let rep = finalize(&src, &dst, &|| false).unwrap();
        assert_eq!(rep.samples, n);
        assert_eq!(probe(&dst).unwrap(), Layout::Faststart);
        let after = info(&dst).unwrap();
        assert_eq!(after.fragments, 0);
        assert_eq!(after.video_frames, before.video_frames);
        assert_eq!(after.keyframes, before.keyframes);
        assert!((after.duration_secs - before.duration_secs).abs() < 0.001, "{} vs {}", after.duration_secs, before.duration_secs);
        assert_eq!(after.codec.as_deref(), Some("avc1.64001f"));
        // Keyframes stay at the same times (the first one moves to 0).
        let (ka, kb) = (keyframes(&src).unwrap(), keyframes(&dst).unwrap());
        assert_eq!(ka.len(), kb.len());
        assert!((ka[0] - 0.03).abs() < 1e-6 && kb[0] == 0.0, "{} {}", ka[0], kb[0]);
        for (x, y) in ka.iter().zip(&kb).skip(1) {
            assert!((x - y).abs() < 1e-6);
        }
        // Every frame's start time, same in both layouts (ability bubbles appear on the frame of
        // their key-down): 60 fps from 0.03 s (the first one moves to 0 when finalized).
        let (fa, fb) = (frame_times(&src).unwrap(), frame_times(&dst).unwrap());
        assert_eq!(fa.len(), before.video_frames);
        assert_eq!(fb.len(), fa.len());
        assert!((fa[0] - 0.03).abs() < 1e-6 && fb[0] == 0.0);
        for (i, (x, y)) in fa.iter().zip(&fb).enumerate().skip(1) {
            assert!((x - (0.03 + i as f64 / 60.0)).abs() < 1e-4, "frame {i}: {x}");
            assert!((x - y).abs() < 1e-6);
        }
        // Same media bytes, in the same order.
        let a = parse(&src).unwrap();
        let b = parse(&dst).unwrap();
        let mut fa = File::open(&src).unwrap();
        let mut fb = File::open(&dst).unwrap();
        for (ta, tb) in a.tracks.iter().zip(&b.tracks) {
            assert_eq!(ta.samples.len(), tb.samples.len());
            for (sa, sb) in ta.samples.iter().zip(&tb.samples).step_by(37) {
                let mut x = vec![0; sa.size as usize];
                let mut y = vec![0; sb.size as usize];
                fa.seek(SeekFrom::Start(sa.off)).unwrap();
                fa.read_exact(&mut x).unwrap();
                fb.seek(SeekFrom::Start(sb.off)).unwrap();
                fb.read_exact(&mut y).unwrap();
                assert_eq!(x, y);
                assert_eq!(sa.sync, sb.sync);
            }
        }
        // Faststart again is a no-op layout-wise.
        let dst2 = tmp("a-fast2.mp4");
        finalize(&dst, &dst2, &|| false).unwrap();
        assert_eq!(std::fs::metadata(&dst2).unwrap().len(), std::fs::metadata(&dst).unwrap().len());
    }

    #[test]
    fn crashed_recording_is_finalized_up_to_the_cut() {
        let src = tmp("b.mp4");
        write_fragmented(&src, 10, 0);
        let bytes = std::fs::read(&src).unwrap();
        let cut = tmp("b-cut.mp4");
        std::fs::write(&cut, &bytes[..bytes.len() / 2 + 123]).unwrap();
        let dst = tmp("b-fast.mp4");
        let rep = finalize(&cut, &dst, &|| false).unwrap();
        assert!(rep.duration_secs > 4.0 && rep.duration_secs < 6.0, "{rep:?}");
        assert_eq!(probe(&dst).unwrap(), Layout::Faststart);
    }

    #[test]
    fn loop_tool_makes_longer_and_shorter_recordings() {
        let src = tmp("d.mp4");
        write_fragmented(&src, 5, 0);
        let long = tmp("d-long.mp4");
        loop_recording(&src, &long, 17.0).unwrap();
        let i = info(&long).unwrap();
        assert_eq!(i.layout, Layout::Fragmented);
        assert!((i.duration_secs - 17.0).abs() < 1.1, "{i:?}");
        assert!((i.keyframe_interval_max - 1.0).abs() < 0.01, "{i:?}");
        let short = tmp("d-short.mp4");
        loop_recording(&src, &short, 2.0).unwrap();
        assert!((info(&short).unwrap().duration_secs - 2.0).abs() < 1.1);
        // Cut 3 s from the middle: starts on the keyframe at 4 s, plays from 0.
        let piece = tmp("d-cut.mp4");
        let t0 = cut(&long, &piece, 4.5, 3.0).unwrap();
        assert!((t0 - 4.0).abs() < 0.02, "{t0}");
        let ci = info(&piece).unwrap();
        assert_eq!(ci.layout, Layout::Faststart);
        assert!((ci.duration_secs - 3.5).abs() < 0.1, "{ci:?}");
        assert_eq!(ci.video_frames, 210);
        let fast = tmp("d-long-fast.mp4");
        finalize(&long, &fast, &|| false).unwrap();
        assert_eq!(info(&fast).unwrap().video_frames, i.video_frames);
        // With room for the in-place index (benchmarks of v1.7.1): same media, indexes in place.
        let res = tmp("d-long-res.mp4");
        loop_recording_r(&src, &res, 17.0, 64 << 10).unwrap();
        assert_eq!(info(&res).unwrap().video_frames, i.video_frames);
        assert!(index_in_place(&res).unwrap().indexed);
        assert_eq!(frame_times(&res).unwrap(), frame_times(&fast).unwrap());
        // From a finalized (or in-place indexed) recording: the same fragments again.
        let again = tmp("d-again.mp4");
        loop_recording(&fast, &again, 17.0).unwrap();
        let ia = info(&again).unwrap();
        assert_eq!(ia.layout, Layout::Fragmented);
        assert_eq!(ia.video_frames, i.video_frames);
        assert_eq!(ia.fragments, i.fragments);
        assert_eq!(sample_bytes(&again), sample_bytes(&long));
        let again2 = tmp("d-again2.mp4");
        loop_recording(&res, &again2, 17.0).unwrap();
        assert_eq!(sample_bytes(&again2), sample_bytes(&long));
    }

    /// Top-level box kinds of a file, in order.
    fn top_kinds(path: &Path) -> Vec<String> {
        parse(path).unwrap().tops.iter().map(|h| String::from_utf8_lossy(&h.kind).to_string()).collect()
    }

    /// Every sample's bytes, per track (to compare files sample by sample).
    fn sample_bytes(path: &Path) -> Vec<Vec<Vec<u8>>> {
        let p = parse(path).unwrap();
        let mut f = File::open(path).unwrap();
        p.tracks
            .iter()
            .map(|t| {
                t.samples
                    .iter()
                    .map(|s| {
                        let mut b = vec![0; s.size as usize];
                        f.seek(SeekFrom::Start(s.off)).unwrap();
                        f.read_exact(&mut b).unwrap();
                        b
                    })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn reserve_size() {
        // 60 fps + game audio: ~10.6 MB for 3 hours; bounds.
        let r = reserve_bytes(60, 1);
        assert!(r > 9 << 20 && r < 12 << 20, "{r}");
        assert_eq!(reserve_bytes(1, 0), 2 << 20);
        assert_eq!(reserve_bytes(1000, 4), 64 << 20);
    }

    #[test]
    fn index_in_place_matches_the_copy() {
        let src = tmp("ip.mp4");
        let n = write_fragmented_r(&src, 12, 300_000, 256 << 10);
        let len = std::fs::metadata(&src).unwrap().len();
        let before = info(&src).unwrap();
        assert_eq!(before.layout, Layout::Fragmented);
        // The copy (pre-v1.7.1 way) as the reference.
        let copy = tmp("ip-copy.mp4");
        finalize(&src, &copy, &|| false).unwrap();
        let rep = index_in_place(&src).unwrap();
        assert!(rep.indexed, "{rep:?}");
        assert_eq!(rep.samples, n);
        assert_eq!(rep.fragments, before.fragments, "{rep:?}");
        // Same file size: nothing copied, nothing appended.
        assert_eq!(std::fs::metadata(&src).unwrap().len(), len);
        assert_eq!(probe(&src).unwrap(), Layout::Faststart);
        // Laid out like any faststart file: the index, then one mdat to the end of the file (a
        // player reads 4 box headers, not one per fragment).
        assert_eq!(top_kinds(&src), vec!["ftyp", "free", "moov", "mdat"]);
        let pp = parse(&src).unwrap();
        assert_eq!(pp.tops.last().unwrap().end(), pp.len);
        let after = info(&src).unwrap();
        let reference = info(&copy).unwrap();
        assert_eq!(after.fragments, 0);
        assert_eq!(after.video_frames, reference.video_frames);
        assert_eq!(after.keyframes, reference.keyframes);
        assert!((after.duration_secs - reference.duration_secs).abs() < 1e-9);
        assert_eq!(keyframes(&src).unwrap(), keyframes(&copy).unwrap());
        assert_eq!(frame_times(&src).unwrap(), frame_times(&copy).unwrap());
        assert_eq!(sample_bytes(&src), sample_bytes(&copy));
        // Running it again is a no-op.
        let again = index_in_place(&src).unwrap();
        assert!(!again.indexed, "{again:?}");
        // make_playable picks the in-place way.
        let src2 = tmp("ip2.mp4");
        write_fragmented_r(&src2, 3, 0, 256 << 10);
        let (in_place, _) = make_playable(&src2, &|| false).unwrap();
        assert!(in_place);
    }

    #[test]
    fn old_recordings_and_no_room_fall_back_to_the_copy() {
        // No reserve (recorded before v1.7.1): untouched, Unsupported; make_playable copies.
        let src = tmp("noroom.mp4");
        write_fragmented(&src, 4, 0);
        let bytes = std::fs::read(&src).unwrap();
        let e = index_in_place(&src).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::Unsupported);
        assert_eq!(std::fs::read(&src).unwrap(), bytes);
        let (in_place, rep) = make_playable(&src, &|| false).unwrap();
        assert!(!in_place && rep.from == Layout::Fragmented);
        assert_eq!(probe(&src).unwrap(), Layout::Faststart);
        // A reserve too small for the index (a very long game): untouched too.
        let small = tmp("small.mp4");
        write_fragmented_r(&small, 6, 0, 600);
        let bytes = std::fs::read(&small).unwrap();
        let e = index_in_place(&small).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::Unsupported, "{e}");
        assert_eq!(std::fs::read(&small).unwrap(), bytes);
    }

    #[test]
    fn power_cut_between_the_steps_is_resumed() {
        let reference = tmp("pc-ref.mp4");
        write_fragmented_r(&reference, 6, 100_000, 128 << 10);
        let want = sample_bytes(&reference);
        let frames = info(&reference).unwrap().video_frames;
        // Cut after step 1 (index hidden in the reserve): still the plain fragmented recording.
        let a = tmp("pc-a.mp4");
        std::fs::copy(&reference, &a).unwrap();
        index_steps(&a, 1).unwrap();
        assert_eq!(probe(&a).unwrap(), Layout::Fragmented);
        assert_eq!(info(&a).unwrap().fragments, info(&reference).unwrap().fragments);
        assert_eq!(sample_bytes(&a), want);
        assert!(index_in_place(&a).unwrap().indexed);
        assert_eq!(probe(&a).unwrap(), Layout::Faststart);
        assert_eq!(sample_bytes(&a), want);
        // After step 2 the file is final (the same as a full run).
        let b = tmp("pc-b.mp4");
        std::fs::copy(&reference, &b).unwrap();
        index_steps(&b, 2).unwrap();
        assert_eq!(probe(&b).unwrap(), Layout::Faststart);
        assert_eq!(sample_bytes(&b), want);
        assert!(!index_in_place(&b).unwrap().indexed);
        // A torn step 2: the old moov already `free`, the new one still hidden. The old one is
        // found again and the run redone.
        let c = tmp("pc-c.mp4");
        std::fs::copy(&reference, &c).unwrap();
        index_steps(&c, 1).unwrap();
        let mh = parse(&c).unwrap().moov_hdr;
        {
            let mut f = std::fs::OpenOptions::new().write(true).open(&c).unwrap();
            f.seek(SeekFrom::Start(mh.start + 4)).unwrap();
            f.write_all(b"free").unwrap();
        }
        assert_eq!(info(&c).unwrap().video_frames, frames, "torn: the old index is still found");
        assert!(index_in_place(&c).unwrap().indexed);
        assert_eq!(probe(&c).unwrap(), Layout::Faststart);
        assert_eq!(sample_bytes(&c), want);
    }

    #[test]
    fn crashed_recording_is_indexed_up_to_the_cut() {
        let full = tmp("cr-full.mp4");
        write_fragmented_r(&full, 10, 0, 32 << 10);
        let bytes = std::fs::read(&full).unwrap();
        for frac in [0.37, 0.5, 0.81] {
            let cut = tmp("cr-cut.mp4");
            std::fs::write(&cut, &bytes[..(bytes.len() as f64 * frac) as usize + 17]).unwrap();
            let before = info(&cut).unwrap();
            let r = index_in_place(&cut).unwrap();
            assert!(r.indexed);
            let after = info(&cut).unwrap();
            assert_eq!(after.layout, Layout::Faststart);
            assert_eq!(after.video_frames, before.video_frames);
            assert!(after.duration_secs > 10.0 * frac - 1.5 && after.duration_secs < 10.0 * frac + 0.5, "{frac}: {after:?}");
            // The cut piece is inside the one mdat, which runs to the end of the file.
            let p = parse(&cut).unwrap();
            assert_eq!(top_kinds(&cut), vec!["ftyp", "free", "moov", "mdat"], "{frac}");
            assert_eq!(p.tops.last().unwrap().end(), p.len);
        }
    }

    #[test]
    fn cancel_leaves_the_original_alone() {
        let src = tmp("c.mp4");
        write_fragmented(&src, 3, 0);
        let len = std::fs::metadata(&src).unwrap().len();
        let e = finalize_in_place(&src, &|| true).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::Interrupted);
        assert_eq!(std::fs::metadata(&src).unwrap().len(), len);
        assert!(!src.with_extension("mp4.finalizing").exists());
        finalize_in_place(&src, &|| false).unwrap();
        assert_eq!(probe(&src).unwrap(), Layout::Faststart);
    }
}
