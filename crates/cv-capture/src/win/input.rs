//! Cursor and mouse-button recording for the replay's input overlay. Nothing here touches the
//! game: no hooks (never `SetWindowsHookEx`), no injection, no game memory. One below-normal-
//! priority thread reads what Windows offers every program:
//!
//! - the cursor position (`GetCursorPos`) at the chosen rate (125 / 250 / 500 Hz) while the
//!   cursor moves, woken by a high-resolution periodic waitable timer (no change to the system
//!   timer resolution); a sample is stored only when the cursor moved. When the mouse rests for
//!   250 ms it drops to a 30 Hz check of `GetLastInputInfo` until something moves again;
//! - the mouse buttons (L R M X1 X2) with `GetAsyncKeyState` on the same tick (the physical
//!   button state, plus the "pressed since the last call" bit so a click shorter than a tick
//!   isn't missed). A button's time is the middle of the tick it changed in.
//!   Measured on the owner's PC (2026-10-01): reading the mouse through Raw Input
//!   (`RIDEV_INPUTSINK`) costs 7.7 µs per mouse report, i.e. ~0.8 % of a core with a 1000 Hz
//!   mouse in motion, because every movement report must be read; the five button states cost
//!   0.46 µs per tick. So mouse Raw Input isn't used (the wheel isn't recorded);
//! - the game window's client area, frame and DPI (every 500 ms, and when it gets the focus back)
//!   and whether it's the foreground window (every tick). Nothing is recorded while it isn't.
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
use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE, HWND, POINT, RECT, WAIT_OBJECT_0};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::System::Threading::{
    CreateWaitableTimerExW, GetCurrentThread, GetThreadTimes, SetThreadPriority, SetWaitableTimer, WaitForSingleObject, CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
    THREAD_PRIORITY_BELOW_NORMAL, TIMER_ALL_ACCESS,
};
use windows::Win32::UI::HiDpi::{GetDpiForWindow, SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, GetLastInputInfo, LASTINPUTINFO};
use windows::Win32::UI::WindowsAndMessaging::{GetClientRect, GetCursorPos, GetForegroundWindow, GetWindowThreadProcessId, IsWindow};

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

/// Mouse buttons from the asynchronous key state (physical buttons, like Raw Input).
struct Buttons {
    down: [bool; 5],
}

const BUTTON_VK: [(i32, u8); 5] = [(0x01, BTN_LEFT), (0x02, BTN_RIGHT), (0x04, BTN_MIDDLE), (0x05, BTN_X1), (0x06, BTN_X2)];

impl Buttons {
    fn new() -> Buttons {
        let mut b = Buttons { down: [false; 5] };
        // Current state, and clear the "pressed since" bits.
        for (i, (vk, _)) in BUTTON_VK.iter().enumerate() {
            b.down[i] = unsafe { GetAsyncKeyState(*vk) } as u16 & 0x8000 != 0;
        }
        b
    }
    /// Changes since the last call: (button, down).
    fn poll(&mut self, out: &mut Vec<(u8, bool)>) {
        for (i, (vk, b)) in BUTTON_VK.iter().enumerate() {
            let s = unsafe { GetAsyncKeyState(*vk) } as u16;
            let now = s & 0x8000 != 0;
            let pressed_since = s & 1 != 0;
            let was = self.down[i];
            if now != was {
                out.push((*b, now));
            } else if !now && pressed_since {
                // Pressed and released between two ticks.
                out.push((*b, true));
                out.push((*b, false));
            }
            self.down[i] = now;
        }
    }
}

/// Per-section time (TSC cycles) inside the loop, for the cost report.
#[derive(Default)]
struct Sections {
    buttons: u64,
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
    let (Some(fast), Some(slow)) = (Timer::new().filter(|t| t.start(period_ms)), Timer::new().filter(|t| t.start(33))) else {
        log::warn!("input capture: no timer");
        return CaptureStats::default();
    };
    if !fast.1 {
        log::warn!("input capture: high-resolution timer not available (Windows older than 10 1803); ~64 Hz");
    }
    let mut buttons = Buttons::new();
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
    let mut prev_t = w.video_us(qpc_hns());
    // Active = the cursor moved (or a button changed) in the last 250 ms: full rate. Otherwise
    // 30 Hz in the game (and only GetLastInputInfo unless something happened), 10 Hz outside it.
    let mut quiet_until = 0i64;
    let mut changes: Vec<(u8, bool)> = Vec::with_capacity(16);
    let mut last_input = 0u32;
    log::info!("input capture: {rate} Hz (cursor + buttons, no hooks, no mouse Raw Input)");
    while !stop.load(Ordering::Relaxed) {
        let now0 = qpc_hns();
        if focused && now0 < quiet_until {
            fast.wait();
            st.active_ticks += 1;
        } else if focused {
            slow.wait(); // 30 Hz
            st.idle_wakes += 1;
        } else {
            std::thread::sleep(std::time::Duration::from_millis(100));
            st.idle_wakes += 1;
        }
        st.ticks += 1;
        let now = qpc_hns();
        let t = w.video_us(now);

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
        if !focused {
            prev_t = t;
            continue;
        }
        if now >= quiet_until {
            // Resting: only look at the mouse when Windows saw any input since the last check.
            let mut li = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
            let input = unsafe { GetLastInputInfo(&mut li) }.as_bool() && li.dwTime != last_input;
            last_input = li.dwTime;
            if !input {
                prev_t = t;
                continue;
            }
        }
        changes.clear();
        buttons.poll(&mut changes);
        let c5 = tsc();
        sec.buttons += c5 - c4;
        if !changes.is_empty() {
            // The middle of the interval it happened in.
            let tb = (prev_t + t) / 2;
            for &(button, down) in &changes {
                st.buttons += down as u64;
                w.push(Record::Button { t: tb, button, down });
            }
            quiet_until = now + 2_500_000;
        }
        let mut p = POINT::default();
        if unsafe { GetCursorPos(&mut p) }.is_ok() && last_pt != Some((p.x, p.y)) {
            last_pt = Some((p.x, p.y));
            let (x, y) = to_client_units(p.x, p.y, info.unwrap().client);
            w.push(Record::Cursor { t, x, y });
            st.cursor_samples += 1;
            quiet_until = now + 2_500_000;
        }
        let c6 = tsc();
        sec.cursor += c6 - c5;
        w.flush_if_due(t);
        sec.write += tsc() - c6;
        prev_t = t;
    }
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
        ("buttons".into(), ms(sec.buttons)),
        ("foreground".into(), ms(sec.focus)),
        ("cursor".into(), ms(sec.cursor)),
        ("window".into(), ms(sec.window)),
        ("write".into(), ms(sec.write)),
    ];
    st
}
