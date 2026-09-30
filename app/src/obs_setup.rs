//! Finding OBS, turning on its WebSocket server, and launching it.

use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct ObsInfo {
    pub installed: bool,
    pub exe_path: Option<String>,
    pub running: bool,
    pub websocket_enabled: bool,
    pub websocket_port: u16,
    pub websocket_auth: bool,
    /// True if we could read OBS's WebSocket settings file.
    pub config_found: bool,
}

fn appdata() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(PathBuf::from)
}

pub fn websocket_config_path() -> Option<PathBuf> {
    appdata().map(|a| a.join("obs-studio").join("plugin_config").join("obs-websocket").join("config.json"))
}

pub fn find_obs_exe(configured: Option<&str>) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(c) = configured.filter(|c| !c.is_empty()) {
        candidates.push(c.into());
    }
    #[cfg(windows)]
    if let Some(dir) = registry_install_dir() {
        candidates.push(Path::new(&dir).join("bin").join("64bit").join("obs64.exe"));
    }
    for base in [std::env::var_os("ProgramFiles"), std::env::var_os("ProgramW6432"), Some("C:\\Program Files".into())].into_iter().flatten() {
        candidates.push(Path::new(&base).join("obs-studio").join("bin").join("64bit").join("obs64.exe"));
    }
    if let Some(pf86) = std::env::var_os("ProgramFiles(x86)") {
        candidates.push(Path::new(&pf86).join("Steam").join("steamapps").join("common").join("OBS Studio").join("bin").join("64bit").join("obs64.exe"));
    }
    candidates.into_iter().find(|p| p.is_file())
}

#[cfg(windows)]
fn registry_install_dir() -> Option<String> {
    use windows::core::w;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};
    let mut buf = [0u16; 520];
    let mut len = (buf.len() * 2) as u32;
    unsafe {
        let r = RegGetValueW(HKEY_LOCAL_MACHINE, w!("SOFTWARE\\OBS Studio"), None, RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr() as *mut _), Some(&mut len));
        if r.is_err() {
            return None;
        }
    }
    let n = (len as usize / 2).saturating_sub(1);
    Some(String::from_utf16_lossy(&buf[..n.min(buf.len())])).filter(|s| !s.is_empty())
}

pub fn global_ini_path() -> Option<PathBuf> {
    appdata().map(|a| a.join("obs-studio").join("global.ini"))
}

/// Reads `[section] key=value` pairs from an INI text.
pub fn ini_section(text: &str, section: &str) -> Vec<(String, String)> {
    let mut inside = false;
    let mut out = Vec::new();
    for line in text.lines() {
        let l = line.trim().trim_start_matches('\u{feff}');
        if l.starts_with('[') {
            inside = l.trim_matches(|c| c == '[' || c == ']').eq_ignore_ascii_case(section);
        } else if inside {
            if let Some((k, v)) = l.split_once('=') {
                out.push((k.trim().to_string(), v.trim().to_string()));
            }
        }
    }
    out
}

/// Sets keys in one INI section, keeping everything else as it was.
pub fn ini_set(text: &str, section: &str, values: &[(&str, String)]) -> String {
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let header = lines.iter().position(|l| l.trim().trim_start_matches('\u{feff}').eq_ignore_ascii_case(&format!("[{section}]")));
    let start = match header {
        Some(h) => h,
        None => {
            if lines.last().is_some_and(|l| !l.trim().is_empty()) {
                lines.push(String::new());
            }
            lines.push(format!("[{section}]"));
            lines.len() - 1
        }
    };
    let end = lines.iter().enumerate().skip(start + 1).find(|(_, l)| l.trim().starts_with('[')).map(|(i, _)| i).unwrap_or(lines.len());
    let mut insert_at = end;
    // Don't insert after trailing blank lines of the section.
    while insert_at > start + 1 && lines[insert_at - 1].trim().is_empty() {
        insert_at -= 1;
    }
    for (k, v) in values {
        match (start + 1..end).find(|i| lines[*i].split_once('=').is_some_and(|(key, _)| key.trim() == *k)) {
            Some(i) => lines[i] = format!("{k}={v}"),
            None => {
                lines.insert(insert_at, format!("{k}={v}"));
                insert_at += 1;
            }
        }
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

/// OBS 31+ keeps the WebSocket settings in a JSON file; OBS 28-30 in global.ini.
/// Returns them in the JSON shape either way.
pub fn read_websocket_config() -> Option<serde_json::Value> {
    if let Some(v) = websocket_config_path().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()) {
        return Some(v);
    }
    let text = std::fs::read_to_string(global_ini_path()?).ok()?;
    let kv = ini_section(&text, "OBSWebSocket");
    if kv.is_empty() {
        return None;
    }
    let get = |k: &str| kv.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone());
    let b = |k: &str| get(k).map(|v| v == "true");
    Some(serde_json::json!({
        "server_enabled": b("ServerEnabled").unwrap_or(false),
        "server_port": get("ServerPort").and_then(|p| p.parse::<u16>().ok()).unwrap_or(4455),
        "auth_required": b("AuthRequired").unwrap_or(true),
        "server_password": get("ServerPassword").unwrap_or_default(),
    }))
}

pub fn detect(configured: Option<&str>) -> ObsInfo {
    let exe = find_obs_exe(configured);
    let cfg = read_websocket_config();
    ObsInfo {
        installed: exe.is_some(),
        exe_path: exe.map(|p| p.to_string_lossy().to_string()),
        running: crate::platform::is_process_running("obs64.exe"),
        websocket_enabled: cfg.as_ref().and_then(|c| c["server_enabled"].as_bool()).unwrap_or(false),
        websocket_port: cfg.as_ref().and_then(|c| c["server_port"].as_u64()).unwrap_or(4455) as u16,
        websocket_auth: cfg.as_ref().and_then(|c| c["auth_required"].as_bool()).unwrap_or(true),
        config_found: cfg.is_some(),
    }
}

/// Password from OBS's own WebSocket config (so an existing setup just works).
pub fn existing_password() -> Option<String> {
    read_websocket_config().and_then(|c| c["server_password"].as_str().map(str::to_string)).filter(|s| !s.is_empty())
}

fn random_password() -> String {
    use std::hash::{BuildHasher, Hasher};
    // RandomState is seeded from the OS random source.
    let mut seed = std::collections::hash_map::RandomState::new().build_hasher().finish() | 1;
    let chars = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789";
    (0..20)
        .map(|_| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            chars[(seed % chars.len() as u64) as usize] as char
        })
        .collect()
}

/// Turns on OBS's WebSocket server with a password. OBS must be closed, because it
/// rewrites this file when it exits. Returns (port, password).
pub fn enable_websocket() -> anyhow::Result<(u16, String)> {
    if crate::platform::is_process_running("obs64.exe") {
        anyhow::bail!("Please close OBS first, then try again.");
    }
    let path = websocket_config_path().ok_or_else(|| anyhow::anyhow!("APPDATA not set"))?;
    let mut cfg = read_websocket_config().unwrap_or_else(|| serde_json::json!({}));
    let password = cfg["server_password"].as_str().filter(|s| s.len() >= 6).map(str::to_string).unwrap_or_else(random_password);
    let port = cfg["server_port"].as_u64().unwrap_or(4455) as u16;
    cfg["server_enabled"] = true.into();
    cfg["server_port"] = port.into();
    cfg["auth_required"] = true.into();
    cfg["server_password"] = password.clone().into();
    cfg["alerts_enabled"] = false.into();
    cfg["first_load"] = false.into();
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(&path, serde_json::to_vec_pretty(&cfg)?)?;
    // Older OBS (28-30) reads the same settings from global.ini.
    if let Some(ini) = global_ini_path().filter(|p| p.exists()) {
        let text = std::fs::read_to_string(&ini).unwrap_or_default();
        let new = ini_set(
            &text,
            "OBSWebSocket",
            &[
                ("FirstLoad", "false".into()),
                ("ServerEnabled", "true".into()),
                ("ServerPort", port.to_string()),
                ("AlertsEnabled", "false".into()),
                ("AuthRequired", "true".into()),
                ("ServerPassword", password.clone()),
            ],
        );
        std::fs::write(&ini, new)?;
    }
    Ok((port, password))
}

/// Starts OBS minimized to the tray. Returns false if it can't be found.
pub fn launch(exe: &Path) -> bool {
    let dir = exe.parent().unwrap_or(Path::new("."));
    let mut cmd = std::process::Command::new(exe);
    cmd.current_dir(dir).args(["--minimize-to-tray", "--disable-shutdown-check"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        cmd.creation_flags(DETACHED_PROCESS);
    }
    match cmd.spawn() {
        Ok(_) => true,
        Err(e) => {
            log::warn!("launching OBS failed: {e}");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ini_roundtrip() {
        let text = "[General]\nFoo=1\n\n[OBSWebSocket]\nServerEnabled=false\nServerPort=4455\n\n[Other]\nX=y\n";
        let out = ini_set(text, "OBSWebSocket", &[("ServerEnabled", "true".into()), ("ServerPassword", "abc".into())]);
        let kv = ini_section(&out, "OBSWebSocket");
        assert!(kv.contains(&("ServerEnabled".into(), "true".into())));
        assert!(kv.contains(&("ServerPassword".into(), "abc".into())));
        assert!(kv.contains(&("ServerPort".into(), "4455".into())));
        assert_eq!(ini_section(&out, "Other"), vec![("X".to_string(), "y".to_string())]);
        let fresh = ini_set("", "OBSWebSocket", &[("A", "1".into())]);
        assert_eq!(ini_section(&fresh, "OBSWebSocket"), vec![("A".to_string(), "1".to_string())]);
    }
}
