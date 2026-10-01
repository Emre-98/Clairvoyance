//! Automated test of the input recording (replay overlay) on Windows.
//!
//!   inputtest [--dpi aware|unaware] [--rate 250] [--secs 12] [--no-video] [--out DIR]
//!
//! Opens a fake game window (with a title bar and frame, so client != window), records it with
//! the real recorder (window capture, 60 fps, cursor drawn by Windows) and the real input capture
//! thread, and drives the mouse with SendInput along a known path with known clicks, a side
//! button, the wheel, a focus loss (click on another window) and a window move. Then it checks:
//!
//! - file contents: every recorded cursor sample against the path SendInput drew (px error),
//!   click/button/wheel records and their timing, the focus gap, the window change;
//! - DPI normalization: `--dpi unaware` makes the window DPI-unaware, so on a 150 % display
//!   Windows scales it (logical 100 % coordinates vs physical pixels); `aware` = per-monitor v2;
//! - alignment with the video: the cursor Windows drew into each video frame (found in the
//!   pixels) against the recorded cursor at that frame's time, mapped the way the overlay maps it
//!   (client -> captured frame -> letterbox); also the time shift that fits best;
//! - cost: capture-thread CPU, samples, raw mouse messages, file size, writer memory.
//!
//! Writes `inputtest-report.json` and prints a summary. Exit code 1 if a check fails.

#[cfg(not(windows))]
fn main() {
    eprintln!("Windows only");
}

#[cfg(windows)]
fn main() {
    win::main();
}

#[cfg(windows)]
mod win {
    use cv_capture::win::d3d::qpc_hns;
    use cv_capture::win::input::{window_info, InputCapture};
    use cv_core::game::{FrameSource, Region};
    use cv_core::input::{self, stats::Analysis, CaptureRequest, InputWriter, Meta, Record};
    use serde_json::json;
    use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use windows::core::w;
    use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
    use windows::Win32::Graphics::Gdi::{BeginPaint, ClientToScreen, CreateSolidBrush, DeleteObject, EndPaint, FillRect, InvalidateRect, PAINTSTRUCT};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::HiDpi::{SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, DPI_AWARENESS_CONTEXT_UNAWARE};
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN,
        MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_VIRTUALDESK, MOUSEEVENTF_WHEEL, MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, MOUSEINPUT, MOUSE_EVENT_FLAGS,
    };
    use windows::Win32::UI::WindowsAndMessaging::*;

    static FLASH: AtomicBool = AtomicBool::new(false);
    static GAME: AtomicIsize = AtomicIsize::new(0);

    unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        unsafe {
            match msg {
                WM_PAINT => {
                    let mut ps = PAINTSTRUCT::default();
                    let hdc = BeginPaint(hwnd, &mut ps);
                    let mut rc = RECT::default();
                    let _ = GetClientRect(hwnd, &mut rc);
                    let game = hwnd.0 as isize == GAME.load(Ordering::SeqCst);
                    let bg = CreateSolidBrush(COLORREF(if game { 0x00808080 } else { 0x00403020 }));
                    FillRect(hdc, &rc, bg);
                    let _ = DeleteObject(bg.into());
                    if game && FLASH.load(Ordering::SeqCst) {
                        // Click indicator: green square in the top-left corner.
                        let g = CreateSolidBrush(COLORREF(0x0000ff00));
                        FillRect(hdc, &RECT { left: 10, top: 10, right: 50, bottom: 50 }, g);
                        let _ = DeleteObject(g.into());
                    }
                    let _ = EndPaint(hwnd, &ps);
                    LRESULT(0)
                }
                WM_LBUTTONDOWN | WM_RBUTTONDOWN => {
                    FLASH.store(true, Ordering::SeqCst);
                    let _ = InvalidateRect(Some(hwnd), None, false);
                    SetTimer(Some(hwnd), 1, 120, None);
                    LRESULT(0)
                }
                WM_TIMER => {
                    let _ = KillTimer(Some(hwnd), 1);
                    FLASH.store(false, Ordering::SeqCst);
                    let _ = InvalidateRect(Some(hwnd), None, false);
                    LRESULT(0)
                }
                WM_SETCURSOR => {
                    SetCursor(LoadCursorW(None, IDC_ARROW).ok());
                    LRESULT(1)
                }
                _ => DefWindowProcW(hwnd, msg, wp, lp),
            }
        }
    }

    fn arg(name: &str) -> Option<String> {
        let a: Vec<String> = std::env::args().collect();
        a.iter().position(|x| x == name).and_then(|i| a.get(i + 1)).cloned()
    }

    /// Absolute SendInput over the virtual desktop (physical pixels: this thread is PMv2).
    fn send_mouse(flags: MOUSE_EVENT_FLAGS, x: i32, y: i32, data: i32) {
        unsafe {
            let (vx, vy) = (GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN));
            let (vw, vh) = (GetSystemMetrics(SM_CXVIRTUALSCREEN), GetSystemMetrics(SM_CYVIRTUALSCREEN));
            let nx = (((x - vx) as f64 + 0.5) * 65536.0 / vw as f64) as i32;
            let ny = (((y - vy) as f64 + 0.5) * 65536.0 / vh as f64) as i32;
            let i = INPUT {
                r#type: INPUT_MOUSE,
                Anonymous: INPUT_0 { mi: MOUSEINPUT { dx: nx, dy: ny, mouseData: data as u32, dwFlags: flags | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK | MOUSEEVENTF_MOVE, time: 0, dwExtraInfo: 0 } },
            };
            SendInput(&[i], std::mem::size_of::<INPUT>() as i32);
        }
    }

    fn client_screen(h: HWND) -> RECT {
        unsafe {
            let mut rc = RECT::default();
            let _ = GetClientRect(h, &mut rc);
            let mut a = POINT { x: rc.left, y: rc.top };
            let mut b = POINT { x: rc.right, y: rc.bottom };
            let _ = ClientToScreen(h, &mut a);
            let _ = ClientToScreen(h, &mut b);
            RECT { left: a.x, top: a.y, right: b.x, bottom: b.y }
        }
    }

    fn wait_until(t0: Instant, secs: f64) {
        let target = t0 + Duration::from_secs_f64(secs);
        while Instant::now() < target {
            let left = target - Instant::now();
            if left > Duration::from_millis(3) {
                std::thread::sleep(Duration::from_millis(1));
            } else {
                std::hint::spin_loop();
            }
        }
    }

    struct Truth {
        /// (qpc hns, screen x, y)
        moves: Vec<(i64, i32, i32)>,
        /// (qpc hns, button, down)
        buttons: Vec<(i64, u8, bool)>,
        wheel: Vec<i64>,
        focus_lost: (i64, i64),
        moved_at: i64,
    }

    /// `--other x y w h secs`: a topmost window in a separate process (to take the focus away
    /// from the fake game the way another program would).
    fn other_window_process(a: &[i32]) {
        unsafe {
            let _ = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let hinst = GetModuleHandleW(None).unwrap();
            let class = w!("CvInputTestOther");
            let wc = WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: hinst.into(), lpszClassName: class, hCursor: LoadCursorW(None, IDC_ARROW).unwrap(), ..Default::default() };
            RegisterClassW(&wc);
            let h = CreateWindowExW(WS_EX_TOPMOST, class, w!("Other window"), WS_OVERLAPPEDWINDOW | WS_VISIBLE, a[0], a[1], a[2], a[3], None, None, Some(hinst.into()), None).unwrap();
            let _ = SetWindowPos(h, Some(HWND_TOPMOST), a[0], a[1], a[2], a[3], SWP_SHOWWINDOW);
            let end = Instant::now() + Duration::from_secs(a[4] as u64);
            let mut msg = MSG::default();
            while Instant::now() < end {
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }

    /// `--micro`: cost of each candidate Windows call while the mouse moves (SendInput at
    /// 500 Hz from another thread), called 250 times a second from a per-monitor-aware thread.
    fn micro() {
        use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
        use windows::Win32::UI::Input::{GetRawInputBuffer, GetRawInputData, RegisterRawInputDevices, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER, RIDEV_INPUTSINK, RID_INPUT};
        unsafe {
            let _ = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        }
        let tsc = || unsafe { core::arch::x86_64::_rdtsc() };
        let (t0, q0) = (tsc(), qpc_hns());
        std::thread::sleep(Duration::from_millis(200));
        let hz = (tsc() - t0) as f64 / ((qpc_hns() - q0) as f64 / 1e7);
        let us = |c: u64| c as f64 / hz * 1e6;
        // The mouse driver: circles in the middle of the screen at 500 Hz.
        let stop = Arc::new(AtomicBool::new(false));
        let st = stop.clone();
        let drv = std::thread::spawn(move || {
            unsafe {
                let _ = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            }
            let (cx, cy) = unsafe { (GetSystemMetrics(SM_CXSCREEN) / 2, GetSystemMetrics(SM_CYSCREEN) / 2) };
            let t0 = Instant::now();
            let mut k = 0u64;
            while !st.load(Ordering::Relaxed) {
                let t = t0.elapsed().as_secs_f64();
                send_mouse(MOUSE_EVENT_FLAGS(0), cx + (300.0 * t.cos()) as i32, cy + (200.0 * t.sin()) as i32, 0);
                k += 1;
                wait_until(t0, k as f64 / 500.0);
            }
        });
        // Raw Input on a message-only window of this thread.
        let hwnd = unsafe {
            let hinst = GetModuleHandleW(None).unwrap();
            let class = w!("CvMicroRaw");
            let wc = WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: hinst.into(), lpszClassName: class, ..Default::default() };
            RegisterClassW(&wc);
            CreateWindowExW(WINDOW_EX_STYLE(0), class, w!(""), WINDOW_STYLE(0), 0, 0, 0, 0, Some(HWND_MESSAGE), None, Some(hinst.into()), None).unwrap()
        };
        let reg = |on: bool| unsafe {
            let dev = RAWINPUTDEVICE { usUsagePage: 1, usUsage: 2, dwFlags: if on { RIDEV_INPUTSINK } else { windows::Win32::UI::Input::RIDEV_REMOVE }, hwndTarget: if on { hwnd } else { HWND::default() } };
            let _ = RegisterRawInputDevices(&[dev], std::mem::size_of::<RAWINPUTDEVICE>() as u32);
        };
        let hdr = std::mem::size_of::<RAWINPUTHEADER>() as u32;
        let mut buf = vec![0u64; 2048];
        let mut report = Vec::new();
        let mut run = |name: &str, f: &mut dyn FnMut() -> u64| {
            let t0 = Instant::now();
            let (mut cyc, mut n, mut msgs) = (0u64, 0u64, 0u64);
            while t0.elapsed() < Duration::from_secs(4) {
                std::thread::sleep(Duration::from_millis(4));
                let a = tsc();
                msgs += f();
                cyc += tsc() - a;
                n += 1;
            }
            let line = format!("{name}: {:.2} us per call ({n} calls{})", us(cyc) / n as f64, if msgs > 0 { format!(", {msgs} messages, {:.2} us per message", us(cyc) / msgs as f64) } else { String::new() });
            println!("MICRO {line}");
            report.push(line);
        };
        let mut p = POINT::default();
        run("GetCursorPos", &mut || unsafe { let _ = GetCursorPos(&mut p); 0 });
        run("GetPhysicalCursorPos", &mut || unsafe { let _ = GetPhysicalCursorPos(&mut p); 0 });
        run("GetForegroundWindow", &mut || unsafe { std::hint::black_box(GetForegroundWindow()); 0 });
        run("GetAsyncKeyState x5 (L R M X1 X2)", &mut || unsafe {
            for vk in [1, 2, 4, 5, 6] {
                std::hint::black_box(GetAsyncKeyState(vk));
            }
            0
        });
        reg(true);
        std::thread::sleep(Duration::from_millis(100));
        run("GetRawInputBuffer (mouse registered)", &mut || unsafe {
            let mut total = 0u64;
            loop {
                let mut cb = (buf.len() * 8) as u32;
                let n = GetRawInputBuffer(Some(buf.as_mut_ptr() as *mut RAWINPUT), &mut cb, hdr);
                if n == 0 || n == u32::MAX {
                    break;
                }
                total += n as u64;
            }
            total
        });
        run("PeekMessage + GetRawInputData (mouse registered)", &mut || unsafe {
            let mut msg = MSG::default();
            let mut total = 0;
            while PeekMessageW(&mut msg, Some(hwnd), WM_INPUT, WM_INPUT, PM_REMOVE).as_bool() {
                let mut ri = RAWINPUT::default();
                let mut size = std::mem::size_of::<RAWINPUT>() as u32;
                GetRawInputData(HRAWINPUT(msg.lParam.0 as *mut _), RID_INPUT, Some(&mut ri as *mut _ as *mut _), &mut size, hdr);
                total += 1;
            }
            total
        });
        run("GetCursorPos (mouse registered)", &mut || unsafe { let _ = GetCursorPos(&mut p); 0 });
        reg(false);
        stop.store(true, Ordering::Relaxed);
        let _ = drv.join();
    }

    pub fn main() {
        let argv: Vec<String> = std::env::args().collect();
        if let Some(i) = argv.iter().position(|a| a == "--other") {
            let v: Vec<i32> = argv[i + 1..].iter().take(5).filter_map(|x| x.parse().ok()).collect();
            if v.len() == 5 {
                other_window_process(&v);
            }
            return;
        }
        if argv.iter().any(|a| a == "--micro") {
            micro();
            return;
        }
        let idle = argv.iter().any(|a| a == "--idle");
        // --load: keep half the cores busy (like a game), so the CPU runs at game clocks; on an
        // idle desktop the cores clock down and every call looks slower than it is in game.
        let load_stop = Arc::new(AtomicBool::new(false));
        if argv.iter().any(|a| a == "--load") {
            let n = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4) / 2;
            for _ in 0..n {
                let st = load_stop.clone();
                std::thread::spawn(move || {
                    let mut x = 1u64;
                    while !st.load(Ordering::Relaxed) {
                        for _ in 0..10_000 {
                            x = x.wrapping_mul(6364136223846793005).wrapping_add(1);
                        }
                        std::hint::black_box(x);
                    }
                });
            }
            println!("load: {n} busy threads");
        }
        let dpi = arg("--dpi").unwrap_or_else(|| "aware".into());
        let rate: u32 = arg("--rate").and_then(|r| r.parse().ok()).unwrap_or(250);
        let secs: f64 = arg("--secs").and_then(|r| r.parse().ok()).unwrap_or(12.0);
        let with_video = !std::env::args().any(|a| a == "--no-video");
        let out = std::path::PathBuf::from(arg("--out").unwrap_or_else(|| "inputtest-output".into())).join(&dpi);
        let _ = std::fs::remove_dir_all(&out);
        std::fs::create_dir_all(&out).unwrap();
        if let Ok(f) = std::fs::File::create(out.join("log.txt")) {
            struct L(std::sync::Mutex<std::fs::File>);
            impl log::Log for L {
                fn enabled(&self, _: &log::Metadata) -> bool {
                    true
                }
                fn log(&self, r: &log::Record) {
                    use std::io::Write;
                    let _ = writeln!(self.0.lock().unwrap(), "{} {}", r.level(), r.args());
                }
                fn flush(&self) {}
            }
            let _ = log::set_boxed_logger(Box::new(L(std::sync::Mutex::new(f))));
            log::set_max_level(log::LevelFilter::Info);
        }
        unsafe {
            let _ = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        }
        println!("inputtest: window {dpi}, cursor {rate} Hz, {secs} s, video {with_video}");

        // Window thread: the fake game + another window (to take the focus away).
        let (tx, rx) = std::sync::mpsc::channel::<(isize, isize)>();
        let unaware = dpi == "unaware";
        std::thread::spawn(move || unsafe {
            let _ = SetThreadDpiAwarenessContext(if unaware { DPI_AWARENESS_CONTEXT_UNAWARE } else { DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2 });
            let hinst = GetModuleHandleW(None).unwrap();
            let class = w!("CvInputTestGame");
            let wc = WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: hinst.into(), lpszClassName: class, hCursor: LoadCursorW(None, IDC_ARROW).unwrap(), ..Default::default() };
            RegisterClassW(&wc);
            let game = CreateWindowExW(WS_EX_TOPMOST, class, w!("Input test game"), WS_OVERLAPPEDWINDOW | WS_VISIBLE, 100, 100, 1000, 600, None, None, Some(hinst.into()), None).unwrap();
            GAME.store(game.0 as isize, Ordering::SeqCst);
            tx.send((game.0 as isize, 0)).unwrap();
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        });
        let (g, _) = rx.recv().unwrap();
        let game = HWND(g as *mut _);
        // Place them in physical pixels: the game at (200, 150), client ~ 1280 x 720 physical.
        unsafe {
            let _ = SetWindowPos(game, None, 200, 150, 1300, 760, SWP_NOZORDER);
            let gr = client_screen(game);
            // The other window: another process, to the right of the game.
            let _ = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--other", &(gr.right + 80).to_string(), &gr.top.to_string(), "500", "350", &((secs as i32) + 10).to_string()])
                .spawn();
        }
        std::thread::sleep(Duration::from_millis(600));
        let info0 = window_info(game).unwrap();
        println!("game window: client {:?}, frame {:?}, dpi {}", info0.client, info0.frame, info0.dpi);
        let exe = std::env::current_exe().unwrap().file_name().unwrap().to_string_lossy().to_lowercase();

        // Activate the game with a click (focus by input, like a player would).
        let c = client_screen(game);
        let (cx, cy) = ((c.left + c.right) / 2, (c.top + c.bottom) / 2);
        send_mouse(MOUSE_EVENT_FLAGS(0), cx, cy, 0);
        std::thread::sleep(Duration::from_millis(50));
        send_mouse(MOUSEEVENTF_LEFTDOWN, cx, cy, 0);
        send_mouse(MOUSEEVENTF_LEFTUP, cx, cy, 0);
        std::thread::sleep(Duration::from_millis(400));

        let rec = if with_video {
            match cv_capture::win::start_test_recording(&out, &exe) {
                Ok(r) => Some(r),
                Err(e) => {
                    println!("recording failed: {e:#}");
                    None
                }
            }
        } else {
            None
        };
        let base = rec.as_ref().and_then(|r| r.test_clock_base()).unwrap_or_else(qpc_hns);
        let path = out.join("test.input");
        let writer = Arc::new(InputWriter::create(&path, base, rate, &Meta { game_id: "test".into(), rate, app_version: "test".into() }).unwrap());
        let cap = InputCapture::start(CaptureRequest { writer: writer.clone(), process_names: vec![exe.clone()] });
        std::thread::sleep(Duration::from_millis(300));

        // The scripted "game".
        let mut truth = Truth { moves: Vec::new(), buttons: Vec::new(), wheel: Vec::new(), focus_lost: (0, 0), moved_at: 0 };
        let t0 = Instant::now();
        let step = 1.0 / 500.0;
        let mut t = 0.0;
        let mut max_buf = 0usize;
        let mut events: Vec<(f64, &str)> = vec![];
        for k in 1..((secs - 1.0) as i32) {
            events.push((k as f64, if k % 2 == 1 { "left" } else { "right" }));
        }
        events.push((6.5, "x1"));
        events.push((7.5, "wheel"));
        events.push((8.0, "away"));
        events.push((9.0, "back"));
        events.push((10.0, "move"));
        events.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        let mut ev_i = 0;
        let mut away = false;
        if idle {
            // Mouse untouched: only the capture thread runs.
            std::thread::sleep(Duration::from_secs_f64(secs));
        }
        while !idle && t < secs {
            wait_until(t0, t);
            let c = client_screen(game);
            let (w, h) = ((c.right - c.left) as f64, (c.bottom - c.top) as f64);
            let x = c.left + (w * (0.5 + 0.38 * (std::f64::consts::PI * 0.5 * t).sin())) as i32;
            let y = c.top + (h * (0.5 + 0.36 * (std::f64::consts::PI * 0.35 * t + 0.7).sin())) as i32;
            if !away && !idle {
                send_mouse(MOUSE_EVENT_FLAGS(0), x, y, 0);
                truth.moves.push((qpc_hns(), x, y));
            }
            while !idle && ev_i < events.len() && events[ev_i].0 <= t {
                let (bx, by) = if away { (0, 0) } else { (x, y) };
                let now = qpc_hns();
                match events[ev_i].1 {
                    "left" => {
                        send_mouse(MOUSEEVENTF_LEFTDOWN, bx, by, 0);
                        truth.buttons.push((now, 1, true));
                        std::thread::sleep(Duration::from_millis(40));
                        send_mouse(MOUSEEVENTF_LEFTUP, bx, by, 0);
                        truth.buttons.push((qpc_hns(), 1, false));
                    }
                    "right" => {
                        send_mouse(MOUSEEVENTF_RIGHTDOWN, bx, by, 0);
                        truth.buttons.push((now, 2, true));
                        std::thread::sleep(Duration::from_millis(40));
                        send_mouse(MOUSEEVENTF_RIGHTUP, bx, by, 0);
                        truth.buttons.push((qpc_hns(), 2, false));
                    }
                    "x1" => {
                        send_mouse(MOUSEEVENTF_XDOWN, bx, by, 1);
                        truth.buttons.push((now, 4, true));
                        std::thread::sleep(Duration::from_millis(30));
                        send_mouse(MOUSEEVENTF_XUP, bx, by, 1);
                        truth.buttons.push((qpc_hns(), 4, false));
                    }
                    "wheel" => {
                        send_mouse(MOUSEEVENTF_WHEEL, x, y, -120);
                        truth.wheel.push(now);
                    }
                    "away" => {
                        let r = client_screen(game);
                        // Middle of the other window (to the right of the game, 500 x 350).
                        let (ox, oy) = (r.right + 80 + 250, r.top + 175);
                        send_mouse(MOUSE_EVENT_FLAGS(0), ox, oy, 0);
                        std::thread::sleep(Duration::from_millis(20));
                        send_mouse(MOUSEEVENTF_LEFTDOWN, ox, oy, 0);
                        send_mouse(MOUSEEVENTF_LEFTUP, ox, oy, 0);
                        truth.focus_lost.0 = qpc_hns();
                        away = true;
                    }
                    "back" => {
                        send_mouse(MOUSE_EVENT_FLAGS(0), x, y, 0);
                        std::thread::sleep(Duration::from_millis(20));
                        send_mouse(MOUSEEVENTF_LEFTDOWN, x, y, 0);
                        send_mouse(MOUSEEVENTF_LEFTUP, x, y, 0);
                        truth.focus_lost.1 = qpc_hns();
                        away = false;
                    }
                    "move" => unsafe {
                        let mut wr = RECT::default();
                        let _ = GetWindowRect(game, &mut wr);
                        let _ = SetWindowPos(game, None, wr.left + 60, wr.top + 40, 0, 0, SWP_NOSIZE | SWP_NOZORDER);
                        truth.moved_at = qpc_hns();
                    },
                    _ => {}
                }
                ev_i += 1;
            }
            max_buf = max_buf.max(writer.buffered());
            t += step;
        }
        std::thread::sleep(Duration::from_millis(300));
        let cost = cap.stop();
        load_stop.store(true, Ordering::Relaxed);
        let end_us = writer.video_us(qpc_hns());
        let (bytes, nrec) = writer.finish(end_us);
        let video = rec.as_ref().and_then(|r| r.stop_test_recording().map_err(|e| println!("stop recording: {e:#}")).ok());
        unsafe {
            let _ = PostMessageW(Some(game), WM_CLOSE, WPARAM(0), LPARAM(0));
            if let Ok(o) = FindWindowW(w!("CvInputTestOther"), w!("Other window")) {
                let _ = PostMessageW(Some(o), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }

        if idle {
            println!(
                "IDLE COST ({rate} Hz, mouse untouched, game focused): capture thread {:.4}% of one core ({:.2} ms CPU from cycles, {:.1} ms tick-sampled, in {:.1} s), {} ticks ({} active, {} idle wakes), {} raw mouse messages, sections {:?}",
                cost.core_percent(), cost.cpu_ms, cost.cpu_ms_sampled, cost.wall_secs, cost.ticks, cost.active_ticks, cost.idle_wakes, cost.raw_mouse_msgs, cost.sections_ms
            );
            std::fs::write(out.join("idle-cost.json"), serde_json::to_vec_pretty(&cost).unwrap()).unwrap();
            return;
        }
        // ---- checks ----
        let mut checks: Vec<(String, bool, String)> = Vec::new();
        let mut check = |name: &str, ok: bool, detail: String| {
            println!("{} {name}: {detail}", if ok { "PASS" } else { "FAIL" });
            checks.push((name.into(), ok, detail));
        };
        let f = input::read(&path).unwrap();
        let an = Analysis::from_file(&f);
        let us = |q: i64| (q - base) / 10;
        let windows: Vec<(i64, input::WindowInfo)> = f.records.iter().filter_map(|r| if let Record::Window { t, info } = r { Some((*t, *info)) } else { None }).collect();
        let client_at = |t: i64| windows.iter().rev().find(|w| w.0 <= t).or(windows.first()).map(|w| w.1).unwrap();
        // Cursor samples vs the path (truth position at the sample's time).
        let mut errs = Vec::new();
        let mut lag_ms = Vec::new();
        for r in &f.records {
            let Record::Cursor { t, x, y } = *r else { continue };
            let i = truth.moves.partition_point(|m| us(m.0) <= t);
            if i == 0 {
                continue;
            }
            let (_, tx, ty) = truth.moves[i - 1];
            let c = client_at(t).client;
            let px = c.x as f64 + x as f64 / input::UNIT * c.w as f64;
            let py = c.y as f64 + y as f64 / input::UNIT * c.h as f64;
            let e = ((px - tx as f64).powi(2) + (py - ty as f64).powi(2)).sqrt();
            errs.push(e);
            // Which truth sample matches best (time lag of the sample).
            let mut best = (f64::MAX, 0i64);
            for j in i.saturating_sub(10)..(i + 3).min(truth.moves.len()) {
                let (q, ax, ay) = truth.moves[j];
                let d = ((px - ax as f64).powi(2) + (py - ay as f64).powi(2)).sqrt();
                if d < best.0 {
                    best = (d, t - us(q));
                }
            }
            lag_ms.push(best.1 as f64 / 1000.0);
        }
        errs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        lag_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let pct = |v: &Vec<f64>, p: f64| v.get(((v.len() as f64 - 1.0) * p) as usize).copied().unwrap_or(f64::NAN);
        check(
            "cursor samples on the SendInput path",
            !errs.is_empty() && pct(&errs, 0.5) <= 1.5 && pct(&errs, 0.95) <= 3.0,
            format!("{} samples, error median {:.2} px, p95 {:.2} px, max {:.2} px; sample time - input time median {:.2} ms, p95 {:.2} ms", errs.len(), pct(&errs, 0.5), pct(&errs, 0.95), pct(&errs, 1.0), pct(&lag_ms, 0.5), pct(&lag_ms, 0.95)),
        );
        // Samples only when the cursor moved: SendInput moves it 500 times a second; 1 s of the
        // test is away from the game.
        let expect_samples = if idle { 0.0 } else { (secs - 2.0) * (rate.min(500)) as f64 * 0.8 };
        check("sample rate", errs.len() as f64 >= expect_samples, format!("{} samples in {secs} s at {rate} Hz", errs.len()));
        // Buttons.
        let rec_btn: Vec<(i64, u8, bool)> = f.records.iter().filter_map(|r| if let Record::Button { t, button, down } = r { Some((*t, *button, *down)) } else { None }).collect();
        let mut btn_err = Vec::new();
        let mut matched = 0;
        let in_focus = |q: i64| !(q > truth.focus_lost.0 && q < truth.focus_lost.1);
        let expected: Vec<&(i64, u8, bool)> = truth.buttons.iter().filter(|b| in_focus(b.0)).collect();
        for b in &expected {
            if let Some(r) = rec_btn.iter().filter(|r| r.1 == b.1 && r.2 == b.2).min_by_key(|r| (r.0 - us(b.0)).abs()) {
                let e = (r.0 - us(b.0)) as f64 / 1000.0;
                if e.abs() < 30.0 {
                    matched += 1;
                    btn_err.push(e.abs());
                }
            }
        }
        btn_err.sort_by(|a, b| a.partial_cmp(b).unwrap());
        check(
            "clicks (left, right, side button) recorded with their time",
            matched == expected.len() && pct(&btn_err, 1.0) <= 1000.0 / 60.0,
            format!("{matched}/{} matched, time error median {:.2} ms, max {:.2} ms (tick {:.1} ms; target 1 frame = 16.7 ms)", expected.len(), pct(&btn_err, 0.5), pct(&btn_err, 1.0), 1000.0 / rate as f64),
        );
        let wheel = f.records.iter().filter(|r| matches!(r, Record::Wheel { delta: -120, .. })).count();
        check("wheel not recorded (no mouse Raw Input, by design)", wheel == 0, format!("{wheel} wheel record(s)"));
        // Focus gap.
        let gap = an.focus.windows(2).map(|w| (w[0].1, w[1].0)).find(|g| g.1 - g.0 > 0.3);
        let (fl0, fl1) = (us(truth.focus_lost.0) as f64 / 1e6, us(truth.focus_lost.1) as f64 / 1e6);
        check(
            "focus loss is a gap (nothing recorded meanwhile)",
            gap.is_some_and(|g| (g.0 - fl0).abs() < 0.1 && (g.1 - fl1).abs() < 0.1) && !an.moves.iter().any(|m| m.t > fl0 + 0.05 && m.t < fl1 - 0.05),
            format!("gap {:?}, clicked away at {fl0:.2}-{fl1:.2} s", gap),
        );
        let moved = windows.iter().any(|w| (w.0 - us(truth.moved_at)).abs() < 600_000 && w.0 > 0); // checked every 500 ms
        check("window move logged", moved && windows.len() >= 2, format!("{} window records", windows.len()));
        let core = cost.core_percent();
        check(
            "capture cost",
            true,
            format!(
                "capture thread {:.4}% of one core ({:.2} ms CPU from cycles; {:.1} ms tick-sampled; in {:.1} s), {} ticks ({:.1} us per tick), {} leftover WM_INPUT, {} raw mouse messages, {} active ticks, {} idle wakes, sections {:?}, writer buffer max {:.1} KB, file {:.1} KB, {} records",
                core,
                cost.cpu_ms,
                cost.cpu_ms_sampled,
                cost.wall_secs,
                cost.ticks,
                cost.cpu_ms * 1000.0 / cost.ticks.max(1) as f64,
                cost.leftover_msgs,
                cost.raw_mouse_msgs,
                cost.active_ticks,
                cost.idle_wakes,
                cost.sections_ms,
                max_buf as f64 / 1024.0,
                bytes as f64 / 1024.0,
                nrec
            ),
        );
        // DPI: the client size in physical pixels vs what the window thinks (unaware = scaled).
        check(
            "DPI normalization",
            true,
            format!("monitor DPI {} ({}%), window {}, client {}x{} physical; positions above are checked in physical pixels", info0.dpi, info0.dpi * 100 / 96, dpi, info0.client.w, info0.client.h),
        );

        // ---- the video ----
        let mut video_report = json!(null);
        if let Some(v) = video {
            let _ = cv_capture::remux::finalize_in_place(&v, &|| false);
            match cv_capture::win::frames::VideoFrames::open(&v) {
                Ok(mut fr) => {
                    let (vw, vh) = fr.size();
                    let mut pts: Vec<(f64, f64, f64)> = Vec::new(); // (t, x, y) cursor hotspot in video px
                    let mut flashes: Vec<f64> = Vec::new();
                    let dur = fr.duration();
                    let mut prev_flash = false;
                    let _ = fr.frames(0.5, dur - 0.1, Region { x: 0, y: 0, w: vw, h: vh }, &mut |t, img| {
                        // The cursor: pixels far from the gray background (arrow outline/fill),
                        // ignoring the flash square and the window frame.
                        let mut minx = u32::MAX;
                        let mut miny = u32::MAX;
                        let mut n = 0;
                        let wi = client_at((t * 1e6) as i64);
                        let (fx, fy) = (wi.client.x - wi.frame.x, wi.client.y - wi.frame.y);
                        let sx = vw as f64 / wi.frame.w as f64;
                        let x0 = ((fx as f64 + 70.0) * sx) as u32;
                        let y0 = ((fy as f64 + 2.0) * sx) as u32;
                        let x1 = (((fx + wi.client.w) as f64 - 2.0) * sx).min(vw as f64 - 1.0) as u32;
                        let y1 = (((fy + wi.client.h) as f64 - 2.0) * sx).min(vh as f64 - 1.0) as u32;
                        for y in y0..y1 {
                            for x in x0..x1 {
                                let p = img.px(x, y);
                                let d = (p[0] as i32 - 128).abs() + (p[1] as i32 - 128).abs() + (p[2] as i32 - 128).abs();
                                if d > 150 {
                                    n += 1;
                                    if y < miny || (y == miny && x < minx) {
                                        miny = y;
                                        minx = x;
                                    }
                                }
                            }
                        }
                        if n > 20 && n < 3000 {
                            pts.push((t, minx as f64, miny as f64));
                        }
                        // The click flash (green square at client 10..50).
                        let sq = img.px(((fx as f64 + 30.0) * sx) as u32, ((fy as f64 + 30.0) * sx) as u32);
                        let on = sq[1] > 200 && sq[0] < 80;
                        if on && !prev_flash {
                            flashes.push(t);
                        }
                        prev_flash = on;
                        true
                    });
                    // Recorded cursor at time t (seconds), mapped into the video like the overlay does.
                    let cursor_at = |t: f64| -> Option<(f64, f64)> {
                        let i = an.moves.partition_point(|m| m.t <= t);
                        if i == 0 {
                            return None;
                        }
                        let m = an.moves[i - 1];
                        let wi = client_at((t * 1e6) as i64);
                        let s = (vw as f64 / wi.frame.w as f64).min(vh as f64 / wi.frame.h as f64);
                        let lx = (vw as f64 - wi.frame.w as f64 * s) / 2.0;
                        let ly = (vh as f64 - wi.frame.h as f64 * s) / 2.0;
                        let cx = (wi.client.x - wi.frame.x) as f64 + m.x as f64 * wi.client.w as f64;
                        let cy = (wi.client.y - wi.frame.y) as f64 + m.y as f64 * wi.client.h as f64;
                        Some((lx + cx * s, ly + cy * s))
                    };
                    let err_at = |shift: f64| -> Vec<f64> {
                        let mut e: Vec<f64> = pts
                            .iter()
                            .filter(|p| !(p.0 > fl0 - 0.1 && p.0 < fl1 + 0.3))
                            .filter_map(|&(t, x, y)| cursor_at(t + shift).map(|(cx, cy)| ((cx - x).powi(2) + (cy - y).powi(2)).sqrt()))
                            .collect();
                        e.sort_by(|a, b| a.partial_cmp(b).unwrap());
                        e
                    };
                    let e0 = err_at(0.0);
                    let mut best = (f64::MAX, 0.0);
                    let mut s = -0.060;
                    while s <= 0.060 {
                        let e = err_at(s);
                        let m = pct(&e, 0.5);
                        if m < best.0 {
                            best = (m, s);
                        }
                        s += 0.001;
                    }
                    let px_per_ms = {
                        // Cursor speed in video px per ms (median), to express pixel errors as time.
                        let mut sp: Vec<f64> = pts.windows(2).map(|w| ((w[1].1 - w[0].1).powi(2) + (w[1].2 - w[0].2).powi(2)).sqrt() / ((w[1].0 - w[0].0) * 1000.0)).filter(|v| v.is_finite()).collect();
                        sp.sort_by(|a, b| a.partial_cmp(b).unwrap());
                        pct(&sp, 0.5)
                    };
                    let clicks_rec: Vec<f64> = rec_btn.iter().filter(|b| b.2 && b.1 <= 2).map(|b| b.0 as f64 / 1e6).collect();
                    let mut flash_lag: Vec<f64> = clicks_rec
                        .iter()
                        .filter_map(|&c| flashes.iter().find(|&&f| f >= c - 0.05 && f < c + 0.2).map(|f| (f - c) * 1000.0))
                        .collect();
                    flash_lag.sort_by(|a, b| a.partial_cmp(b).unwrap());
                    let frame_ms = 1000.0 / 60.0;
                    check(
                        "video alignment: recorded cursor vs the cursor in the frames",
                        pts.len() > 100 && best.1.abs() * 1000.0 <= frame_ms,
                        format!(
                            "{} frames {}x{}; error at 0 ms shift: median {:.1} px, p95 {:.1} px (cursor moves {:.2} px/ms, so {:.1} ms); best fit shift {:+.0} ms (median {:.1} px)",
                            pts.len(),
                            vw,
                            vh,
                            pct(&e0, 0.5),
                            pct(&e0, 0.95),
                            px_per_ms,
                            pct(&e0, 0.5) / px_per_ms.max(1e-6),
                            best.1 * 1000.0,
                            best.0
                        ),
                    );
                    check(
                        "video alignment: clicks vs the click flash drawn by the window",
                        !flash_lag.is_empty(),
                        format!("{} flashes; flash appears {:.0} ms (median), {:.0}..{:.0} ms after the recorded click (includes the window's own paint + DWM, ~1-2 frames)", flash_lag.len(), pct(&flash_lag, 0.5), pct(&flash_lag, 0.0), pct(&flash_lag, 1.0)),
                    );
                    video_report = json!({ "frames": pts.len(), "size": [vw, vh], "error_px_at_0": [pct(&e0, 0.5), pct(&e0, 0.95)], "best_shift_ms": best.1 * 1000.0, "best_px": best.0, "px_per_ms": px_per_ms, "flash_lag_ms": flash_lag });
                }
                Err(e) => check("video", false, format!("can't open the recording: {e:#}")),
            }
        }

        let ok = checks.iter().all(|c| c.1);
        let report = json!({
            "dpi": dpi, "rate": rate, "secs": secs, "monitor_dpi": info0.dpi, "client": [info0.client.w, info0.client.h], "frame": [info0.frame.w, info0.frame.h],
            "checks": checks.iter().map(|c| json!({"name": c.0, "ok": c.1, "detail": c.2})).collect::<Vec<_>>(),
            "cost": cost, "file_bytes": bytes, "records": nrec, "writer_buffer_max": max_buf, "video": video_report,
        });
        std::fs::write(out.join("inputtest-report.json"), serde_json::to_vec_pretty(&report).unwrap()).unwrap();
        println!("{}", if ok { "ALL PASSED" } else { "SOME CHECKS FAILED" });
        std::process::exit(if ok { 0 } else { 1 });
    }
}
