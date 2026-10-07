//! MP4 writing, pure Rust (no platform code, unit-tested with ffmpeg on the build machine).
//!
//! - [`FragmentedWriter`]: the full-game recording. Fragmented MP4: a small header, then a
//!   `moof`+`mdat` pair about every second. Each fragment is complete on its own, so a crash
//!   or power cut only loses the last second. A clean stop appends an `mfra` index for fast seeking.
//!   Right after the header the writer leaves an empty `free` box ([`reserve_bytes`]): when the
//!   recording stops, the full index is written into it in place (`remux::index_in_place`), so
//!   the file becomes a "faststart" MP4 within a fraction of a second, without copying the media.
//! - [`write_clip`]: a normal MP4 (index at the front) for replay-buffer clips.
//!
//! Tracks: H.264 video (AVCC samples, no B-frames) and any number of AAC audio tracks.
//! Timestamps are in 100 ns units (Windows' QPC/`TimeSpan` unit) relative to the recording start.

use std::io::{Seek, Write};

pub const HNS: i64 = 10_000_000; // 100 ns ticks per second
pub const VIDEO_TIMESCALE: u32 = 90_000;

#[derive(Debug, Clone)]
pub struct VideoConfig {
    pub width: u32,
    pub height: u32,
    pub sps: Vec<u8>,
    pub pps: Vec<u8>,
    /// Nominal frame rate, used for the last frame's duration.
    pub fps: u32,
}

#[derive(Debug, Clone)]
pub struct AudioConfig {
    pub sample_rate: u32,
    pub channels: u16,
    /// AAC AudioSpecificConfig (2 bytes for AAC-LC).
    pub asc: Vec<u8>,
    pub name: String,
}

impl AudioConfig {
    /// AudioSpecificConfig for AAC-LC.
    pub fn aac_lc(sample_rate: u32, channels: u16, name: &str) -> Self {
        let idx = [96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350]
            .iter()
            .position(|r| *r == sample_rate)
            .unwrap_or(3) as u16;
        let v: u16 = (2 << 11) | (idx << 7) | ((channels & 0xF) << 3);
        Self { sample_rate, channels, asc: v.to_be_bytes().to_vec(), name: name.into() }
    }
}

/// One encoded frame. `track` 0 = video, 1.. = audio tracks in the order given.
#[derive(Debug, Clone)]
pub struct Packet {
    pub track: usize,
    /// Presentation time in 100 ns units since the recording start.
    pub pts: i64,
    pub data: Vec<u8>,
    pub key: bool,
}

// ---------- H.264 helpers ----------

/// Splits an Annex-B byte stream (00 00 01 / 00 00 00 01 start codes) into NAL units.
pub fn split_annexb(data: &[u8]) -> Vec<&[u8]> {
    let mut nals = Vec::new();
    let mut i = 0;
    let mut start: Option<usize> = None;
    while i + 2 < data.len() {
        if data[i] == 0 && data[i + 1] == 0 && data[i + 2] == 1 {
            if let Some(s) = start {
                let mut end = i;
                while end > s && data[end - 1] == 0 {
                    end -= 1;
                }
                nals.push(&data[s..end]);
            }
            i += 3;
            start = Some(i);
        } else {
            i += 1;
        }
    }
    if let Some(s) = start {
        if s < data.len() {
            nals.push(&data[s..]);
        }
    } else if !data.is_empty() {
        // Already length-prefixed or a single NAL without start code.
        nals.push(data);
    }
    nals
}

/// Result of converting one encoder output buffer.
#[derive(Debug, Default)]
pub struct AccessUnit {
    /// Length-prefixed (4 byte) NAL units, parameter sets and AUDs removed.
    pub avcc: Vec<u8>,
    pub key: bool,
    pub sps: Option<Vec<u8>>,
    pub pps: Option<Vec<u8>>,
}

pub fn annexb_to_avcc(data: &[u8]) -> AccessUnit {
    let mut au = AccessUnit::default();
    for nal in split_annexb(data) {
        if nal.is_empty() {
            continue;
        }
        match nal[0] & 0x1F {
            7 => au.sps = Some(nal.to_vec()),
            8 => au.pps = Some(nal.to_vec()),
            9 => {} // access unit delimiter
            t => {
                if t == 5 {
                    au.key = true;
                }
                au.avcc.extend_from_slice(&(nal.len() as u32).to_be_bytes());
                au.avcc.extend_from_slice(nal);
            }
        }
    }
    au
}

// ---------- box helpers ----------

struct B(Vec<u8>);

impl B {
    fn new() -> Self {
        B(Vec::with_capacity(256))
    }
    fn u8(&mut self, v: u8) -> &mut Self {
        self.0.push(v);
        self
    }
    fn u16(&mut self, v: u16) -> &mut Self {
        self.0.extend_from_slice(&v.to_be_bytes());
        self
    }
    fn u32(&mut self, v: u32) -> &mut Self {
        self.0.extend_from_slice(&v.to_be_bytes());
        self
    }
    fn u64(&mut self, v: u64) -> &mut Self {
        self.0.extend_from_slice(&v.to_be_bytes());
        self
    }
    fn bytes(&mut self, v: &[u8]) -> &mut Self {
        self.0.extend_from_slice(v);
        self
    }
    fn zeros(&mut self, n: usize) -> &mut Self {
        self.0.resize(self.0.len() + n, 0);
        self
    }
}

fn boxed(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(body.len() + 8);
    v.extend_from_slice(&((body.len() + 8) as u32).to_be_bytes());
    v.extend_from_slice(kind);
    v.extend_from_slice(body);
    v
}

fn full(kind: &[u8; 4], version: u8, flags: u32, body: &[u8]) -> Vec<u8> {
    let mut b = Vec::with_capacity(body.len() + 4);
    b.push(version);
    b.extend_from_slice(&flags.to_be_bytes()[1..]);
    b.extend_from_slice(body);
    boxed(kind, &b)
}

fn cat(parts: &[Vec<u8>]) -> Vec<u8> {
    parts.concat()
}

const MATRIX: [u32; 9] = [0x0001_0000, 0, 0, 0, 0x0001_0000, 0, 0, 0, 0x4000_0000];

/// The `ftyp` the recorder writes (test tools).
#[doc(hidden)]
pub fn fragmented_ftyp() -> Vec<u8> {
    ftyp(true)
}

fn ftyp(fragmented: bool) -> Vec<u8> {
    let mut b = B::new();
    b.bytes(b"isom").u32(0x200).bytes(b"isom").bytes(b"iso2").bytes(b"avc1").bytes(b"mp41");
    if fragmented {
        b.bytes(b"iso6");
    }
    boxed(b"ftyp", &b.0)
}

fn mvhd(duration_ms: u64, next_track: u32) -> Vec<u8> {
    let mut b = B::new();
    b.u32(0).u32(0).u32(1000).u32(duration_ms.min(u32::MAX as u64) as u32).u32(0x0001_0000).u16(0x0100).zeros(10);
    for m in MATRIX {
        b.u32(m);
    }
    b.zeros(24).u32(next_track);
    full(b"mvhd", 0, 0, &b.0)
}

fn tkhd(track_id: u32, duration_ms: u64, audio: bool, w: u32, h: u32) -> Vec<u8> {
    let mut b = B::new();
    b.u32(0).u32(0).u32(track_id).u32(0).u32(duration_ms.min(u32::MAX as u64) as u32).zeros(8);
    b.u16(0).u16(if audio { 1 } else { 0 }).u16(if audio { 0x0100 } else { 0 }).u16(0);
    for m in MATRIX {
        b.u32(m);
    }
    b.u32(w << 16).u32(h << 16);
    full(b"tkhd", 0, 3, &b.0)
}

fn mdhd(timescale: u32, duration: u64) -> Vec<u8> {
    let mut b = B::new();
    b.u32(0).u32(0).u32(timescale).u32(duration.min(u32::MAX as u64) as u32).u16(0x55c4).u16(0);
    full(b"mdhd", 0, 0, &b.0)
}

fn hdlr(audio: bool, name: &str) -> Vec<u8> {
    let mut b = B::new();
    b.u32(0).bytes(if audio { b"soun" } else { b"vide" }).zeros(12).bytes(name.as_bytes()).u8(0);
    full(b"hdlr", 0, 0, &b.0)
}

fn dinf() -> Vec<u8> {
    let url = full(b"url ", 0, 1, &[]);
    let mut d = B::new();
    d.u32(1).bytes(&url);
    boxed(b"dinf", &full(b"dref", 0, 0, &d.0))
}

fn avc1(v: &VideoConfig) -> Vec<u8> {
    let sps = &v.sps;
    let mut c = B::new();
    c.u8(1).u8(*sps.get(1).unwrap_or(&100)).u8(*sps.get(2).unwrap_or(&0)).u8(*sps.get(3).unwrap_or(&40)).u8(0xFF).u8(0xE1);
    c.u16(sps.len() as u16).bytes(sps).u8(1).u16(v.pps.len() as u16).bytes(&v.pps);
    if matches!(sps.get(1), Some(100) | Some(110) | Some(122) | Some(144)) {
        c.u8(0xFC | 1).u8(0xF8).u8(0xF8).u8(0);
    }
    let avcc = boxed(b"avcC", &c.0);
    let mut b = B::new();
    b.zeros(6).u16(1).zeros(16).u16(v.width as u16).u16(v.height as u16).u32(0x0048_0000).u32(0x0048_0000).u32(0).u16(1);
    let mut name = [0u8; 32];
    let n = b"Clairvoyance";
    name[0] = n.len() as u8;
    name[1..1 + n.len()].copy_from_slice(n);
    b.bytes(&name).u16(0x18).u16(0xFFFF).bytes(&avcc);
    boxed(b"avc1", &b.0)
}

fn descr(tag: u8, body: &[u8]) -> Vec<u8> {
    let n = body.len() as u32;
    let mut v = vec![tag, 0x80 | ((n >> 21) & 0x7F) as u8, 0x80 | ((n >> 14) & 0x7F) as u8, 0x80 | ((n >> 7) & 0x7F) as u8, (n & 0x7F) as u8];
    v.extend_from_slice(body);
    v
}

fn mp4a(a: &AudioConfig) -> Vec<u8> {
    let dsi = descr(0x05, &a.asc);
    let mut dcd = B::new();
    dcd.u8(0x40).u8(0x15).u8(0).u16(0).u32(0).u32(0).bytes(&dsi);
    let dcd = descr(0x04, &dcd.0);
    let sl = descr(0x06, &[0x02]);
    let mut es = B::new();
    es.u16(0).u8(0).bytes(&dcd).bytes(&sl);
    let esds = full(b"esds", 0, 0, &descr(0x03, &es.0));
    let mut b = B::new();
    b.zeros(6).u16(1).zeros(8).u16(a.channels).u16(16).u16(0).u16(0).u32(a.sample_rate << 16).bytes(&esds);
    boxed(b"mp4a", &b.0)
}

/// Sample tables for a non-fragmented file; empty for fragmented.
#[derive(Default)]
struct Tables {
    durations: Vec<u32>,
    sizes: Vec<u32>,
    offsets: Vec<u64>,
    sync: Option<Vec<u32>>, // 1-based sample numbers, video only
}

fn stbl(entry: Vec<u8>, t: &Tables) -> Vec<u8> {
    let mut sd = B::new();
    sd.u32(1).bytes(&entry);
    let stsd = full(b"stsd", 0, 0, &sd.0);
    // stts: run-length of durations
    let mut runs: Vec<(u32, u32)> = Vec::new();
    for d in &t.durations {
        match runs.last_mut() {
            Some((n, v)) if v == d => *n += 1,
            _ => runs.push((1, *d)),
        }
    }
    let mut s = B::new();
    s.u32(runs.len() as u32);
    for (n, v) in &runs {
        s.u32(*n).u32(*v);
    }
    let stts = full(b"stts", 0, 0, &s.0);
    let mut s = B::new();
    if t.sizes.is_empty() {
        s.u32(0);
    } else {
        s.u32(1).u32(1).u32(1).u32(1);
    }
    let stsc = full(b"stsc", 0, 0, &s.0);
    let mut s = B::new();
    s.u32(0).u32(t.sizes.len() as u32);
    for z in &t.sizes {
        s.u32(*z);
    }
    let stsz = full(b"stsz", 0, 0, &s.0);
    let mut s = B::new();
    s.u32(t.offsets.len() as u32);
    for o in &t.offsets {
        s.u64(*o);
    }
    let co64 = full(b"co64", 0, 0, &s.0);
    let mut parts = vec![stsd, stts, stsc, stsz, co64];
    if let Some(sync) = &t.sync {
        let mut s = B::new();
        s.u32(sync.len() as u32);
        for n in sync {
            s.u32(*n);
        }
        parts.push(full(b"stss", 0, 0, &s.0));
    }
    boxed(b"stbl", &cat(&parts))
}

fn trak(track_id: u32, audio: bool, entry: Vec<u8>, timescale: u32, media_duration: u64, w: u32, h: u32, name: &str, t: &Tables) -> Vec<u8> {
    let dur_ms = media_duration * 1000 / timescale as u64;
    let xmhd = if audio { full(b"smhd", 0, 0, &[0, 0, 0, 0]) } else { full(b"vmhd", 0, 1, &[0; 8]) };
    let minf = boxed(b"minf", &cat(&[xmhd, dinf(), stbl(entry, t)]));
    let mdia = boxed(b"mdia", &cat(&[mdhd(timescale, media_duration), hdlr(audio, name), minf]));
    boxed(b"trak", &cat(&[tkhd(track_id, dur_ms, audio, w, h), mdia]))
}

fn ticks(hns: i64, timescale: u32) -> i64 {
    // Rounded conversion from 100 ns units.
    (hns as i128 * timescale as i128 + (HNS as i128 / 2)).div_euclid(HNS as i128) as i64
}

fn track_timescale(track: usize, audio: &[AudioConfig]) -> u32 {
    if track == 0 {
        VIDEO_TIMESCALE
    } else {
        audio[track - 1].sample_rate
    }
}

fn default_duration(track: usize, v: &VideoConfig, _audio: &[AudioConfig]) -> u32 {
    if track == 0 {
        VIDEO_TIMESCALE / v.fps.max(1)
    } else {
        1024 // AAC frame
    }
}

// ---------- fragmented writer ----------

struct Pending {
    data: Vec<u8>,
    ticks: i64,
    key: bool,
}

struct TrackState {
    /// Samples ready for the next fragment: (data, duration, key)
    ready: Vec<(Vec<u8>, u32, bool)>,
    /// Last sample, waiting for the next one to know its duration.
    held: Option<Pending>,
    /// Decode time (in track ticks) of the first sample in `ready`.
    next_dts: Option<i64>,
    total: i64,
    frags: Vec<(u64, u64)>, // (decode time, moof offset) for mfra
}

/// Room left after the header for the full index written when the recording stops: enough for
/// 3 hours at `fps` with `audio_tracks` AAC tracks (worst case: every video frame its own
/// duration entry), 2-64 MB. A longer game is finalized by copying instead (as before v1.7.1).
pub fn reserve_bytes(fps: u32, audio_tracks: usize) -> u64 {
    let secs = 3 * 3600u64;
    let per_sec = fps.max(1) as u64 * 12 + audio_tracks as u64 * 200;
    let per_frag = (1 + audio_tracks as u64) * 20;
    (secs * (per_sec + per_frag) + (256 << 10)).clamp(2 << 20, 64 << 20)
}

pub struct FragmentedWriter<W: Write + Seek> {
    out: W,
    video: VideoConfig,
    audio: Vec<AudioConfig>,
    tracks: Vec<TrackState>,
    seq: u32,
    pos: u64,
    bytes_written: u64,
}

impl<W: Write + Seek> FragmentedWriter<W> {
    /// A recording with room for the in-place index ([`reserve_bytes`]).
    pub fn new(out: W, video: VideoConfig, audio: Vec<AudioConfig>) -> std::io::Result<Self> {
        let reserve = reserve_bytes(video.fps, audio.len());
        Self::with_reserve(out, video, audio, reserve)
    }

    /// `reserve` = size of the empty `free` box after the header (0 = none, the pre-v1.7.1 layout).
    pub fn with_reserve(mut out: W, video: VideoConfig, audio: Vec<AudioConfig>, reserve: u64) -> std::io::Result<Self> {
        let mut traks = vec![trak(1, false, avc1(&video), VIDEO_TIMESCALE, 0, video.width, video.height, "Video", &Tables::default())];
        let mut trex = Vec::new();
        for (i, a) in audio.iter().enumerate() {
            traks.push(trak(2 + i as u32, true, mp4a(a), a.sample_rate, 0, 0, 0, &a.name, &Tables::default()));
        }
        for id in 1..=(1 + audio.len() as u32) {
            let mut b = B::new();
            b.u32(id).u32(1).u32(0).u32(0).u32(0);
            trex.push(full(b"trex", 0, 0, &b.0));
        }
        let mvex = boxed(b"mvex", &cat(&trex));
        let mut moov_parts = vec![mvhd(0, 2 + audio.len() as u32)];
        moov_parts.extend(traks);
        moov_parts.push(mvex);
        let mut head = cat(&[ftyp(true), boxed(b"moov", &cat(&moov_parts))]);
        if reserve >= 8 {
            // 32-bit box size: the reserve is at most 64 MB.
            let r = reserve.min(u32::MAX as u64);
            head.extend_from_slice(&(r as u32).to_be_bytes());
            head.extend_from_slice(b"free");
            head.resize(head.len() + (r - 8) as usize, 0);
        }
        out.write_all(&head)?;
        out.flush()?;
        let n = audio.len() + 1;
        Ok(Self {
            out,
            video,
            audio,
            tracks: (0..n).map(|_| TrackState { ready: Vec::new(), held: None, next_dts: None, total: 0, frags: Vec::new() }).collect(),
            seq: 0,
            pos: head.len() as u64,
            bytes_written: head.len() as u64,
        })
    }

    pub fn get_mut(&mut self) -> &mut W {
        &mut self.out
    }

    pub fn bytes_written(&self) -> u64 {
        self.bytes_written
    }

    pub fn push(&mut self, p: Packet) {
        let Some(t) = self.tracks.get_mut(p.track) else { return };
        let ts = track_timescale(p.track, &self.audio);
        let mut tk = ticks(p.pts.max(0), ts);
        if let Some(h) = t.held.take() {
            // Never go backwards in time (duration at least 1 tick).
            let dur = (tk - h.ticks).clamp(1, u32::MAX as i64);
            tk = h.ticks + dur;
            if t.next_dts.is_none() {
                t.next_dts = Some(h.ticks);
            }
            t.ready.push((h.data, dur as u32, h.key));
        }
        t.held = Some(Pending { data: p.data, ticks: tk, key: p.key });
    }

    /// Writes everything ready so far as one fragment. Call about once per second.
    pub fn flush_fragment(&mut self) -> std::io::Result<()> {
        if self.tracks.iter().all(|t| t.ready.is_empty()) {
            return Ok(());
        }
        self.seq += 1;
        // Build trafs with placeholder data offsets; offsets are patched after we know the moof size.
        let mut trafs: Vec<(usize, Vec<u8>, usize)> = Vec::new(); // (track, bytes, offset-of-data_offset-field)
        for (i, t) in self.tracks.iter().enumerate() {
            if t.ready.is_empty() {
                continue;
            }
            let mut tfhd = B::new();
            tfhd.u32(i as u32 + 1);
            let tfhd = full(b"tfhd", 0, 0x02_0000, &tfhd.0);
            let dts = t.next_dts.unwrap_or(0).max(0) as u64;
            let mut tfdt = B::new();
            tfdt.u64(dts);
            let tfdt = full(b"tfdt", 1, 0, &tfdt.0);
            let mut tr = B::new();
            tr.u32(t.ready.len() as u32).u32(0);
            for (data, dur, key) in &t.ready {
                let flags = if i == 0 && !key { 0x0101_0000 } else { 0x0200_0000 };
                tr.u32(*dur).u32(data.len() as u32).u32(flags);
            }
            let trun = full(b"trun", 0, 0x000701, &tr.0);
            // data_offset lives 8 (box header) + 4 (version/flags) + 4 (sample count) bytes into trun.
            let off_in_traf = 8 + tfhd.len() + tfdt.len() + 16;
            let traf = boxed(b"traf", &cat(&[tfhd, tfdt, trun]));
            trafs.push((i, traf, off_in_traf));
        }
        let mut mfhd = B::new();
        mfhd.u32(self.seq);
        let mfhd = full(b"mfhd", 0, 0, &mfhd.0);
        let moof_len = 8 + mfhd.len() + trafs.iter().map(|t| t.1.len()).sum::<usize>();
        // Patch data offsets (relative to moof start): moof, then mdat header, then track data in order.
        let mut data_pos = moof_len + 8;
        let mut traf_pos = 8 + mfhd.len();
        for (i, traf, off) in trafs.iter_mut() {
            let o = (data_pos as u32).to_be_bytes();
            traf[*off..*off + 4].copy_from_slice(&o);
            data_pos += self.tracks[*i].ready.iter().map(|r| r.0.len()).sum::<usize>();
            traf_pos += traf.len();
        }
        let _ = traf_pos;
        let moof = boxed(b"moof", &cat(&[vec![mfhd], trafs.iter().map(|t| t.1.clone()).collect()].concat()));
        let mdat_len = data_pos - moof_len;
        let moof_offset = self.pos;
        self.out.write_all(&moof)?;
        self.out.write_all(&(mdat_len as u32).to_be_bytes())?;
        self.out.write_all(b"mdat")?;
        for (i, _, _) in &trafs {
            let t = &mut self.tracks[*i];
            let dts = t.next_dts.unwrap_or(0).max(0) as u64;
            t.frags.push((dts, moof_offset));
            let mut dur_sum = 0i64;
            for (data, dur, _) in t.ready.drain(..) {
                self.out.write_all(&data)?;
                dur_sum += dur as i64;
            }
            t.total += dur_sum;
            t.next_dts = Some(t.next_dts.unwrap_or(0) + dur_sum);
        }
        self.out.flush()?;
        self.pos += (moof.len() + mdat_len) as u64;
        self.bytes_written = self.pos;
        Ok(())
    }

    /// Writes the held samples and an `mfra` index. Returns the writer.
    pub fn finish(mut self) -> std::io::Result<W> {
        for i in 0..self.tracks.len() {
            let d = default_duration(i, &self.video, &self.audio);
            let t = &mut self.tracks[i];
            if let Some(h) = t.held.take() {
                if t.next_dts.is_none() {
                    t.next_dts = Some(h.ticks);
                }
                t.ready.push((h.data, d, h.key));
            }
        }
        self.flush_fragment()?;
        let mut tfras = Vec::new();
        for (i, t) in self.tracks.iter().enumerate() {
            let mut b = B::new();
            b.u32(i as u32 + 1).u32(0).u32(t.frags.len() as u32);
            for (time, off) in &t.frags {
                b.u64(*time).u64(*off).u8(1).u8(1).u8(1);
            }
            tfras.push(full(b"tfra", 1, 0, &b.0));
        }
        let body_len: usize = tfras.iter().map(|t| t.len()).sum::<usize>() + 8 + 16;
        let mut mfro = B::new();
        mfro.u32(body_len as u32);
        let mfra = boxed(b"mfra", &cat(&[tfras, vec![full(b"mfro", 0, 0, &mfro.0)]].concat()));
        self.out.write_all(&mfra)?;
        self.out.flush()?;
        Ok(self.out)
    }

    /// Duration in seconds of what has been written (video track).
    pub fn duration_secs(&self) -> f64 {
        self.tracks[0].total as f64 / VIDEO_TIMESCALE as f64
    }
}

// ---------- regular MP4 (clips) ----------

/// Writes a normal MP4 with the index at the front. `packets` must be sorted by pts per track.
/// The clip's timeline starts at `start` (100 ns units); earlier packets are dropped.
pub fn write_clip<W: Write + Seek>(mut out: W, video: &VideoConfig, audio: &[AudioConfig], packets: &[Packet], start: i64) -> std::io::Result<f64> {
    let n = 1 + audio.len();
    let mut per: Vec<Vec<&Packet>> = vec![Vec::new(); n];
    for p in packets {
        if p.track < n && p.pts >= start {
            per[p.track].push(p);
        }
    }
    // Video must start on a keyframe.
    if let Some(k) = per[0].iter().position(|p| p.key) {
        per[0].drain(..k);
    } else {
        per[0].clear();
    }
    let vstart = per[0].first().map(|p| p.pts).unwrap_or(start);
    for t in per.iter_mut().skip(1) {
        t.retain(|p| p.pts >= vstart);
    }
    let mut tables: Vec<Tables> = Vec::new();
    let mut durations = Vec::new();
    for (i, t) in per.iter().enumerate() {
        let ts = track_timescale(i, audio);
        let mut tb = Tables { sync: if i == 0 { Some(Vec::new()) } else { None }, ..Default::default() };
        for (j, p) in t.iter().enumerate() {
            let d = match t.get(j + 1) {
                Some(nx) => (ticks(nx.pts - vstart, ts) - ticks(p.pts - vstart, ts)).max(1) as u32,
                None => default_duration(i, video, audio),
            };
            tb.durations.push(d);
            tb.sizes.push(p.data.len() as u32);
            if p.key {
                if let Some(s) = tb.sync.as_mut() {
                    s.push(j as u32 + 1);
                }
            }
        }
        durations.push(tb.durations.iter().map(|d| *d as u64).sum::<u64>());
        tables.push(tb);
    }
    let build = |tables: &[Tables]| -> Vec<u8> {
        let mut parts = vec![mvhd(durations[0] * 1000 / VIDEO_TIMESCALE as u64, n as u32 + 1)];
        parts.push(trak(1, false, avc1(video), VIDEO_TIMESCALE, durations[0], video.width, video.height, "Video", &tables[0]));
        for (i, a) in audio.iter().enumerate() {
            parts.push(trak(2 + i as u32, true, mp4a(a), a.sample_rate, durations[i + 1], 0, 0, &a.name, &tables[i + 1]));
        }
        boxed(b"moov", &cat(&parts))
    };
    for t in tables.iter_mut() {
        t.offsets = vec![0; t.sizes.len()];
    }
    // Offsets are fixed-size (co64), so the header size doesn't depend on their values.
    let head_len = ftyp(false).len() + build(&tables).len();
    // Interleave samples in pts order in mdat; mdat uses a 64-bit size header (16 bytes).
    let mut order: Vec<(i64, usize, usize)> = Vec::new();
    for (i, t) in per.iter().enumerate() {
        for (j, p) in t.iter().enumerate() {
            order.push((p.pts, i, j));
        }
    }
    order.sort();
    let mut off = (head_len + 16) as u64;
    for (_, i, j) in &order {
        tables[*i].offsets[*j] = off;
        off += per[*i][*j].data.len() as u64;
    }
    out.write_all(&ftyp(false))?;
    out.write_all(&build(&tables))?;
    let mdat_len = off - head_len as u64;
    out.write_all(&1u32.to_be_bytes())?;
    out.write_all(b"mdat")?;
    out.write_all(&mdat_len.to_be_bytes())?;
    for (_, i, j) in &order {
        out.write_all(&per[*i][*j].data)?;
    }
    out.flush()?;
    let _ = out.stream_position();
    Ok(durations[0] as f64 / VIDEO_TIMESCALE as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn annexb_parsing() {
        let data = [0, 0, 0, 1, 0x67, 1, 2, 0, 0, 1, 0x68, 3, 0, 0, 0, 1, 0x09, 0xF0, 0, 0, 1, 0x65, 9, 9, 9];
        let au = annexb_to_avcc(&data);
        assert_eq!(au.sps.as_deref(), Some(&[0x67, 1, 2][..]));
        assert_eq!(au.pps.as_deref(), Some(&[0x68, 3][..]));
        assert!(au.key);
        assert_eq!(au.avcc, vec![0, 0, 0, 4, 0x65, 9, 9, 9]);
    }

    #[test]
    fn asc() {
        assert_eq!(AudioConfig::aac_lc(48000, 2, "a").asc, vec![0x11, 0x90]);
        assert_eq!(AudioConfig::aac_lc(44100, 2, "a").asc, vec![0x12, 0x10]);
    }

    #[test]
    fn ticks_rounding() {
        assert_eq!(ticks(HNS, 90_000), 90_000);
        assert_eq!(ticks(166_667, 90_000), 1500);
    }
}
