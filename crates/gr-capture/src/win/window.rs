//! Finding the game's process and window.

use windows::core::BOOL;
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, POINT, RECT};
use windows::Win32::Graphics::Gdi::{MonitorFromPoint, MonitorFromWindow, HMONITOR, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTOPRIMARY};
use windows::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS};
use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowRect, GetWindowThreadProcessId, IsWindowVisible};

/// Process ids whose executable name matches (case-insensitive).
pub fn pids_for_exe(exe: &str) -> Vec<u32> {
    let mut out = Vec::new();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return out };
        let mut e = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        if Process32FirstW(snap, &mut e).is_ok() {
            loop {
                let end = e.szExeFile.iter().position(|c| *c == 0).unwrap_or(e.szExeFile.len());
                if String::from_utf16_lossy(&e.szExeFile[..end]).eq_ignore_ascii_case(exe) {
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

struct Search {
    pids: Vec<u32>,
    best: Option<(HWND, i64)>,
}

unsafe extern "system" fn enum_proc(hwnd: HWND, lp: LPARAM) -> BOOL {
    let s = unsafe { &mut *(lp.0 as *mut Search) };
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if s.pids.contains(&pid) && unsafe { IsWindowVisible(hwnd).as_bool() } {
        let mut r = RECT::default();
        if unsafe { GetWindowRect(hwnd, &mut r) }.is_ok() {
            let area = (r.right - r.left) as i64 * (r.bottom - r.top) as i64;
            if area > 100 * 100 && s.best.is_none_or(|(_, a)| area > a) {
                s.best = Some((hwnd, area));
            }
        }
    }
    BOOL(1)
}

/// The game's main (largest visible) window.
pub fn find_window(exe: &str) -> Option<HWND> {
    let pids = pids_for_exe(exe);
    if pids.is_empty() {
        return None;
    }
    let mut s = Search { pids, best: None };
    unsafe {
        let _ = EnumWindows(Some(enum_proc), LPARAM(&mut s as *mut Search as isize));
    }
    s.best.map(|(h, _)| h)
}

/// The monitor showing the game window, or the primary monitor.
pub fn monitor_for(hwnd: Option<HWND>) -> HMONITOR {
    unsafe {
        match hwnd {
            Some(h) => MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST),
            None => MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY),
        }
    }
}

pub fn window_pid(h: HWND) -> u32 {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
    pid
}
