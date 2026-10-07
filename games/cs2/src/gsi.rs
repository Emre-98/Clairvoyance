//! Tiny HTTP endpoint that receives CS2's Game State Integration posts, plus the cfg file.

use serde_json::Value;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const CFG_NAME: &str = "gamestate_integration_clairvoyance.cfg";
/// Files written by older versions (the app used to be called GameRecorder).
const LEGACY_CFG_NAMES: &[&str] = &["gamestate_integration_gamerecorder.cfg"];

type Latest = Arc<Mutex<Option<(Value, Instant)>>>;

pub struct Server {
    pub port: u16,
    latest: Latest,
    stop: Arc<AtomicBool>,
}

impl Server {
    /// Listens on 127.0.0.1:port. Blocking accept on its own thread: no CPU while idle.
    pub fn start(port: u16, token: &str) -> anyhow::Result<Server> {
        let listener = TcpListener::bind(("127.0.0.1", port))?;
        let latest: Latest = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));
        let (l, st, tok) = (latest.clone(), stop.clone(), token.to_string());
        std::thread::Builder::new().name("cs2-gsi".into()).spawn(move || {
            for conn in listener.incoming() {
                if st.load(Ordering::SeqCst) {
                    break;
                }
                if let Ok(c) = conn {
                    let (l, tok) = (l.clone(), tok.clone());
                    std::thread::spawn(move || handle(c, &l, &tok));
                }
            }
        })?;
        Ok(Server { port, latest, stop })
    }

    /// Latest state and how old it is.
    pub fn latest(&self) -> Option<(Value, Duration)> {
        self.latest.lock().unwrap().as_ref().map(|(v, t)| (v.clone(), t.elapsed()))
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(("127.0.0.1", self.port)); // unblock accept()
    }
}

fn handle(stream: TcpStream, latest: &Latest, token: &str) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
    let mut writer = match stream.try_clone() {
        Ok(w) => w,
        Err(_) => return,
    };
    let mut reader = BufReader::new(stream);
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            return;
        }
        let mut len = 0usize;
        loop {
            let mut h = String::new();
            if reader.read_line(&mut h).unwrap_or(0) == 0 {
                return;
            }
            let h = h.trim_end();
            if h.is_empty() {
                break;
            }
            if let Some((k, v)) = h.split_once(':') {
                if k.trim().eq_ignore_ascii_case("content-length") {
                    len = v.trim().parse().unwrap_or(0);
                }
            }
        }
        let mut body = vec![0u8; len.min(4 * 1024 * 1024)];
        if reader.read_exact(&mut body).is_err() {
            return;
        }
        if let Ok(v) = serde_json::from_slice::<Value>(&body) {
            if v["auth"]["token"].as_str() == Some(token) {
                *latest.lock().unwrap() = Some((v, Instant::now()));
            }
        }
        if writer.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: keep-alive\r\n\r\n").is_err() {
            return;
        }
    }
}

pub fn cfg_contents(port: u16, token: &str) -> String {
    format!(
        r#""Clairvoyance"
{{
    "uri"       "http://127.0.0.1:{port}/"
    "timeout"   "5.0"
    "buffer"    "0.1"
    "throttle"  "0.5"
    "heartbeat" "10.0"
    "auth"
    {{
        "token" "{token}"
    }}
    "data"
    {{
        "provider"           "1"
        "map"                "1"
        "round"              "1"
        "player_id"          "1"
        "player_state"       "1"
        "player_match_stats" "1"
    }}
}}
"#
    )
}

/// Writes the cfg file if it's missing or different. Returns true if it changed.
pub fn install_cfg(dir: &Path, port: u16, token: &str) -> std::io::Result<bool> {
    if !dir.is_dir() {
        return Ok(false);
    }
    for old in LEGACY_CFG_NAMES {
        let p = dir.join(old);
        if p.exists() && std::fs::remove_file(&p).is_ok() {
            log::info!("removed old CS2 GSI config {}", p.display());
        }
    }
    let path = dir.join(CFG_NAME);
    let want = cfg_contents(port, token);
    if std::fs::read_to_string(&path).ok().as_deref() == Some(want.as_str()) {
        return Ok(false);
    }
    std::fs::write(&path, want)?;
    log::info!("installed CS2 GSI config at {}", path.display());
    Ok(true)
}

/// Library paths from Steam's libraryfolders.vdf.
pub fn parse_library_folders(vdf: &str) -> Vec<PathBuf> {
    vdf.lines()
        .filter_map(|l| {
            let l = l.trim();
            let rest = l.strip_prefix("\"path\"")?;
            let v = rest.trim().trim_matches('"');
            Some(PathBuf::from(v.replace("\\\\", "\\")))
        })
        .collect()
}

fn steam_root() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        use windows::core::w;
        use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ};
        let mut buf = [0u16; 520];
        let mut len = (buf.len() * 2) as u32;
        let ok = unsafe { RegGetValueW(HKEY_CURRENT_USER, w!("Software\\Valve\\Steam"), w!("SteamPath"), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr() as *mut _), Some(&mut len)).is_ok() };
        if ok {
            let n = (len as usize / 2).saturating_sub(1);
            let s = String::from_utf16_lossy(&buf[..n.min(buf.len())]);
            if !s.is_empty() {
                return Some(PathBuf::from(s.replace('/', "\\")));
            }
        }
    }
    std::env::var_os("ProgramFiles(x86)").map(|p| PathBuf::from(p).join("Steam")).filter(|p| p.is_dir())
}

/// CS2's cfg folder(s) in every Steam library.
pub fn find_cfg_dirs() -> Vec<PathBuf> {
    let Some(root) = steam_root() else { return Vec::new() };
    let mut libs = vec![root.clone()];
    if let Ok(vdf) = std::fs::read_to_string(root.join("steamapps").join("libraryfolders.vdf")) {
        for l in parse_library_folders(&vdf) {
            if !libs.contains(&l) {
                libs.push(l);
            }
        }
    }
    libs.into_iter()
        .map(|l| l.join("steamapps").join("common").join("Counter-Strike Global Offensive").join("game").join("csgo").join("cfg"))
        .filter(|p| p.is_dir())
        .collect()
}
