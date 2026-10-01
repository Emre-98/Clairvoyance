//! Cursor and mouse recording for the replay's input overlay. Nothing here touches the game:
//! no hooks (never `SetWindowsHookEx`), no injection, no game memory. It reads what Windows
//! offers every program:
//!
//! - the cursor position (`GetCursorPos`) polled at the chosen rate (125 / 250 / 500 Hz) by one
//!   below-normal-priority thread, woken by a high-resolution waitable timer (no change to the
//!   system timer resolution); a sample is stored only when the cursor moved;
//! - mouse buttons and the wheel through **Raw Input** (`RIDEV_INPUTSINK`, a copy of the input,
//!   it can't delay or block anything). The same thread reads them in batches on each tick with
//!   `GetRawInputBuffer`, instead of waking for every mouse report (a 1000-8000 Hz mouse would
//!   otherwise wake it thousands of times a second). Button times are the middle of the tick in
//!   which they arrived (within half a tick: 2 ms at 250 Hz);
//! - the game window's client area, frame and DPI (checked every 100 ms) and whether it's the
//!   foreground window (every tick). Nothing is recorded while it isn't focused.
//!
//! The thread is DPI-aware per monitor (v2), so every position is in physical pixels whatever the
//! scaling (100 %, 150 %...) and whatever the game's own DPI awareness.
//! Keys don't come through here: the app's existing Raw Input keyboard thread sends them to the
//! engine, which knows the game's chat state.

use super::d3d::qpc_hns;
use cv_core::input::{to_client_units, CaptureRequest, CaptureStats, Record, Rect, WindowInfo, BTN_LEFT, BTN_MIDDLE, BTN_RIGHT, BTN_X1, BTN_X2};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use windows::core::w;
use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE, HWND, POINT, RECT, WAIT_OBJECT_0};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{
    CreateWaitableTimerExW, GetCurrentThread, GetThreadTimes, SetThreadPriority, SetWaitableTimer, WaitForSingleObject, CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
    THREAD_PRIORITY_BELOW_NORMAL, TIMER_ALL_ACCESS,
};
use windows::Win32::UI::HiDpi::{GetDpiForWindow, SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::Input::{
    GetRawInputBuffer, GetRawInputData, RegisterRawInputDevices, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER, RIDEV_INPUTSINK, RIDEV_REMOVE, RID_INPUT,
    RIM_TYPEMOUSE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetCursorPos, GetForegroundWindow, GetWindowThreadProcessId, IsWindow, PeekMessageW,
    RegisterClassW, HWND_MESSAGE, MSG, PM_REMOVE, WINDOW_EX_STYLE, WINDOW_STYLE, WM_INPUT, WNDCLASSW,
};

/// A running capture; [`InputCapture::stop`] ends it and returns what it cost.
pub struct InputCapture {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<CaptureStats>>,
}

impl InputCapture {
    pub fn start(req: CaptureRequest) -> InputCapture {
        let stop = Arc::new(AtomicBool::new(false));
        let s2 = stop.clone();
        let thread = std::thread::Builder::new().name("input-capture".into()).spawn(move || run(req, s2)).ok();
        InputCapture { stop, thread }
    }

    pub fn stop(mut self) -> CaptureStats {
        self.stop.store(true, Ordering::SeqCst);
        self.thread.take().and_then(|t| t.join().ok()).unwrap_or_default()
    }
}

impl Drop for InputCapture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

fn thread_cpu_ms() -> f64 {
    let (mut c, mut e, mut k, mut u) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
    unsafe {
        if GetThreadTimes(GetCurrentThread(), &mut c, &mut e, &mut k, &mut u).is_err() {
            return 0.0;
        }
    }
    let ft = |f: FILETIME| ((f.dwHighDateTime as u64) << 32 | f.dwLowDateTime as u64) as f64 / 10_000.0;
    ft(k) + ft(u)
}

/// Client area, window frame (what window capture records) and DPI, in physical pixels.
pub fn window_info(hwnd: HWND) -> Option<WindowInfo> {
    unsafe {
        let mut rc = RECT::default();
        GetClientRect(hwnd, &mut rc).ok()?;
        let mut tl = POINT { x: rc.left, y: rc.top };
        let mut br = POINT { x: rc.right, y: rc.bottom };
        if !ClientToScreen(hwnd, &mut tl).as_bool() || !ClientToScreen(hwnd, &mut br).as_bool() {
            return None;
        }
        let client = Rect { x: tl.x, y: tl.y, w: br.x - tl.x, h: br.y - tl.y };
        let mut fr = RECT::default();
        let frame = if DwmGetWindowAttribute(hwnd, DWMWA_EXTENDED_FRAME_BOUNDS, &mut fr as *mut _ as *mut _, std::mem::size_of::<RECT>() as u32).is_ok() {
            Rect { x: fr.left, y: fr.top, w: fr.right - fr.left, h: fr.bottom - fr.top }
        } else {
            client
        };
        Some(WindowInfo { client, frame, dpi: GetDpiForWindow(hwnd) })
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: windows::Win32::Foundation::WPARAM, lp: windows::Win32::Foundation::LPARAM) -> windows::Win32::Foundation::LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

struct Mouse {
    hwnd: HWND,
    buf: Vec<u64>,
    msgs: u64,
}

impl Mouse {
    fn register() -> Option<Mouse> {
        unsafe {
            let hinst = GetModuleHandleW(None).ok()?;
            let class = w!("ClairvoyanceInputCapture");
            let wc = WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: hinst.into(), lpszClassName: class, ..Default::default() };
            RegisterClassW(&wc);
            let hwnd = CreateWindowExW(WINDOW_EX_STYLE(0), class, w!(""), WINDOW_STYLE(0), 0, 0, 0, 0, Some(HWND_MESSAGE), None, Some(hinst.into()), None).ok()?;
            let dev = RAWINPUTDEVICE { usUsagePage: 0x01, usUsage: 0x02, dwFlags: RIDEV_INPUTSINK, hwndTarget: hwnd };
            if let Err(e) = RegisterRawInputDevices(&[dev], std::mem::size_of::<RAWINPUTDEVICE>() as u32) {
                log::warn!("input capture: raw mouse input: {e}");
                let _ = DestroyWindow(hwnd);
                return None;
            }
            Some(Mouse { hwnd, buf: vec![0u64; 2048], msgs: 0 })
        }
    }

    /// Reads every pending mouse report: (button, down) changes and wheel deltas.
    fn drain(&mut self, out: &mut Vec<MouseEv>) {
        let hdr = std::mem::size_of::<RAWINPUTHEADER>() as u32;
        unsafe {
            loop {
                let mut cb = (self.buf.len() * 8) as u32;
                let n = GetRawInputBuffer(Some(self.buf.as_mut_ptr() as *mut RAWINPUT), &mut cb, hdr);
                if n == 0 || n == u32::MAX {
                    break;
                }
                let mut p = self.buf.as_ptr() as *const u8;
                for _ in 0..n {
                    let ri = &*(p as *const RAWINPUT);
                    handle(ri, out);
                    self.msgs += 1;
                    // NEXTRAWINPUTBLOCK: 8-byte aligned on 64-bit.
                    let next = p as usize + ri.header.dwSize as usize;
                    p = ((next + 7) & !7) as *const u8;
                }
            }
            // Leftover WM_INPUT messages (keeps the queue empty either way).
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, Some(self.hwnd), WM_INPUT, WM_INPUT, PM_REMOVE).as_bool() {
                let mut ri = RAWINPUT::default();
                let mut size = std::mem::size_of::<RAWINPUT>() as u32;
                let r = GetRawInputData(HRAWINPUT(msg.lParam.0 as *mut _), RID_INPUT, Some(&mut ri as *mut _ as *mut _), &mut size, hdr);
                if r != u32::MAX && r != 0 {
                    handle(&ri, out);
                    self.msgs += 1;
                }
            }
        }
    }
}

impl Drop for Mouse {
    fn drop(&mut self) {
        unsafe {
            let dev = RAWINPUTDEVICE { usUsagePage: 0x01, usUsage: 0x02, dwFlags: RIDEV_REMOVE, hwndTarget: HWND::default() };
            let _ = RegisterRawInputDevices(&[dev], std::mem::size_of::<RAWINPUTDEVICE>() as u32);
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

enum MouseEv {
    Button(u8, bool),
    Wheel(i32),
}

fn handle(ri: &RAWINPUT, out: &mut Vec<MouseEv>) {
    if ri.header.dwType != RIM_TYPEMOUSE.0 {
        return;
    }
    let (flags, data) = unsafe {
        let m = ri.data.mouse;
        (m.Anonymous.Anonymous.usButtonFlags, m.Anonymous.Anonymous.usButtonData)
    };
    if flags == 0 {
        return; // movement only
    }
    for (down, up, b) in [(0x1, 0x2, BTN_LEFT), (0x4, 0x8, BTN_RIGHT), (0x10, 0x20, BTN_MIDDLE), (0x40, 0x80, BTN_X1), (0x100, 0x200, BTN_X2)] {
        if flags & down != 0 {
            out.push(MouseEv::Button(b, true));
        }
        if flags & up != 0 {
            out.push(MouseEv::Button(b, false));
        }
    }
    if flags & 0x400 != 0 {
        out.push(MouseEv::Wheel(data as i16 as i32));
    }
}

struct Timer(HANDLE, bool);

impl Timer {
    fn new() -> Option<Timer> {
        unsafe {
            if let Ok(h) = CreateWaitableTimerExW(None, None, CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, TIMER_ALL_ACCESS.0) {
                return Some(Timer(h, true));
            }
            // Before Windows 10 1803: a normal timer (~15.6 ms steps).
            CreateWaitableTimerExW(None, None, 0, TIMER_ALL_ACCESS.0).ok().map(|h| Timer(h, false))
        }
    }
    fn wait(&self, hns: i64) {
        unsafe {
            let due = -hns;
            if SetWaitableTimer(self.0, &due, 0, None, None, false).is_ok() {
                if WaitForSingleObject(self.0, 1000) != WAIT_OBJECT_0 {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
            } else {
                std::thread::sleep(std::time::Duration::from_nanos(hns as u64 * 100));
            }
        }
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

fn pid_of(h: HWND) -> u32 {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
    pid
}

fn run(req: CaptureRequest, stop: Arc<AtomicBool>) -> CaptureStats {
    unsafe {
        let _ = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
    }
    let w = req.writer;
    let rate = w.rate().clamp(60, 1000);
    let interval = 10_000_000 / rate as i64;
    let Some(timer) = Timer::new() else {
        log::warn!("input capture: no timer");
        return CaptureStats::default();
    };
    if !timer.1 {
        log::warn!("input capture: high-resolution timer not available (Windows older than 10 1803); ~64 Hz");
    }
    let mut mouse = Mouse::register();
    let cpu0 = thread_cpu_ms();
    let wall0 = std::time::Instant::now();
    let mut st = CaptureStats::default();
    let exe = req.process_names.first().cloned().unwrap_or_default();
    let mut game: Option<(HWND, u32)> = None;
    let mut info: Option<WindowInfo> = None;
    let mut focused = false;
    let mut last_pt: Option<(i32, i32)> = None;
    let mut next_find = 0i64;
    let mut next_rect = 0i64;
    let mut prev_t = w.video_us(qpc_hns());
    let mut evs = Vec::with_capacity(64);
    log::info!("input capture: {rate} Hz, raw mouse {}", if mouse.is_some() { "on" } else { "off" });
    while !stop.load(Ordering::Relaxed) {
        timer.wait(interval);
        st.ticks += 1;
        let now = qpc_hns();
        let t = w.video_us(now);
        // The game window: looked up every 2 s until found, then kept while it exists.
        if game.is_some_and(|(h, _)| unsafe { !IsWindow(Some(h)).as_bool() }) {
            game = None;
            info = None;
        }
        if game.is_none() && now >= next_find {
            next_find = now + 20_000_000;
            game = super::window::find_window(&exe).map(|h| (h, pid_of(h)));
            next_rect = 0;
        }
        evs.clear();
        if let Some(m) = mouse.as_mut() {
            m.drain(&mut evs);
        }
        let Some((hwnd, pid)) = game else {
            if focused {
                focused = false;
                w.push(Record::Focus { t, focused: false });
            }
            prev_t = t;
            continue;
        };
        // Window position / size / DPI (every 100 ms).
        if now >= next_rect {
            next_rect = now + 1_000_000;
            if let Some(i) = window_info(hwnd) {
                if info != Some(i) {
                    info = Some(i);
                    w.push(Record::Window { t, info: i });
                    last_pt = None; // re-sample against the new client area
                }
            }
        }
        let fg = unsafe { GetForegroundWindow() };
        let is_focused = !fg.is_invalid() && (fg == hwnd || pid_of(fg) == pid) && info.is_some();
        if is_focused != focused {
            focused = is_focused;
            w.push(Record::Focus { t, focused });
            last_pt = None;
        }
        if focused {
            // Buttons/wheel that arrived during this tick: the middle of it.
            let tb = (prev_t + t) / 2;
            for e in evs.drain(..) {
                match e {
                    MouseEv::Button(button, down) => {
                        st.buttons += down as u64;
                        w.push(Record::Button { t: tb, button, down });
                    }
                    MouseEv::Wheel(delta) => w.push(Record::Wheel { t: tb, delta }),
                }
            }
            let mut p = POINT::default();
            if unsafe { GetCursorPos(&mut p) }.is_ok() && last_pt != Some((p.x, p.y)) {
                last_pt = Some((p.x, p.y));
                let (x, y) = to_client_units(p.x, p.y, info.unwrap().client);
                w.push(Record::Cursor { t, x, y });
                st.cursor_samples += 1;
            }
        }
        w.flush_if_due(t);
        prev_t = t;
    }
    if let Some(m) = &mouse {
        st.raw_mouse_msgs = m.msgs;
    }
    drop(mouse.take());
    if focused {
        w.push(Record::Focus { t: w.video_us(qpc_hns()), focused: false });
    }
    st.wall_secs = wall0.elapsed().as_secs_f64();
    st.cpu_ms = thread_cpu_ms() - cpu0;
    st
}
