//! Windows services for the engine: process list, focused window, TTS, perf stats,
//! GPU vendor. On other OSes (dev builds only) these are harmless stubs.

use cv_core::engine::Platform;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub struct WinPlatform {
    tts: Mutex<Option<Sender<(String, u8)>>>,
    perf: Mutex<Option<(Instant, u64)>>,
    input: crate::input::InputControl,
    /// Game simulator: this process name counts as running while the flag is set.
    fake_process: Mutex<Option<(String, Arc<AtomicBool>)>>,
    /// Cursor / mouse recording for the replay overlay (during a game).
    #[cfg(windows)]
    capture: Mutex<Option<cv_capture::win::input::InputCapture>>,
}

impl WinPlatform {
    pub fn new(input: crate::input::InputControl) -> Self {
        Self {
            tts: Mutex::new(None),
            perf: Mutex::new(None),
            input,
            fake_process: Mutex::new(None),
            #[cfg(windows)]
            capture: Mutex::new(None),
        }
    }

    pub fn set_fake_process(&self, fake: Option<(String, Arc<AtomicBool>)>) {
        *self.fake_process.lock().unwrap() = fake;
    }

    fn fake_running(&self) -> Option<String> {
        self.fake_process.lock().unwrap().as_ref().filter(|(_, f)| f.load(Ordering::SeqCst)).map(|(n, _)| n.clone())
    }

    fn tts_sender(&self) -> Sender<(String, u8)> {
        let mut g = self.tts.lock().unwrap();
        if let Some(s) = g.as_ref() {
            return s.clone();
        }
        let (tx, rx) = channel::<(String, u8)>();
        std::thread::Builder::new().name("tts".into()).spawn(move || tts_thread(rx)).ok();
        *g = Some(tx.clone());
        tx
    }
}

impl Platform for WinPlatform {
    fn running_processes(&self) -> Vec<String> {
        let mut v = process_names();
        if let Some(name) = self.fake_running() {
            v.push(name);
        }
        v
    }

    fn foreground_process(&self) -> Option<String> {
        if let Some(name) = self.fake_running() {
            return Some(name);
        }
        foreground_exe()
    }

    fn speak(&self, text: &str, volume: u8) {
        let _ = self.tts_sender().send((text.to_string(), volume));
    }

    fn perf_sample(&self) -> Option<(f64, f64)> {
        let (cpu_100ns, ram) = own_usage()?;
        let now = Instant::now();
        let mut g = self.perf.lock().unwrap();
        let cpu = match *g {
            Some((t, prev)) => {
                let wall = now.duration_since(t).as_secs_f64();
                let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1) as f64;
                if wall > 0.0 {
                    ((cpu_100ns.saturating_sub(prev)) as f64 / 1e7) / wall / cores * 100.0
                } else {
                    0.0
                }
            }
            None => 0.0,
        };
        *g = Some((now, cpu_100ns));
        Some((cpu, ram as f64 / (1024.0 * 1024.0)))
    }

    fn set_input_enabled(&self, enabled: bool) {
        self.input.set_enabled(enabled);
    }

    fn start_input_capture(&self, req: cv_core::input::CaptureRequest) {
        #[cfg(windows)]
        {
            let mut g = self.capture.lock().unwrap();
            if let Some(old) = g.take() {
                old.stop();
            }
            *g = Some(cv_capture::win::input::InputCapture::start(req));
        }
        #[cfg(not(windows))]
        drop(req);
    }

    fn stop_input_capture(&self) -> Option<cv_core::input::CaptureStats> {
        #[cfg(windows)]
        {
            let c = self.capture.lock().unwrap().take();
            c.map(|c| c.stop())
        }
        #[cfg(not(windows))]
        None
    }
}

#[cfg(windows)]
mod win {
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE};
    use windows::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS};
    use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows::Win32::System::Threading::{
        GetCurrentProcess, GetProcessTimes, OpenProcess, QueryFullProcessImageNameW, SetPriorityClass, BELOW_NORMAL_PRIORITY_CLASS, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

    fn wide_to_string(w: &[u16]) -> String {
        let end = w.iter().position(|c| *c == 0).unwrap_or(w.len());
        String::from_utf16_lossy(&w[..end])
    }

    pub fn process_names() -> Vec<String> {
        let mut out = Vec::with_capacity(256);
        unsafe {
            let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return out };
            let mut e = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
            if Process32FirstW(snap, &mut e).is_ok() {
                loop {
                    out.push(wide_to_string(&e.szExeFile).to_lowercase());
                    if Process32NextW(snap, &mut e).is_err() {
                        break;
                    }
                }
            }
            let _ = CloseHandle(snap);
        }
        out
    }

    pub fn foreground_exe() -> Option<String> {
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.0.is_null() {
                return None;
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid == 0 {
                return None;
            }
            let h: HANDLE = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
            let mut buf = [0u16; 1024];
            let mut len = buf.len() as u32;
            let r = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len);
            let _ = CloseHandle(h);
            r.ok()?;
            let full = String::from_utf16_lossy(&buf[..len as usize]);
            full.rsplit(['\\', '/']).next().map(|s| s.to_lowercase())
        }
    }

    fn ft(f: FILETIME) -> u64 {
        ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64
    }

    /// (kernel+user CPU time in 100 ns units, working set bytes)
    pub fn own_usage() -> Option<(u64, u64)> {
        unsafe {
            let p = GetCurrentProcess();
            let (mut c, mut x, mut k, mut u) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
            GetProcessTimes(p, &mut c, &mut x, &mut k, &mut u).ok()?;
            let mut m = PROCESS_MEMORY_COUNTERS { cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32, ..Default::default() };
            GetProcessMemoryInfo(p, &mut m, m.cb).ok()?;
            Some((ft(k) + ft(u), m.WorkingSetSize as u64))
        }
    }

    pub fn lower_priority() {
        unsafe {
            let _ = SetPriorityClass(GetCurrentProcess(), BELOW_NORMAL_PRIORITY_CLASS);
        }
    }

    pub fn tts_thread(rx: std::sync::mpsc::Receiver<(String, u8)>) {
        use windows::core::GUID;
        use windows::Win32::Media::Speech::ISpVoice;
        use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};
        const CLSID_SPVOICE: GUID = GUID::from_u128(0x96749377_3391_11d2_9ee3_00c04f797396);
        const SPF_ASYNC: u32 = 1;
        const SPF_PURGEBEFORESPEAK: u32 = 2;
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let voice: ISpVoice = match CoCreateInstance(&CLSID_SPVOICE, None, CLSCTX_ALL) {
                Ok(v) => v,
                Err(e) => {
                    log::warn!("text-to-speech unavailable: {e}");
                    return;
                }
            };
            while let Ok((text, vol)) = rx.recv() {
                let _ = voice.SetVolume(vol.min(100) as u16);
                let w: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
                let _ = voice.Speak(PCWSTR(w.as_ptr()), SPF_ASYNC | SPF_PURGEBEFORESPEAK, None);
            }
        }
    }

    /// (vendor name, GPU name, encoder family: "nvenc" / "amd" / "qsv") of the main GPU.
    pub fn gpu() -> Option<(String, String, String)> {
        use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE};
        unsafe {
            let f: IDXGIFactory1 = CreateDXGIFactory1().ok()?;
            let mut best: Option<(u64, u32, String)> = None;
            let mut i = 0;
            while let Ok(a) = f.EnumAdapters1(i) {
                i += 1;
                let Ok(d) = a.GetDesc1() else { continue };
                if (d.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0 {
                    continue;
                }
                let name = String::from_utf16_lossy(&d.Description[..d.Description.iter().position(|c| *c == 0).unwrap_or(d.Description.len())]);
                let mem = d.DedicatedVideoMemory as u64;
                if best.as_ref().is_none_or(|b| mem > b.0) {
                    best = Some((mem, d.VendorId, name));
                }
            }
            let (_, vendor, name) = best?;
            let (v, enc) = match vendor {
                0x10DE => ("NVIDIA", "nvenc"),
                0x1002 | 0x1022 => ("AMD", "amd"),
                0x8086 => ("Intel", "qsv"),
                _ => ("Unknown", "nvenc"),
            };
            Some((v.into(), name, enc.into()))
        }
    }
}

#[cfg(not(windows))]
mod win {
    pub fn process_names() -> Vec<String> {
        Vec::new()
    }
    pub fn foreground_exe() -> Option<String> {
        None
    }
    pub fn own_usage() -> Option<(u64, u64)> {
        None
    }
    pub fn lower_priority() {}
    pub fn tts_thread(rx: std::sync::mpsc::Receiver<(String, u8)>) {
        while let Ok((t, _)) = rx.recv() {
            log::info!("(tts) {t}");
        }
    }
    pub fn gpu() -> Option<(String, String, String)> {
        None
    }
}

use win::{foreground_exe, own_usage, process_names, tts_thread};
pub use win::{gpu, lower_priority};

pub fn is_process_running(name: &str) -> bool {
    process_names().iter().any(|p| p.eq_ignore_ascii_case(name))
}

/// Free bytes on the drive that holds `dir`.
#[cfg(windows)]
pub fn free_space(dir: &std::path::Path) -> Option<u64> {
    use windows::core::HSTRING;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let mut probe = dir.to_path_buf();
    while !probe.exists() {
        probe = probe.parent()?.to_path_buf();
    }
    let mut free = 0u64;
    unsafe { GetDiskFreeSpaceExW(&HSTRING::from(probe.as_os_str()), Some(&mut free), None, None).ok()? };
    Some(free)
}

#[cfg(not(windows))]
pub fn free_space(_dir: &std::path::Path) -> Option<u64> {
    None
}

/// Whether Windows apps are set to dark mode (Settings > Personalization > Colors).
#[cfg(windows)]
pub fn windows_prefers_dark() -> bool {
    use windows::core::w;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
    let mut v: u32 = 1;
    let mut len = 4u32;
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut v as *mut u32 as *mut _),
            Some(&mut len),
        )
    };
    r.is_ok() && v == 0
}

#[cfg(not(windows))]
pub fn windows_prefers_dark() -> bool {
    true
}

/// e.g. "Windows 11 Pro 24H2 (build 26100)" (for the test report).
#[cfg(windows)]
pub fn os_version() -> String {
    use windows::core::{w, PCWSTR};
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};
    let get = |name: PCWSTR| -> String {
        let mut buf = [0u16; 128];
        let mut len = (buf.len() * 2) as u32;
        let r = unsafe { RegGetValueW(HKEY_LOCAL_MACHINE, w!("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion"), name, RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr() as *mut _), Some(&mut len)) };
        if r.is_err() {
            return String::new();
        }
        let n = buf.iter().position(|c| *c == 0).unwrap_or(0);
        String::from_utf16_lossy(&buf[..n])
    };
    let build = get(w!("CurrentBuild"));
    // ProductName still says "Windows 10" on Windows 11; the build number tells them apart.
    let product = get(w!("ProductName"));
    let product = if build.parse::<u32>().unwrap_or(0) >= 22000 { product.replace("Windows 10", "Windows 11") } else { product };
    format!("{product} {} (build {build})", get(w!("DisplayVersion")))
}

#[cfg(not(windows))]
pub fn os_version() -> String {
    std::env::consts::OS.to_string()
}

/// Runs `f` with this thread in Windows' background mode (lowest CPU and disk I/O priority), so
/// thumbnails and clean-up never compete with anything else.
pub fn in_background_mode<T>(f: impl FnOnce() -> T) -> T {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_MODE_BACKGROUND_BEGIN, THREAD_MODE_BACKGROUND_END};
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_BEGIN);
        let r = f();
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_END);
        r
    }
    #[cfg(not(windows))]
    f()
}
