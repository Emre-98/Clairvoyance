//! Where Steam and Deadlock keep the files this module reads. Nothing here is ever written.

use std::path::{Path, PathBuf};

/// Deadlock's Steam app id.
pub const APP_ID: &str = "1422450";
/// Lower-case executable names (`game\bin\win64`); early builds were called project8.exe.
pub const PROCESS_NAMES: &[&str] = &["deadlock.exe", "project8.exe"];

/// One `"key"  "value"` line of a Valve text file (.vdf / .acf).
pub fn vdf_pair(line: &str) -> Option<(String, String)> {
    let mut chars = line.trim().chars();
    let mut parts = Vec::with_capacity(2);
    while parts.len() < 2 {
        let c = chars.next()?;
        if c.is_whitespace() {
            continue;
        }
        if c != '"' {
            return None;
        }
        let mut s = String::new();
        loop {
            match chars.next()? {
                '"' => break,
                '\\' => s.push(chars.next()?),
                c => s.push(c),
            }
        }
        parts.push(s);
    }
    let value = parts.pop()?;
    Some((parts.pop()?, value))
}

/// The value of `key` (any case) on the first line that has it.
pub fn vdf_value(text: &str, key: &str) -> Option<String> {
    text.lines().filter_map(vdf_pair).find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v)
}

/// Library paths from Steam's `libraryfolders.vdf`.
pub fn parse_library_folders(vdf: &str) -> Vec<PathBuf> {
    vdf.lines().filter_map(vdf_pair).filter(|(k, _)| k.eq_ignore_ascii_case("path")).map(|(_, v)| PathBuf::from(v)).collect()
}

/// An app's launch options in a Steam `localconfig.vdf`, as Steam last saved them (it writes
/// that file late, often only when it closes). `None` = none set.
pub fn launch_options(localconfig: &str, app_id: &str) -> Option<String> {
    let id = format!("\"{app_id}\"");
    // Depth inside a block that started with the app id on its own line.
    let mut depth: Option<u32> = None;
    for t in localconfig.lines().map(str::trim) {
        let Some(d) = depth else {
            if t == id {
                depth = Some(0);
            }
            continue;
        };
        if t == "{" {
            depth = Some(d + 1);
        } else if t == "}" {
            depth = if d <= 1 { None } else { Some(d - 1) };
        } else if d == 0 {
            depth = None;
        } else if d == 1 && vdf_pair(t).is_some_and(|(k, _)| k.eq_ignore_ascii_case("LaunchOptions")) {
            return vdf_pair(t).map(|(_, v)| v).filter(|v| !v.trim().is_empty());
        }
    }
    None
}

/// Steam's install folder (registry, then the default place).
pub fn steam_root() -> Option<PathBuf> {
    let default = || std::env::var_os("ProgramFiles(x86)").map(|p| PathBuf::from(p).join("Steam"));
    crate::platform::steam_path().filter(|p| p.is_dir()).or_else(default).filter(|p| p.is_dir())
}

/// Every Steam library (the Steam folder itself is one).
pub fn libraries(steam: &Path) -> Vec<PathBuf> {
    let mut libs = vec![steam.to_path_buf()];
    if let Ok(vdf) = std::fs::read_to_string(steam.join("steamapps").join("libraryfolders.vdf")) {
        for l in parse_library_folders(&vdf) {
            if !libs.iter().any(|k| k.to_string_lossy().eq_ignore_ascii_case(&l.to_string_lossy())) {
                libs.push(l);
            }
        }
    }
    libs
}

/// Deadlock's install folder (`<library>\steamapps\common\Deadlock`).
pub fn install_dir(steam: &Path) -> Option<PathBuf> {
    libraries(steam).into_iter().find_map(|lib| {
        let apps = lib.join("steamapps");
        let manifest = std::fs::read_to_string(apps.join(format!("appmanifest_{APP_ID}.acf"))).unwrap_or_default();
        let name = vdf_value(&manifest, "installdir").unwrap_or_else(|| "Deadlock".into());
        Some(apps.join("common").join(name)).filter(|d| d.join("game").is_dir())
    })
}

/// `game\citadel`: the game's own folder (console.log, cfg, replays).
pub fn citadel_dir(install: &Path) -> PathBuf {
    install.join("game").join("citadel")
}

/// Folders where the game can write a console log (`-condebug` writes `console.log`).
pub fn log_dirs(install: &Path) -> Vec<PathBuf> {
    let game = install.join("game");
    vec![game.join("citadel"), game.clone(), game.join("bin").join("win64")]
}

/// Where "Download Replay" saves its `.dem` files (with mods it can be the addons one).
pub fn replay_dirs(install: &Path) -> Vec<PathBuf> {
    let citadel = citadel_dir(install);
    vec![citadel.join("replays"), citadel.join("addons").join("replays")]
}

/// The Steam accounts that have signed in on this PC (`userdata\<account id>`).
pub fn user_dirs(steam: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(steam.join("userdata")) else { return Vec::new() };
    let numeric = |p: &PathBuf| p.file_name().is_some_and(|n| n.to_string_lossy().bytes().all(|b| b.is_ascii_digit()));
    let mut v: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_dir() && numeric(p)).collect();
    v.sort();
    v
}

/// An account's `config\localconfig.vdf` (launch options, Steam client settings).
fn localconfig(user: &Path) -> Option<String> {
    std::fs::read_to_string(user.join("config").join("localconfig.vdf")).ok()
}

/// Steam's game-recording folders (they hold `timelines\`): the default one of every account,
/// plus a folder the user chose in Steam's settings, if Steam saved one.
pub fn recording_dirs(steam: &Path) -> Vec<PathBuf> {
    let mut v = Vec::new();
    for user in user_dirs(steam) {
        v.push(user.join("gamerecordings"));
        // Unverified key name (seen in community notes only); harmless when it isn't there.
        let chosen = localconfig(&user).and_then(|t| vdf_value(&t, "BackgroundRecordPath"));
        if let Some(c) = chosen.map(PathBuf::from).filter(|c| !c.as_os_str().is_empty() && !v.contains(c)) {
            v.push(c);
        }
    }
    v
}

/// The launch options of every account that has some for Deadlock.
pub fn deadlock_launch_options(steam: &Path) -> Vec<String> {
    user_dirs(steam).iter().filter_map(|u| localconfig(u.as_path())).filter_map(|t| launch_options(&t, APP_ID)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vdf_lines() {
        assert_eq!(vdf_pair("\t\t\"path\"\t\t\"E:\\\\SteamLibrary\""), Some(("path".into(), "E:\\SteamLibrary".into())));
        assert_eq!(vdf_pair("  \"installdir\"  \"Deadlock\"  "), Some(("installdir".into(), "Deadlock".into())));
        assert_eq!(vdf_pair("\"a\" \"say \\\"hi\\\"\""), Some(("a".into(), "say \"hi\"".into())));
        assert_eq!(vdf_pair("\"apps\""), None);
        assert_eq!(vdf_pair("{"), None);
        assert_eq!(vdf_pair(""), None);
    }

    #[test]
    fn library_folders() {
        // The owner's file (2026-10-09), shortened.
        let vdf = "\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\t\"C:\\\\Program Files (x86)\\\\Steam\"\n\t\t\"apps\"\n\t\t{\n\t\t\t\"228980\"\t\t\"1\"\n\t\t}\n\t}\n\t\"2\"\n\t{\n\t\t\"path\"\t\t\"E:\\\\SteamLibrary\"\n\t\t\"apps\"\n\t\t{\n\t\t\t\"1422450\"\t\t\"38726120639\"\n\t\t}\n\t}\n}\n";
        assert_eq!(parse_library_folders(vdf), vec![PathBuf::from("C:\\Program Files (x86)\\Steam"), PathBuf::from("E:\\SteamLibrary")]);
    }

    #[test]
    fn install_dir_name_from_the_manifest() {
        let acf = "\"AppState\"\n{\n\t\"appid\"\t\t\"1422450\"\n\t\"name\"\t\t\"Deadlock\"\n\t\"installdir\"\t\t\"Deadlock\"\n}\n";
        assert_eq!(vdf_value(acf, "installdir").as_deref(), Some("Deadlock"));
        assert_eq!(vdf_value(acf, "missing"), None);
    }

    /// The shape of Steam's localconfig.vdf around an app (the owner's file, 2026-10-09).
    fn localconfig(deadlock_extra: &str) -> String {
        format!(
            "\"UserLocalConfigStore\"\n{{\n\t\"Software\"\n\t{{\n\t\t\"apps\"\n\t\t{{\n\t\t\t\"730\"\n\t\t\t{{\n\t\t\t\t\"LaunchOptions\"\t\t\"-novid\"\n\t\t\t}}\n\t\t\t\"1422450\"\n\t\t\t{{\n\t\t\t\t\"LastPlayed\"\t\t\"1791525758\"\n\t\t\t\t\"cloud\"\n\t\t\t\t{{\n\t\t\t\t\t\"last_sync_state\"\t\t\"synchronized\"\n\t\t\t\t}}\n{deadlock_extra}\t\t\t}}\n\t\t\t\"570\"\n\t\t\t{{\n\t\t\t\t\"LaunchOptions\"\t\t\"-console\"\n\t\t\t}}\n\t\t}}\n\t}}\n}}\n"
        )
    }

    #[test]
    fn launch_options_of_one_app() {
        assert_eq!(launch_options(&localconfig(""), APP_ID), None, "other apps' options don't count");
        let set = localconfig("\t\t\t\t\"LaunchOptions\"\t\t\"-condebug -novid\"\n");
        assert_eq!(launch_options(&set, APP_ID).as_deref(), Some("-condebug -novid"));
        assert_eq!(launch_options(&set, "730").as_deref(), Some("-novid"));
        assert_eq!(launch_options(&set, "570").as_deref(), Some("-console"));
        assert_eq!(launch_options(&localconfig("\t\t\t\t\"LaunchOptions\"\t\t\"\"\n"), APP_ID), None, "empty = none");
        // The id as a plain value elsewhere (not a block) doesn't start a block.
        assert_eq!(launch_options("\"x\"\n{\n\t\"1422450\"\n\t\"LaunchOptions\"\t\t\"-a\"\n}\n", APP_ID), None);
    }
}
