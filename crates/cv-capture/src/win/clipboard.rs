//! Files on the clipboard, the way Explorer's Copy puts them there: Ctrl+V in Discord, a chat app
//! or a folder then attaches / copies the file.
//!
//! What Explorer sets (and what Discord / Chromium read): `CF_HDROP`, a `DROPFILES` header followed
//! by the absolute paths in UTF-16, each ended by a NUL and the list by a second NUL, plus
//! "Preferred DropEffect" = `DROPEFFECT_COPY`, so pasting into a folder copies rather than moves.

use anyhow::{bail, Context, Result};
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::time::Duration;
use windows::core::w;
use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, POINT};
use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW, SetClipboardData};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GHND};
use windows::Win32::UI::Shell::DROPFILES;
use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, HWND_MESSAGE, WINDOW_EX_STYLE, WINDOW_STYLE};

pub const CF_HDROP: u32 = 15;
const DROPEFFECT_COPY: u32 = 1;
/// Another app (a clipboard manager, a remote-desktop client) may hold the clipboard briefly.
const OPEN_TRIES: u32 = 5;
const OPEN_WAIT: Duration = Duration::from_millis(50);

/// The `CF_HDROP` block for one file: `DROPFILES`, then the path in UTF-16 and two NULs.
pub fn hdrop_bytes(path: &Path) -> Vec<u8> {
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0, 0]).collect();
    let header = DROPFILES { pFiles: std::mem::size_of::<DROPFILES>() as u32, pt: POINT { x: 0, y: 0 }, fNC: false.into(), fWide: true.into() };
    let mut out = Vec::with_capacity(std::mem::size_of::<DROPFILES>() + wide.len() * 2);
    // SAFETY: DROPFILES is plain old data (repr(C), integers only).
    out.extend_from_slice(unsafe { std::slice::from_raw_parts(&header as *const DROPFILES as *const u8, std::mem::size_of::<DROPFILES>()) });
    out.extend(wide.iter().flat_map(|c| c.to_le_bytes()));
    out
}

unsafe fn global(bytes: &[u8]) -> Result<HGLOBAL> {
    let h = unsafe { GlobalAlloc(GHND, bytes.len())? };
    let p = unsafe { GlobalLock(h) } as *mut u8;
    if p.is_null() {
        let _ = unsafe { GlobalFree(Some(h)) };
        bail!("GlobalLock failed");
    }
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len());
        let _ = GlobalUnlock(h);
    }
    Ok(h)
}

unsafe fn set(format: u32, bytes: &[u8]) -> Result<()> {
    let h = unsafe { global(bytes)? };
    // On success the clipboard owns the memory; otherwise it's still ours to free.
    if let Err(e) = unsafe { SetClipboardData(format, Some(HANDLE(h.0))) } {
        let _ = unsafe { GlobalFree(Some(h)) };
        return Err(e.into());
    }
    Ok(())
}

/// Puts the file at `path` (made absolute; it must exist) on the clipboard.
pub fn copy_file(path: &Path) -> Result<()> {
    let path = std::path::absolute(path).context("absolute path")?;
    if !path.is_file() {
        bail!("{} doesn't exist", path.display());
    }
    let drop = hdrop_bytes(&path);

    // A message-only window owns the clipboard: with no owner, EmptyClipboard leaves it ownerless
    // and SetClipboardData may fail. The data stays on the clipboard once the window is gone.
    let owner = unsafe { CreateWindowExW(WINDOW_EX_STYLE(0), w!("STATIC"), None, WINDOW_STYLE(0), 0, 0, 0, 0, Some(HWND_MESSAGE), None, None, None) }.ok();
    let mut opened = false;
    for i in 0..OPEN_TRIES {
        if unsafe { OpenClipboard(owner) }.is_ok() {
            opened = true;
            break;
        }
        if i + 1 < OPEN_TRIES {
            std::thread::sleep(OPEN_WAIT);
        }
    }
    let res = if !opened {
        Err(anyhow::anyhow!("the clipboard is busy (another app has it open)"))
    } else {
        let r = (|| -> Result<()> {
            unsafe {
                EmptyClipboard().context("EmptyClipboard")?;
                set(CF_HDROP, &drop).context("CF_HDROP")?;
                // "Copy", not "move", when it's pasted into a folder.
                let effect = RegisterClipboardFormatW(w!("Preferred DropEffect"));
                if effect == 0 {
                    bail!("RegisterClipboardFormat failed");
                }
                set(effect, &DROPEFFECT_COPY.to_le_bytes()).context("Preferred DropEffect")?;
            }
            Ok(())
        })();
        let _ = unsafe { CloseClipboard() };
        r
    };
    if let Some(h) = owner {
        let _ = unsafe { DestroyWindow(h) };
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::DataExchange::{GetClipboardData, IsClipboardFormatAvailable};
    use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};

    fn open() {
        for _ in 0..20 {
            if unsafe { OpenClipboard(None) }.is_ok() {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("couldn't open the clipboard");
    }

    #[test]
    fn copies_a_file_like_explorer() {
        let dir = std::env::temp_dir().join(format!("cv-clipboard-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // Spaces and non-ASCII: the path is UTF-16, not the ANSI code page.
        let file = dir.join("Zed pentakill é.mp4");
        std::fs::write(&file, b"not really a video").unwrap();

        copy_file(&file).unwrap();

        open();
        let got = (|| unsafe {
            IsClipboardFormatAvailable(CF_HDROP)?;
            let h = GetClipboardData(CF_HDROP)?;
            let drop = HDROP(h.0);
            assert_eq!(DragQueryFileW(drop, u32::MAX, None), 1, "one file");
            let n = DragQueryFileW(drop, 0, None) as usize;
            let mut buf = vec![0u16; n + 1];
            let m = DragQueryFileW(drop, 0, Some(&mut buf)) as usize;
            let effect = RegisterClipboardFormatW(w!("Preferred DropEffect"));
            let e = GetClipboardData(effect)?;
            let p = GlobalLock(HGLOBAL(e.0)) as *const u32;
            let effect = if p.is_null() { 0 } else { p.read_unaligned() };
            let _ = GlobalUnlock(HGLOBAL(e.0));
            Ok::<_, windows::core::Error>((String::from_utf16_lossy(&buf[..m]), effect))
        })();
        let _ = unsafe { CloseClipboard() };
        let (path, effect) = got.expect("CF_HDROP and Preferred DropEffect on the clipboard");
        assert_eq!(Path::new(&path), std::path::absolute(&file).unwrap());
        assert_eq!(effect, DROPEFFECT_COPY);
        assert!(file.is_file(), "the file is still there after the copy");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
