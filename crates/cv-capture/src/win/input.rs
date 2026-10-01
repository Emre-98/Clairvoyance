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
    MsgWaitForMultipleObjectsEx, MWMO_INPUTAVAILABLE, QS_RAWINPUT,
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

/// CPU cycles this thread has used (exact, unlike the tick-sampled `GetThreadTimes`, which
/// over-charges a thread that wakes often for a few microseconds).
fn thread_cycles() -> u64 {
    let mut c = 0u64;
    unsafe {
        let _ = windows::Win32::System::WindowsProgramming::QueryThreadCycleTime(GetCurrentThread(), &mut c);
    }
    c
}

fn tsc() -> u64 {
    unsafe { core::arch::x86_64::_rdtsc() }
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
    leftover: u64,
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
            Some(Mouse { hwnd, buf: vec![0u64; 2048], msgs: 0, leftover: 0 })
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
                let full = (n as usize) * 48 > self.buf.len() * 8 / 2;
                let mut p = self.buf.as_ptr() as *const u8;
                for _ in 0..n {
                    let ri = &*(p as *const RAWINPUT);
                    handle(ri, out);
                    self.msgs += 1;
                    // NEXTRAWINPUTBLOCK: 8-byte aligned on 64-bit.
                    let next = p as usize + ri.header.dwSize as usize;
                    p = ((next + 7) & !7) as *const u8;
                }
                // Only ask again when the buffer may have been too small for everything.
                if !full {
                    break;
                }
            }
        }
    }

    /// WM_INPUT messages GetRawInputBuffer didn't take (keeps the queue empty either way).
    fn sweep(&mut self, out: &mut Vec<MouseEv>) {
        let hdr = std::mem::size_of::<RAWINPUTHEADER>() as u32;
        unsafe {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, Some(self.hwnd), WM_INPUT, WM_INPUT, PM_REMOVE).as_bool() {
                let mut ri = RAWINPUT::default();
                let mut size = std::mem::size_of::<RAWINPUT>() as u32;
                let r = GetRawInputData(HRAWINPUT(msg.lParam.0 as *mut _), RID_INPUT, Some(&mut ri as *mut _ as *mut _), &mut size, hdr);
                if r != u32::MAX && r != 0 {
                    handle(&ri, out);
                    self.msgs += 1;
                    self.leftover += 1;
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
    /// Periodic: one syscall per tick (the wait), no re-arming, no drift.
    fn start(&self, period_ms: i32) -> bool {
        let due = -(period_ms as i64) * 10_000;
        unsafe { SetWaitableTimer(self.0, &due, period_ms, None, None, false).is_ok() }
    }
    fn wait(&self) {
        unsafe {
            if WaitForSingleObject(self.0, 1000) != WAIT_OBJECT_0 {
                std::thread::sleep(std::time::Duration::from_millis(1));
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

/// Per-section time (TSC cycles) inside the loop, for the cost report.
#[derive(Default)]
struct Sections {
    drain: u64,
    focus: u64,
    cursor: u64,
    window: u64,
    write: u64,
}

fn run(req: CaptureRequest, stop: Arc<AtomicBool>) -> CaptureStats {
    unsafe {
        let _ = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
    }
    let w = req.writer;
    let rate = w.rate().clamp(60, 1000);
    // Whole milliseconds (the periodic timer's unit): 125 Hz = 8 ms, 250 Hz = 4 ms, 500 Hz = 2 ms.
    let period_ms = (1000 / rate as i32).max(1);
    let Some(timer) = Timer::new().filter(|t| t.start(period_ms)) else {
        log::warn!("input capture: no timer");
        return CaptureStats::default();
    };
    if !timer.1 {
        log::warn!("input capture: high-resolution timer not available (Windows older than 10 1803); ~64 Hz");
    }
    let mut mouse = Mouse::register();
    let cpu0 = thread_cpu_ms();
    let cyc0 = thread_cycles();
    let (tsc0, qpc0) = (tsc(), qpc_hns());
    let wall0 = std::time::Instant::now();
    let mut st = CaptureStats::default();
    let mut sec = Sections::default();
    let exe = req.process_names.first().cloned().unwrap_or_default();
    let mut game: Option<(HWND, u32)> = None;
    let mut info: Option<WindowInfo> = None;
    let mut focused = false;
    let mut last_pt: Option<(i32, i32)> = None;
    let mut next_find = 0i64;
    let mut next_rect = 0i64;
    let mut next_sweep = 0i64;
    let mut prev_t = w.video_us(qpc_hns());
    // Active = the mouse moved recently: sample at the chosen rate. Otherwise the thread sleeps
    // until Raw Input says the mouse moved (or 100 ms pass, for focus and window checks).
    let mut quiet_until = 0i64;
    let mut evs = Vec::with_capacity(64);
    log::info!("input capture: {rate} Hz, raw mouse {}", if mouse.is_some() { "on" } else { "off" });
    while !stop.load(Ordering::Relaxed) {
        let now0 = qpc_hns();
        let active = focused && now0 < quiet_until;
        let mut woke_by_input = false;
        if active {
            timer.wait();
            st.active_ticks += 1;
        } else if focused && mouse.is_some() {
            // Mouse idle in the game: wake on the next raw mouse report.
            woke_by_input = unsafe { MsgWaitForMultipleObjectsEx(None, 100, QS_RAWINPUT, MWMO_INPUTAVAILABLE) } == WAIT_OBJECT_0;
            st.idle_wakes += 1;
        } else {
            // Not in the game window (or no Raw Input): look every 100 ms.
            std::thread::sleep(std::time::Duration::from_millis(100));
            st.idle_wakes += 1;
        }
        st.ticks += 1;
        let now = qpc_hns();
        let t = w.video_us(now);

        let c0 = tsc();
        evs.clear();
        let before = mouse.as_ref().map(|m| m.msgs).unwrap_or(0);
        if let Some(m) = mouse.as_mut() {
            m.drain(&mut evs);
            // Messages left after GetRawInputBuffer: swept once a second.
            if now >= next_sweep || (woke_by_input && m.msgs == before) {
                next_sweep = now + 10_000_000;
                m.sweep(&mut evs);
            }
        }
        let got_input = mouse.as_ref().map(|m| m.msgs).unwrap_or(0) > before;
        if got_input {
            quiet_until = now + 2_500_000; // stay active 250 ms after the last mouse report
        }
        let c1 = tsc();
        sec.drain += c1 - c0;

        // The game window: looked up every 2 s until found; its rect/DPI every 500 ms.
        if game.is_none() && now >= next_find {
            next_find = now + 20_000_000;
            game = super::window::find_window(&exe).map(|h| (h, pid_of(h)));
            next_rect = 0;
        }
        let Some((hwnd, pid)) = game else {
            if focused {
                focused = false;
                w.push(Record::Focus { t, focused: false });
            }
            prev_t = t;
            continue;
        };
        if now >= next_rect {
            next_rect = now + 5_000_000;
            let c2 = tsc();
            if unsafe { !IsWindow(Some(hwnd)).as_bool() } {
                game = None;
                info = None;
                continue;
            }
            if let Some(i) = window_info(hwnd) {
                if info != Some(i) {
                    info = Some(i);
                    w.push(Record::Window { t, info: i });
                    last_pt = None; // re-sample against the new client area
                }
            }
            sec.window += tsc() - c2;
        }
        let c3 = tsc();
        let fg = unsafe { GetForegroundWindow() };
        let is_focused = !fg.is_invalid() && (fg == hwnd || pid_of(fg) == pid) && info.is_some();
        if is_focused != focused {
            focused = is_focused;
            w.push(Record::Focus { t, focused });
            last_pt = None;
            quiet_until = now + 2_500_000;
            next_rect = 0; // the window may have changed while it wasn't in front
        }
        let c4 = tsc();
        sec.focus += c4 - c3;
        if focused {
            // Buttons/wheel that arrived since the last tick: the middle of the interval.
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
                quiet_until = quiet_until.max(now + 2_500_000);
            }
        }
        let c5 = tsc();
        sec.cursor += c5 - c4;
        w.flush_if_due(t);
        sec.write += tsc() - c5;
        prev_t = t;
    }
    if let Some(m) = &mouse {
        st.raw_mouse_msgs = m.msgs;
        st.leftover_msgs = m.leftover;
    }
    drop(mouse.take());
    if focused {
        w.push(Record::Focus { t: w.video_us(qpc_hns()), focused: false });
    }
    st.wall_secs = wall0.elapsed().as_secs_f64();
    st.cpu_ms_sampled = thread_cpu_ms() - cpu0;
    // Cycles -> ms with the TSC rate measured over the run (QueryThreadCycleTime counts TSC ticks).
    let hz = (tsc() - tsc0) as f64 / ((qpc_hns() - qpc0) as f64 / 1e7);
    let ms = |c: u64| if hz > 0.0 { c as f64 / hz * 1000.0 } else { 0.0 };
    st.cpu_ms = if hz > 0.0 { ms(thread_cycles() - cyc0) } else { st.cpu_ms_sampled };
    st.sections_ms = vec![
        ("raw input".into(), ms(sec.drain)),
        ("foreground".into(), ms(sec.focus)),
        ("cursor".into(), ms(sec.cursor)),
        ("window".into(), ms(sec.window)),
        ("write".into(), ms(sec.write)),
    ];
    st
}
