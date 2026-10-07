//! One-time move from the app's old name (GameRecorder) to Clairvoyance.
//!
//! Runs at start-up before anything reads the settings. It only acts when the new settings file
//! doesn't exist yet and the old one does, so it happens once and never overwrites newer data:
//! - settings: `%APPDATA%\GameRecorder\settings.json` is copied to `%APPDATA%\Clairvoyance\`;
//! - recordings in the default folder (`Videos\GameRecorder`) are moved to `Videos\Clairvoyance`
//!   (a rename, instant on the same drive). If that isn't possible (files in use), the settings
//!   point at the old folder instead, so nothing is ever lost. A custom save folder is kept as is;
//! - app data (`%LOCALAPPDATA%\GameRecorder`: ffmpeg, PresentMon, logs) is moved or copied;
//! - "Start with Windows" is re-registered under the new name.
//! The old installation itself is left alone; the UI offers to uninstall it (`legacy_install`).

use crate::state::Paths;
use serde_json::Value;
use std::path::{Path, PathBuf};

pub const OLD_NAME: &str = "GameRecorder";

pub struct OldPaths {
    pub config_file: PathBuf,
    pub data_dir: PathBuf,
    pub default_save_dir: PathBuf,
}

/// Returns what happened, for the log (the logger isn't running yet when this is called).
pub fn run(new: &Paths, old: &OldPaths) -> Vec<String> {
    let mut log = Vec::new();
    if new.config_file.exists() || !old.config_file.exists() {
        return log;
    }
    log.push(format!("migrating from {OLD_NAME}: {}", old.config_file.display()));

    let mut settings: Value = std::fs::read_to_string(&old.config_file)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| Value::Object(Default::default()));

    // Recordings.
    let custom = settings.get("save_dir").and_then(Value::as_str).map(str::trim).unwrap_or("").to_string();
    if custom.is_empty() && old.default_save_dir.is_dir() {
        if new.default_save_dir.exists() {
            // Both exist (unusual): keep using the old folder so its games stay in the library.
            settings["save_dir"] = Value::String(old.default_save_dir.to_string_lossy().into());
            log.push(format!("{} already exists; keeping recordings in {}", new.default_save_dir.display(), old.default_save_dir.display()));
        } else {
            match std::fs::rename(&old.default_save_dir, &new.default_save_dir) {
                Ok(()) => log.push(format!("moved recordings to {}", new.default_save_dir.display())),
                Err(e) => {
                    settings["save_dir"] = Value::String(old.default_save_dir.to_string_lossy().into());
                    log.push(format!("couldn't move {} ({e}); recordings stay there", old.default_save_dir.display()));
                }
            }
        }
    }

    // App data (downloaded tools, logs).
    if old.data_dir.is_dir() && !new.data_dir.exists() {
        match std::fs::rename(&old.data_dir, &new.data_dir) {
            Ok(()) => log.push(format!("moved app data to {}", new.data_dir.display())),
            Err(e) => {
                log.push(format!("couldn't move {} ({e}); copying the tools instead", old.data_dir.display()));
                for sub in ["ffmpeg", "tools"] {
                    let n = copy_dir(&old.data_dir.join(sub), &new.data_dir.join(sub));
                    if n > 0 {
                        log.push(format!("copied {n} files from {sub}"));
                    }
                }
            }
        }
    }

    // Settings last, so a failure above can be retried on the next start.
    if let Some(dir) = new.config_file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    match serde_json::to_vec_pretty(&settings).map(|b| std::fs::write(&new.config_file, b)) {
        Ok(Ok(())) => log.push(format!("settings copied to {}", new.config_file.display())),
        Ok(Err(e)) => log.push(format!("couldn't write the new settings file: {e}")),
        Err(e) => log.push(format!("couldn't convert the old settings: {e}")),
    }

    if settings.get("start_with_windows").and_then(Value::as_bool) == Some(true) {
        if let Err(e) = crate::autostart::set(true) {
            log.push(format!("autostart: {e:#}"));
        }
    }
    log
}

/// Copies the files of a folder tree. Returns how many were copied.
fn copy_dir(from: &Path, to: &Path) -> usize {
    let Ok(rd) = std::fs::read_dir(from) else { return 0 };
    let _ = std::fs::create_dir_all(to);
    let mut n = 0;
    for e in rd.flatten() {
        let (src, dst) = (e.path(), to.join(e.file_name()));
        if src.is_dir() {
            n += copy_dir(&src, &dst);
        } else if !dst.exists() && std::fs::copy(&src, &dst).is_ok() {
            n += 1;
        }
    }
    n
}

/// The old app's uninstaller, if the old GameRecorder is still installed.
#[cfg(windows)]
pub fn legacy_uninstaller() -> Option<PathBuf> {
    use windows::core::{w, PCWSTR};
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ};
    let mut buf = [0u16; 1024];
    let mut len = (buf.len() * 2) as u32;
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\GameRecorder"),
            PCWSTR(w!("UninstallString").as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut _),
            Some(&mut len),
        )
    };
    if r.is_err() {
        return None;
    }
    let s = String::from_utf16_lossy(&buf[..(len as usize / 2).saturating_sub(1)]);
    let p = PathBuf::from(s.trim().trim_matches('"'));
    p.is_file().then_some(p)
}

#[cfg(not(windows))]
pub fn legacy_uninstaller() -> Option<PathBuf> {
    None
}

/// Runs the old app's uninstaller silently. It removes the old program, its shortcuts and its
/// autostart entry; recordings and settings are never touched by it.
pub fn remove_legacy_install() -> Result<(), String> {
    let Some(exe) = legacy_uninstaller() else { return Ok(()) };
    if crate::platform::is_process_running("GameRecorder.exe") {
        return Err("The old GameRecorder is still running. Quit it from its tray icon (right-click > Quit), then try again.".into());
    }
    let status = std::process::Command::new(&exe).arg("/S").status().map_err(|e| format!("Couldn't start the uninstaller: {e}"))?;
    if !status.success() {
        return Err(format!("The uninstaller failed ({status})."));
    }
    // The NSIS uninstaller copies itself to %TEMP% and returns at once; give it a moment.
    for _ in 0..20 {
        if legacy_uninstaller().is_none() {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(root: &Path, name: &str) -> (Paths, OldPaths) {
        let new = Paths {
            config_file: root.join("roaming").join(name).join("settings.json"),
            data_dir: root.join("local").join(name),
            log_file: root.join("local").join(name).join("logs").join("x.log"),
            default_save_dir: root.join("videos").join(name),
        };
        let old =
            OldPaths { config_file: root.join("roaming").join(OLD_NAME).join("settings.json"), data_dir: root.join("local").join(OLD_NAME), default_save_dir: root.join("videos").join(OLD_NAME) };
        (new, old)
    }

    #[test]
    fn moves_settings_recordings_and_tools_once() {
        let root = std::env::temp_dir().join(format!("cv-migrate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (new, old) = paths(&root, "Clairvoyance");
        std::fs::create_dir_all(old.config_file.parent().unwrap()).unwrap();
        std::fs::write(&old.config_file, r#"{"hotkey_clip":"F7","obs":{"port":4455}}"#).unwrap();
        std::fs::create_dir_all(old.default_save_dir.join("2026-09-30_20-00-00_League")).unwrap();
        std::fs::write(old.default_save_dir.join("2026-09-30_20-00-00_League").join("session.json"), "{}").unwrap();
        std::fs::create_dir_all(old.data_dir.join("ffmpeg")).unwrap();
        std::fs::write(old.data_dir.join("ffmpeg").join("ffmpeg.exe"), "x").unwrap();

        let log = run(&new, &old);
        assert!(!log.is_empty());
        assert!(new.config_file.exists());
        assert!(new.default_save_dir.join("2026-09-30_20-00-00_League").join("session.json").exists());
        assert!(new.data_dir.join("ffmpeg").join("ffmpeg.exe").exists());
        let s: Value = serde_json::from_str(&std::fs::read_to_string(&new.config_file).unwrap()).unwrap();
        assert_eq!(s["hotkey_clip"], "F7");
        assert!(s.get("save_dir").is_none(), "recordings moved to the new default folder");

        // Second start: nothing to do.
        assert!(run(&new, &old).is_empty());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn keeps_old_folder_when_the_new_one_exists() {
        let root = std::env::temp_dir().join(format!("cv-migrate2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (new, old) = paths(&root, "Clairvoyance");
        std::fs::create_dir_all(old.config_file.parent().unwrap()).unwrap();
        std::fs::write(&old.config_file, "{}").unwrap();
        std::fs::create_dir_all(&old.default_save_dir).unwrap();
        std::fs::create_dir_all(&new.default_save_dir).unwrap();
        run(&new, &old);
        let s: Value = serde_json::from_str(&std::fs::read_to_string(&new.config_file).unwrap()).unwrap();
        assert_eq!(s["save_dir"], old.default_save_dir.to_string_lossy().as_ref());
        std::fs::remove_dir_all(root).ok();
    }
}
