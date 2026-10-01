//! Console self-test for the built-in recorder.
//!
//!   recorder-selftest [seconds] [--exe "League of Legends.exe"] [--mic]
//!
//! Records the screen (or the given program's window) with the hardware encoder and prints
//! what happened: encoder, frames captured/dropped, CPU and GPU use, file size.

#[cfg(windows)]
fn main() {
    use cv_capture::win::{d3d, perf::PerfCounters, NativeRecorder};
    use std::time::{Duration, Instant};
    let args: Vec<String> = std::env::args().skip(1).collect();
    let secs: u64 = args.iter().find_map(|a| a.parse().ok()).unwrap_or(10);
    let exe = args.iter().position(|a| a == "--exe").and_then(|i| args.get(i + 1)).cloned();
    let mic = args.iter().any(|a| a == "--mic");
    // Results go next to this program, in "selftest-output".
    let out = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("selftest-output"))).unwrap_or_else(|| "selftest-output".into());
    let _ = std::fs::remove_dir_all(&out);
    let _ = std::fs::create_dir_all(&out);
    // Step-by-step log (written immediately, so it survives a crash).
    if let Ok(f) = std::fs::File::create(out.join("selftest-log.txt")) {
        let _ = log::set_boxed_logger(Box::new(StepLog(std::sync::Mutex::new(f))));
        log::set_max_level(log::LevelFilter::Info);
    }
    cv_capture::win::install_crash_logging();
    let mut report = String::new();
    macro_rules! say { ($($t:tt)*) => {{ let l = format!($($t)*); println!("{l}"); report.push_str(&l); report.push('\n'); }} }

    say!("Clairvoyance recorder self-test");
    match NativeRecorder::available_encoders() {
        Ok((gpu, encs)) => {
            say!("GPU: {gpu}");
            for e in &encs {
                say!("  hardware encoder: {e}");
            }
            if encs.is_empty() {
                say!("  NO hardware H.264 encoder found");
            }
        }
        Err(e) => say!("GPU/encoder check failed: {e:#}"),
    }
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let st = stop.clone();
    let game = exe.clone().unwrap_or_default();
    let sampler = std::thread::spawn(move || {
        let mut pc = PerfCounters::new();
        let mut samples = Vec::new();
        let t0 = Instant::now();
        let mut last_cpu = own_cpu();
        while !st.load(std::sync::atomic::Ordering::SeqCst) {
            std::thread::sleep(Duration::from_secs(1));
            let c = own_cpu();
            let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1) as f64;
            let app_cpu = (c - last_cpu) as f64 / 1e7 / cores * 100.0;
            last_cpu = c;
            if let Some(p) = pc.as_mut() {
                samples.push((p.sample(&game, None), app_cpu));
            }
        }
        (samples, t0.elapsed())
    });
    let t = Instant::now();
    let r = cv_capture::win::self_test(&out, secs, exe.as_deref(), mic);
    stop.store(true, std::sync::atomic::Ordering::SeqCst);
    let (samples, _) = sampler.join().unwrap();
    match r {
        Ok((path, frames, dropped)) => {
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            say!("Recorded {:.1} s: {} frames ({:.1} fps), {} dropped", t.elapsed().as_secs_f64(), frames, frames as f64 / secs as f64, dropped);
            say!("File: {} ({:.1} MB, {:.1} Mbit/s)", path.display(), size as f64 / 1e6, size as f64 * 8.0 / 1e6 / secs as f64);
            let clips: Vec<_> = std::fs::read_dir(&out).map(|d| d.flatten().filter(|e| e.file_name().to_string_lossy().starts_with("Replay_")).map(|e| e.path()).collect()).unwrap_or_default();
            say!("Replay clip: {}", clips.first().map(|p| p.display().to_string()).unwrap_or("none".into()));
            say!("Thumbnail: {}", if out.join("selftest.jpg").exists() { "ok" } else { "missing" });
        }
        Err(e) => say!("RECORDING FAILED: {e:#}"),
    }
    if !samples.is_empty() {
        let n = samples.len() as f64;
        let avg = |f: &dyn Fn(&(cv_capture::win::perf::PerfSnapshot, f64)) -> f64| samples.iter().map(f).sum::<f64>() / n;
        say!("Average over the test: this program {:.2}% CPU, GPU video-encode engine {:.1}%, GPU 3D {:.1}%, this program's GPU use {:.1}%, whole PC CPU {:.1}%",
            avg(&|s| s.1), avg(&|s| s.0.gpu_encode), avg(&|s| s.0.gpu_3d), avg(&|s| s.0.app_gpu), avg(&|s| s.0.cpu_total));
    }
    let _ = d3d::qpc_hns();
    let _ = std::fs::write(out.join("selftest-report.txt"), &report);
    println!("Report saved to {}", out.join("selftest-report.txt").display());
    // Started by double-click: keep the window open. From a script: don't wait for a key.
    use std::io::IsTerminal;
    if std::io::stdin().is_terminal() && !std::env::args().any(|a| a == "--no-wait") {
        println!("Press Enter to close.");
        let _ = std::io::stdin().read_line(&mut String::new());
    }
}

#[cfg(windows)]
struct StepLog(std::sync::Mutex<std::fs::File>);

#[cfg(windows)]
impl log::Log for StepLog {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= log::Level::Info
    }
    fn log(&self, r: &log::Record) {
        use std::io::Write;
        let line = format!("{} {:5} {}\n", chrono::Local::now().format("%H:%M:%S%.3f"), r.level(), r.args());
        print!("  {line}");
        let mut f = self.0.lock().unwrap();
        let _ = f.write_all(line.as_bytes());
        let _ = f.flush();
    }
    fn flush(&self) {}
}

#[cfg(windows)]
fn own_cpu() -> u64 {
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
    unsafe {
        let (mut a, mut b, mut k, mut u) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
        let _ = GetProcessTimes(GetCurrentProcess(), &mut a, &mut b, &mut k, &mut u);
        let f = |t: FILETIME| ((t.dwHighDateTime as u64) << 32) | t.dwLowDateTime as u64;
        f(k) + f(u)
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("The recorder self-test only runs on Windows.");
}
