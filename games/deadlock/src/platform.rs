//! The few Windows calls this module needs: Steam's folder from the registry, whether the game
//! runs, and its windows' titles. Nothing touches the game process: the process list and the
//! window titles are what the task bar reads too. Other OSes (tests, dev builds) get stubs.

use std::path::PathBuf;

/// One top-level window of the game.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct WindowInfo {
    pub title: String,
    pub class: String,
    pub width: i32,
    pub height: i32,
}

#[cfg(windows)]
mod imp {
    use super::WindowInfo;

    use std::path::PathBuf;
    use windows::core::{w, BOOL};
    use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, RECT};
    use windows::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS};
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ};
    use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetClassNameW, GetForegroundWindow, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible};

    pub fn steam_path() -> Option<PathBuf> {
        let mut buf = [0u16; 520];
        let mut len = (buf.len() * 2) as u32;
        let ok = unsafe { RegGetValueW(HKEY_CURRENT_USER, w!("Software\\Valve\\Steam"), w!("SteamPath"), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr() as *mut _), Some(&mut len)).is_ok() };
        if !ok {
            return None;
        }
        let n = (len as usize / 2).saturating_sub(1).min(buf.len());
        let s = String::from_utf16_lossy(&buf[..n]);
        (!s.is_empty()).then(|| PathBuf::from(s.replace('/', "\\")))
    }

    pub fn pids(names: &[&str]) -> Vec<u32> {
        let mut out = Vec::new();
        unsafe {
            let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return out };
            let mut e = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
            if Process32FirstW(snap, &mut e).is_ok() {
                loop {
                    let end = e.szExeFile.iter().position(|c| *c == 0).unwrap_or(e.szExeFile.len());
                    let exe = String::from_utf16_lossy(&e.szExeFile[..end]);
                    if names.iter().any(|n| exe.eq_ignore_ascii_case(n)) {
                        out.push(e.th32ProcessID);
                    }
                    if Process32NextW(snap, &mut e).is_err() {
                        break;
                    }
                }
            }
            let _ = CloseHandle(snap);
        }
        out
    }

    struct Search<'a> {
        pids: &'a [u32],
        found: Vec<WindowInfo>,
    }

    unsafe extern "system" fn enum_proc(hwnd: HWND, lp: LPARAM) -> BOOL {
        let s = unsafe { &mut *(lp.0 as *mut Search) };
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        if s.pids.contains(&pid) && unsafe { IsWindowVisible(hwnd).as_bool() } {
            let mut title = [0u16; 512];
            let mut class = [0u16; 256];
            let t = unsafe { GetWindowTextW(hwnd, &mut title) }.max(0) as usize;
            let c = unsafe { GetClassNameW(hwnd, &mut class) }.max(0) as usize;
            let mut r = RECT::default();
            let _ = unsafe { GetWindowRect(hwnd, &mut r) };
            s.found.push(WindowInfo {
                title: String::from_utf16_lossy(&title[..t.min(title.len())]),
                class: String::from_utf16_lossy(&class[..c.min(class.len())]),
                width: r.right - r.left,
                height: r.bottom - r.top,
            });
        }
        BOOL(1)
    }

    pub fn windows_of(pids: &[u32]) -> Vec<WindowInfo> {
        let mut s = Search { pids, found: Vec::new() };
        unsafe {
            let _ = EnumWindows(Some(enum_proc), LPARAM(&mut s as *mut Search as isize));
        }
        s.found.sort();
        s.found
    }

    pub fn foreground_is(pids: &[u32]) -> bool {
        let mut pid = 0u32;
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.0.is_null() {
                return false;
            }
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
        }
        pids.contains(&pid)
    }
}

#[cfg(not(windows))]
mod imp {
    use super::WindowInfo;

    use std::path::PathBuf;

    pub fn steam_path() -> Option<PathBuf> {
        None
    }
    pub fn pids(_names: &[&str]) -> Vec<u32> {
        Vec::new()
    }
    pub fn windows_of(_pids: &[u32]) -> Vec<WindowInfo> {
        Vec::new()
    }
    pub fn foreground_is(_pids: &[u32]) -> bool {
        false
    }
}

/// Steam's install folder from the registry (`HKCU\Software\Valve\Steam`, `SteamPath`).
pub fn steam_path() -> Option<PathBuf> {
    imp::steam_path()
}

/// Process ids whose executable is one of `names` (any case).
pub fn pids(names: &[&str]) -> Vec<u32> {
    imp::pids(names)
}

/// The visible top-level windows of those processes, sorted.
pub fn windows_of(pids: &[u32]) -> Vec<WindowInfo> {
    imp::windows_of(pids)
}

/// Whether the focused window belongs to one of those processes.
pub fn foreground_is(pids: &[u32]) -> bool {
    imp::foreground_is(pids)
}
