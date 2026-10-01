//! "Save test report" (Settings > Advanced): one small zip with what's needed to understand a
//! problem after a game, without describing it by hand. No videos, no clips, no Riot ID.
//!
//! Contents: `report.txt` (version, GPU/encoder, settings summary, storage, library, the latest
//! recordings' file layout and keyframes, the window's responsiveness numbers, game modes),
//! `clairvoyance.log` (the last 5 MB), the latest game's `session.json` (events, offset, perf),
//! `settings.json` (Riot ID removed) and `cleanup-log.json`.

use crate::state::AppState;
use std::fmt::Write as _;
use std::io::Write;
use std::path::{Path, PathBuf};

const LOG_MAX: u64 = 5 << 20;

pub fn write(st: &AppState, ui: &serde_json::Value) -> anyhow::Result<PathBuf> {
    let dir = st.save_dir().join("test-reports");
    std::fs::create_dir_all(&dir)?;
    let stamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S");
    let path = dir.join(format!("Clairvoyance-test-report-{stamp}.zip"));
    let mut zip = Zip::new(std::fs::File::create(&path)?);

    let mut t = String::new();
    let s = st.settings();
    let _ = writeln!(t, "Clairvoyance {} test report, {}", env!("CARGO_PKG_VERSION"), chrono::Local::now().to_rfc3339());
    let _ = writeln!(t, "Windows: {}", crate::platform::os_version());
    let _ = writeln!(t, "GPU: {:?}", st.gpu);
    let _ = writeln!(
        t,
        "Recording: encoder {} ({}), {}p{} {}, replay buffer {} s, mic {}, display capture {}",
        s.video.encoder,
        crate::state::resolve_encoder(&s, st.gpu.as_ref()).video.encoder,
        s.video.height,
        s.video.fps,
        s.video.quality,
        s.video.replay_buffer_secs,
        s.video.record_mic,
        s.video.display_capture
    );
    let _ = writeln!(t, "Storage: limit {} GB, auto clean-up {}, theme {}, auto update check {}", s.max_disk_gb, s.auto_cleanup, s.theme(), s.auto_update_check);
    let live = st.live.lock().unwrap().clone();
    let _ = writeln!(t, "Engine now: {:?} {}", live.state, live.message.clone().unwrap_or_default());

    let games = st.library.summaries();
    let used: u64 = games.iter().map(|g| g.size_bytes).sum();
    let _ = writeln!(t, "\nLibrary: {} games, {} in {}", games.len(), crate::maintenance::fmt_bytes(used), st.save_dir().display());
    let _ = writeln!(t, "Free disk space: {}", crate::platform::free_space(&st.save_dir()).map(crate::maintenance::fmt_bytes).unwrap_or_else(|| "?".into()));
    let _ = writeln!(t, "\nLatest recordings (file layout, codec, keyframes):");
    for g in games.iter().take(8) {
        let line = match &g.video_path {
            Some(v) => match cv_capture::remux::info(v) {
                Ok(i) => format!(
                    "{:?}, {}, {:.0} s, {} fragments, keyframes every {:.2} s (max {:.2} s), {}",
                    i.layout,
                    i.codec.unwrap_or_default(),
                    i.duration_secs,
                    i.fragments,
                    i.keyframe_interval_avg,
                    i.keyframe_interval_max,
                    crate::maintenance::fmt_bytes(i.bytes)
                ),
                Err(e) => format!("can't read the video: {e}"),
            },
            None => "no video".into(),
        };
        let _ = writeln!(
            t,
            "  {} | {} | {} | {} | {}",
            g.id,
            g.mode_name.clone().unwrap_or_default(),
            g.record_mode.clone().unwrap_or_else(|| "full".into()),
            g.player.as_ref().and_then(|p| p.character.clone()).unwrap_or_default(),
            line
        );
    }
    let _ = writeln!(t, "\nWindow responsiveness (this run): {}", serde_json::to_string_pretty(ui).unwrap_or_default());
    let timings = st.startup.lock().unwrap();
    let _ = writeln!(t, "Startup: window painted {:?} ms after launch (page {:?} ms)", timings.0, timings.1);
    drop(timings);
    for (game, m) in &s.modes {
        let new = m.entries.values().filter(|e| e.is_new).count();
        let off: Vec<&str> = m.entries.values().filter(|e| e.rule == cv_core::modes::ModeRule::Off && e.available != Some(false)).map(|e| e.name.as_str()).collect();
        let clips: Vec<&str> = m.entries.values().filter(|e| e.rule == cv_core::modes::ModeRule::ClipsOnly && e.available != Some(false)).map(|e| e.name.as_str()).collect();
        let _ = writeln!(t, "\nGame modes ({game}): {} known, {new} new, unknown rule {:?}, list from client {:?}", m.entries.len(), m.unknown_rule, m.catalog_updated_at);
        let _ = writeln!(t, "  off: {}", off.join(", "));
        let _ = writeln!(t, "  clips only: {}", clips.join(", "));
    }
    zip.add("report.txt", t.as_bytes())?;

    zip.add("clairvoyance.log", &tail(&st.paths.log_file, LOG_MAX))?;
    if let Some(g) = games.first() {
        if let Ok(b) = std::fs::read(g.dir.join(cv_core::session::SESSION_FILE)) {
            zip.add("latest-game-session.json", &b)?;
        }
    }
    let mut v = serde_json::to_value(&s)?;
    redact(&mut v);
    zip.add("settings.json", serde_json::to_string_pretty(&v)?.as_bytes())?;
    if let Ok(b) = std::fs::read(st.paths.data_dir.join("cleanup-log.json")) {
        zip.add("cleanup-log.json", &b)?;
    }
    zip.finish()?;
    log::info!("test report saved: {}", path.display());
    Ok(path)
}

/// Removes the Riot ID (and anything else that names the player) from the settings copy.
fn redact(v: &mut serde_json::Value) {
    match v {
        serde_json::Value::Object(m) => {
            for (k, x) in m.iter_mut() {
                if k == "riot_id" || k == "player_name" {
                    *x = serde_json::Value::String("(removed)".into());
                } else {
                    redact(x);
                }
            }
        }
        serde_json::Value::Array(a) => a.iter_mut().for_each(redact),
        _ => {}
    }
}

fn tail(path: &Path, max: u64) -> Vec<u8> {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = std::fs::File::open(path) else { return b"(no log file)".to_vec() };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let _ = f.seek(SeekFrom::Start(len.saturating_sub(max)));
    let mut b = Vec::new();
    let _ = f.read_to_end(&mut b);
    b
}

/// A minimal zip writer ("stored", no compression: the files are small text).
struct Zip<W: Write> {
    out: W,
    pos: u32,
    central: Vec<u8>,
    count: u16,
}

impl<W: Write> Zip<W> {
    fn new(out: W) -> Self {
        Zip { out, pos: 0, central: Vec::new(), count: 0 }
    }

    fn add(&mut self, name: &str, data: &[u8]) -> std::io::Result<()> {
        let crc = crc32(data);
        let n = name.as_bytes();
        let (time, date) = dos_time();
        let mut h = Vec::new();
        h.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        h.extend_from_slice(&20u16.to_le_bytes()); // version needed
        h.extend_from_slice(&0x0800u16.to_le_bytes()); // UTF-8 names
        h.extend_from_slice(&0u16.to_le_bytes()); // stored
        h.extend_from_slice(&time.to_le_bytes());
        h.extend_from_slice(&date.to_le_bytes());
        h.extend_from_slice(&crc.to_le_bytes());
        h.extend_from_slice(&(data.len() as u32).to_le_bytes());
        h.extend_from_slice(&(data.len() as u32).to_le_bytes());
        h.extend_from_slice(&(n.len() as u16).to_le_bytes());
        h.extend_from_slice(&0u16.to_le_bytes());
        h.extend_from_slice(n);
        self.out.write_all(&h)?;
        self.out.write_all(data)?;
        let c = &mut self.central;
        c.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        c.extend_from_slice(&20u16.to_le_bytes());
        c.extend_from_slice(&20u16.to_le_bytes());
        c.extend_from_slice(&0x0800u16.to_le_bytes());
        c.extend_from_slice(&0u16.to_le_bytes());
        c.extend_from_slice(&time.to_le_bytes());
        c.extend_from_slice(&date.to_le_bytes());
        c.extend_from_slice(&crc.to_le_bytes());
        c.extend_from_slice(&(data.len() as u32).to_le_bytes());
        c.extend_from_slice(&(data.len() as u32).to_le_bytes());
        c.extend_from_slice(&(n.len() as u16).to_le_bytes());
        c.extend_from_slice(&[0; 12]); // extra, comment, disk, internal attrs, external attrs
        c.extend_from_slice(&self.pos.to_le_bytes());
        c.extend_from_slice(n);
        self.pos += (h.len() + data.len()) as u32;
        self.count += 1;
        Ok(())
    }

    fn finish(mut self) -> std::io::Result<()> {
        self.out.write_all(&self.central)?;
        let mut e = Vec::new();
        e.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        e.extend_from_slice(&[0; 4]);
        e.extend_from_slice(&self.count.to_le_bytes());
        e.extend_from_slice(&self.count.to_le_bytes());
        e.extend_from_slice(&(self.central.len() as u32).to_le_bytes());
        e.extend_from_slice(&self.pos.to_le_bytes());
        e.extend_from_slice(&0u16.to_le_bytes());
        self.out.write_all(&e)?;
        self.out.flush()
    }
}

fn dos_time() -> (u16, u16) {
    use chrono::{Datelike, Timelike};
    let n = chrono::Local::now();
    let time = ((n.hour() << 11) | (n.minute() << 5) | (n.second() / 2)) as u16;
    let date = (((n.year() - 1980).max(0) as u32) << 9 | n.month() << 5 | n.day()) as u16;
    (time, date)
}

fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, t) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *t = c;
    }
    let mut crc = 0xFFFF_FFFFu32;
    for b in data {
        crc = table[((crc ^ *b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_and_zip_are_valid() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        let mut buf = Vec::new();
        let mut z = Zip::new(&mut buf);
        z.add("a.txt", b"hello").unwrap();
        z.add("b/c.json", b"{}").unwrap();
        z.finish().unwrap();
        let p = std::env::temp_dir().join(format!("cvzip-{}.zip", std::process::id()));
        std::fs::write(&p, &buf).unwrap();
        // `unzip -t` checks the CRCs and the directory when it's installed.
        if let Ok(o) = std::process::Command::new("unzip").arg("-t").arg(&p).output() {
            assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stdout));
        }
    }

    #[test]
    fn riot_id_is_removed() {
        let mut v = serde_json::json!({ "games": { "league": { "riot_id": "Me#EUW", "ult_key": "R" } } });
        redact(&mut v);
        assert_eq!(v["games"]["league"]["riot_id"], "(removed)");
        assert_eq!(v["games"]["league"]["ult_key"], "R");
    }
}
