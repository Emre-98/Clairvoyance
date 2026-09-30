//! Keyboard input for hotkeys and the ult key, using Windows Raw Input.
//!
//! Raw Input is not a hook: the app gets a copy of key presses and can't delay or
//! block them, so it can't add input lag. It doesn't touch the game process at all
//! (Vanguard-safe). It's only registered while a game is running.

use gr_core::engine::InputEvent;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;

#[derive(Clone, Default)]
pub struct InputControl {
    hwnd: Arc<AtomicIsize>,
}

impl InputControl {
    pub fn set_enabled(&self, on: bool) {
        #[cfg(windows)]
        win::set_enabled(self.hwnd.load(Ordering::SeqCst), on);
        #[cfg(not(windows))]
        let _ = (on, self.hwnd.load(Ordering::SeqCst));
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
pub fn key_name(vk: u16) -> Option<String> {
    Some(match vk {
        0x41..=0x5A | 0x30..=0x39 => (vk as u8 as char).to_string(),
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        0x60..=0x69 => format!("Num{}", vk - 0x60),
        0x0D => "Enter".into(),
        0x1B => "Escape".into(),
        0x20 => "Space".into(),
        0x09 => "Tab".into(),
        0x08 => "Backspace".into(),
        0x2D => "Insert".into(),
        0x2E => "Delete".into(),
        0x24 => "Home".into(),
        0x23 => "End".into(),
        0x21 => "Pageup".into(),
        0x22 => "Pagedown".into(),
        0x13 => "Pause".into(),
        0x2C => "Printscreen".into(),
        0xC0 => "`".into(),
        0x10 | 0x11 | 0x12 | 0xA0..=0xA5 | 0x5B | 0x5C | 0xFF | 0 => return None,
        other => format!("Key{other}"),
    })
}

pub fn start(tx: UnboundedSender<InputEvent>) -> InputControl {
    let ctl = InputControl::default();
    #[cfg(windows)]
    {
        let hwnd = ctl.hwnd.clone();
        std::thread::Builder::new().name("raw-input".into()).spawn(move || win::run(tx, hwnd)).ok();
    }
    #[cfg(not(windows))]
    drop(tx);
    ctl
}

#[cfg(windows)]
mod win {
    use super::*;
    use gr_core::game::KeyPress;
    use std::cell::RefCell;
    use std::collections::HashSet;
    use std::sync::OnceLock;
    use std::time::Instant;
    use windows::core::w;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Input::{
        GetRawInputData, RegisterRawInputDevices, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER, RIDEV_INPUTSINK, RIDEV_REMOVE, RID_INPUT,
        RIM_TYPEKEYBOARD,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, PostMessageW, RegisterClassW, HWND_MESSAGE, MSG, WINDOW_EX_STYLE,
        WINDOW_STYLE, WM_APP, WM_INPUT, WNDCLASSW,
    };

    const WM_SET_ENABLED: u32 = WM_APP + 1;
    static SENDER: OnceLock<UnboundedSender<InputEvent>> = OnceLock::new();

    #[derive(Default)]
    struct KeyState {
        down: HashSet<u16>,
        ctrl: bool,
        shift: bool,
        alt: bool,
    }
    thread_local! {
        static STATE: RefCell<KeyState> = RefCell::new(KeyState::default());
    }

    pub fn set_enabled(hwnd: isize, on: bool) {
        if hwnd == 0 {
            return;
        }
        unsafe {
            let _ = PostMessageW(Some(HWND(hwnd as *mut _)), WM_SET_ENABLED, WPARAM(on as usize), LPARAM(0));
        }
    }

    fn register(hwnd: HWND, on: bool) {
        let dev = RAWINPUTDEVICE {
            usUsagePage: 0x01,
            usUsage: 0x06, // keyboard
            dwFlags: if on { RIDEV_INPUTSINK } else { RIDEV_REMOVE },
            hwndTarget: if on { hwnd } else { HWND::default() },
        };
        unsafe {
            if let Err(e) = RegisterRawInputDevices(&[dev], std::mem::size_of::<RAWINPUTDEVICE>() as u32) {
                log::warn!("raw input register({on}): {e}");
            }
        }
        if !on {
            STATE.with(|s| *s.borrow_mut() = KeyState::default());
        }
    }

    fn handle_input(lparam: LPARAM) {
        let mut raw = RAWINPUT::default();
        let mut size = std::mem::size_of::<RAWINPUT>() as u32;
        let n = unsafe {
            GetRawInputData(HRAWINPUT(lparam.0 as *mut _), RID_INPUT, Some(&mut raw as *mut _ as *mut _), &mut size, std::mem::size_of::<RAWINPUTHEADER>() as u32)
        };
        if n == u32::MAX || n == 0 || raw.header.dwType != RIM_TYPEKEYBOARD.0 {
            return;
        }
        let kb = unsafe { raw.data.keyboard };
        let vk = kb.VKey;
        let is_down = kb.Message == 0x100 || kb.Message == 0x104; // WM_KEYDOWN / WM_SYSKEYDOWN
        let at = Instant::now();
        let press = STATE.with(|s| {
            let mut s = s.borrow_mut();
            match vk {
                0x10 | 0xA0 | 0xA1 => s.shift = is_down,
                0x11 | 0xA2 | 0xA3 => s.ctrl = is_down,
                0x12 | 0xA4 | 0xA5 => s.alt = is_down,
                _ => {}
            }
            if !is_down {
                s.down.remove(&vk);
                return None;
            }
            // Ignore auto-repeat while a key is held.
            if !s.down.insert(vk) {
                return None;
            }
            key_name(vk).map(|key| KeyPress { key, ctrl: s.ctrl, shift: s.shift, alt: s.alt })
        });
        if let (Some(key), Some(tx)) = (press, SENDER.get()) {
            let _ = tx.send(InputEvent { key, at });
        }
    }

    unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        match msg {
            WM_INPUT => {
                handle_input(lparam);
                unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
            }
            WM_SET_ENABLED => {
                register(hwnd, wparam.0 != 0);
                LRESULT(0)
            }
            _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
        }
    }

    pub fn run(tx: UnboundedSender<InputEvent>, out_hwnd: Arc<AtomicIsize>) {
        let _ = SENDER.set(tx);
        unsafe {
            let Ok(hinst) = GetModuleHandleW(None) else { return };
            let class = w!("GameRecorderRawInput");
            let wc = WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: hinst.into(), lpszClassName: class, ..Default::default() };
            RegisterClassW(&wc);
            let hwnd = match CreateWindowExW(WINDOW_EX_STYLE(0), class, w!(""), WINDOW_STYLE(0), 0, 0, 0, 0, Some(HWND_MESSAGE), None, Some(hinst.into()), None) {
                Ok(h) => h,
                Err(e) => {
                    log::warn!("raw input window: {e}");
                    return;
                }
            };
            out_hwnd.store(hwnd.0 as isize, Ordering::SeqCst);
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                DispatchMessageW(&msg);
            }
        }
    }
}
