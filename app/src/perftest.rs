//! Performance test: measures what recording costs in a real League game.
//!
//! Phases (each `phase_secs` long, back to back, while you play):
//!   1. Baseline: nothing recording.
//!   2. Built-in recorder recording.
//! Measured per phase: League FPS and 1% lows (PresentMon, Intel's open-source frame-time tool,
//! which reads Windows' present events: no game access), League CPU, whole-PC CPU,
//! Clairvoyance's CPU and RAM, GPU 3D load and the video-encode engine load.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PhaseResult {
    pub name: String,
    pub secs: f64,
    pub frames: usize,
    pub fps_avg: Option<f64>,
    pub fps_1_low: Option<f64>,
    pub frametime_p99_ms: Option<f64>,
    pub game_cpu: f64,
    pub total_cpu: f64,
    pub app_cpu: f64,
    pub app_ram_mb: f64,
    pub gpu_3d: f64,
    pub gpu_encode: f64,
    pub game_gpu_3d: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PerfReport {
    pub started_at: String,
    pub gpu: String,
    pub encoder: String,
    pub fps_source: String,
    pub phases: Vec<PhaseResult>,
    pub notes: Vec<String>,
}

impl PerfReport {
    pub fn to_text(&self) -> String {
        let mut s = format!("Clairvoyance performance test, {}\nGPU: {}\nEncoder: {}\nFPS source: {}\n\n", self.started_at, self.gpu, self.encoder, self.fps_source);
        let f = |v: Option<f64>| v.map(|x| format!("{x:.1}")).unwrap_or("-".into());
        s.push_str("phase                 avg FPS  1% low  p99 ms  League CPU  PC CPU  app CPU  app RAM  GPU 3D  GPU encode\n");
        for p in &self.phases {
            s.push_str(&format!(
                "{:<20} {:>8} {:>7} {:>7} {:>10.1}% {:>6.1}% {:>7.2}% {:>6.0}MB {:>6.1}% {:>9.1}%\n",
                p.name, f(p.fps_avg), f(p.fps_1_low), f(p.frametime_p99_ms), p.game_cpu, p.total_cpu, p.app_cpu, p.app_ram_mb, p.gpu_3d, p.gpu_encode
            ));
        }
        if let (Some(base), true) = (self.phases.first(), self.phases.len() > 1) {
            s.push('\n');
            for p in &self.phases[1..] {
                if let (Some(a), Some(b)) = (base.fps_avg, p.fps_avg) {
                    s.push_str(&format!("{}: {:+.1}% average FPS vs baseline", p.name, (b - a) / a * 100.0));
                    if let (Some(a1), Some(b1)) = (base.fps_1_low, p.fps_1_low) {
                        s.push_str(&format!(", {:+.1}% 1% lows", (b1 - a1) / a1 * 100.0));
                    }
                    s.push('\n');
                }
            }
        }
        for n in &self.notes {
            s.push_str(&format!("Note: {n}\n"));
        }
        s
    }
}

/// Frame times (ms) with their time (seconds) from a PresentMon CSV (v1 or v2 columns).
/// Only rows for `process` (e.g. "League of Legends.exe").
pub fn parse_presentmon(csv: &str, process: &str) -> Vec<(f64, f64)> {
    let mut lines = csv.lines();
    let Some(header) = lines.next() else { return Vec::new() };
    let header = header.trim_start_matches('\u{feff}'); // PresentMon writes a UTF-8 BOM
    let cols: Vec<String> = header.split(',').map(|c| c.trim().to_string()).collect();
    let find = |names: &[&str]| cols.iter().position(|c| names.iter().any(|n| c.eq_ignore_ascii_case(n)));
    let app = find(&["Application", "ProcessName"]);
    let ft = find(&["MsBetweenPresents", "FrameTime", "MsBetweenAppStart", "msBetweenPresents"]);
    let time = find(&["TimeInSeconds", "CPUStartTime", "TimeInMs", "CPUStartTimeInMs"]);
    let (Some(ft), Some(time)) = (ft, time) else { return Vec::new() };
    let mut rows: Vec<(f64, f64)> = Vec::new();
    for l in lines {
        let f: Vec<&str> = l.split(',').collect();
        if let Some(a) = app {
            if !f.get(a).is_some_and(|v| v.trim().eq_ignore_ascii_case(process)) {
                continue;
            }
        }
        if let (Some(t), Some(ms)) = (f.get(time).and_then(|v| v.trim().parse::<f64>().ok()), f.get(ft).and_then(|v| v.trim().parse::<f64>().ok())) {
            if ms > 0.0 && ms < 2000.0 {
                rows.push((t, ms));
            }
        }
    }
    // Time unit: the column is seconds or milliseconds; the sum of frame times tells us which.
    if rows.len() > 10 {
        let span = rows.last().unwrap().0 - rows.first().unwrap().0;
        let total_ms: f64 = rows.iter().map(|r| r.1).sum();
        if span > total_ms / 1000.0 * 20.0 {
            for r in rows.iter_mut() {
                r.0 /= 1000.0;
            }
        }
        // Times stay relative to PresentMon's start (not its first League frame), so they
        // line up with the phase windows the test measured from PresentMon's launch.
    }
    rows
}

/// (avg fps, 1% low fps, p99 frame time ms, frames) for rows within [from, to] seconds.
pub fn fps_stats(rows: &[(f64, f64)], from: f64, to: f64) -> Option<(f64, f64, f64, usize)> {
    let mut ft: Vec<f64> = rows.iter().filter(|r| r.0 >= from && r.0 <= to).map(|r| r.1).collect();
    if ft.len() < 20 {
        return None;
    }
    let total: f64 = ft.iter().sum();
    let avg = ft.len() as f64 / (total / 1000.0);
    ft.sort_by(|a, b| a.total_cmp(b));
    let p99 = ft[((ft.len() as f64 * 0.99) as usize).min(ft.len() - 1)];
    // 1% low = average FPS over the slowest 1% of frames.
    let n = (ft.len() / 100).max(1);
    let worst: f64 = ft[ft.len() - n..].iter().sum::<f64>() / n as f64;
    Some((avg, 1000.0 / worst, p99, ft.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presentmon_v1_and_v2() {
        let mut v1 = String::from("Application,ProcessID,SwapChainAddress,Runtime,SyncInterval,PresentFlags,Dropped,TimeInSeconds,MsBetweenPresents\n");
        let mut v2 = String::from("Application,ProcessID,SwapChainAddress,PresentRuntime,SyncInterval,PresentFlags,AllowsTearing,PresentMode,FrameType,CPUStartTime,FrameTime,CPUBusy\n");
        let mut t = 0.0;
        for i in 0..2000 {
            let ms = if i % 100 == 0 { 20.0 } else { 5.0 };
            t += ms;
            v1.push_str(&format!("League of Legends.exe,1,0x1,DXGI,0,0,0,{:.6},{ms}\n", t / 1000.0));
            v2.push_str(&format!("League of Legends.exe,1,0x1,DXGI,0,0,1,Hardware,Application,{t:.4},{ms},1.0\n"));
            v1.push_str("dwm.exe,2,0x2,DXGI,1,0,0,1.0,16.6\n");
        }
        for csv in [&v1, &v2] {
            let rows = parse_presentmon(csv, "League of Legends.exe");
            assert_eq!(rows.len(), 2000);
            let (avg, low, p99, n) = fps_stats(&rows, 0.0, 1000.0).unwrap();
            assert!((avg - 194.17).abs() < 0.5, "avg {avg}");
            assert!((low - 50.0).abs() < 0.5, "1% low {low}");
            assert_eq!(p99, 20.0);
            assert_eq!(n, 2000);
            // Window selection in seconds.
            let (_, _, _, n) = fps_stats(&rows, 2.0, 4.0).unwrap();
            assert!(n > 300 && n < 450, "{n}");
        }
    }

    /// PresentMon 2 as it ran on the owner's PC: TimeInMs, and the first League frame long
    /// after PresentMon started (the game was still loading). Phase windows are measured from
    /// PresentMon's start, so times must not be shifted to the first frame.
    #[test]
    fn presentmon_time_in_ms_keeps_absolute_times() {
        let mut csv = String::from("Application,ProcessID,SwapChainAddress,PresentRuntime,SyncInterval,PresentFlags,AllowsTearing,PresentMode,TimeInMs,MsBetweenSimulationStart,MsBetweenPresents\n");
        let mut t = 33_000.0;
        while t < 100_000.0 {
            let ms = if t < 60_000.0 { 7.0 } else { 10.0 };
            t += ms;
            csv.push_str(&format!("League of Legends.exe,7476,0x1,DXGI,0,512,0,Composed: Flip,{t:.4},NA,{ms}\n"));
        }
        let rows = parse_presentmon(&csv, "League of Legends.exe");
        assert!((rows[0].0 - 33.0).abs() < 0.1, "first frame at {}", rows[0].0);
        let (avg, _, _, _) = fps_stats(&rows, 40.0, 58.0).unwrap();
        assert!((avg - 142.86).abs() < 0.5, "{avg}");
        let (avg, _, _, _) = fps_stats(&rows, 62.0, 98.0).unwrap();
        assert!((avg - 100.0).abs() < 0.5, "{avg}");
    }

}

// ---------------- running the test (Windows) ----------------

#[derive(Debug, Clone, Default, Serialize)]
pub struct PerfTestStatus {
    /// idle, preparing, waiting_for_game, running, analyzing, done, error
    pub state: String,
    pub phase: Option<String>,
    pub phase_ends_in: Option<f64>,
    pub message: Option<String>,
    pub report: Option<PerfReport>,
    pub report_text: Option<String>,
    pub report_path: Option<String>,
}

pub const GAME_EXE: &str = "League of Legends.exe";

#[cfg(windows)]
mod run {
    use super::*;
    use crate::state::AppState;
    use cv_core::engine::{EngineCommand, Platform};
    use cv_core::game::CaptureTarget;
    use cv_core::recorder::{RecordOptions, Recorder};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    fn set(st: &AppState, f: impl FnOnce(&mut PerfTestStatus)) {
        f(&mut st.perf.lock().unwrap());
    }

    async fn ensure_presentmon(dir: &Path) -> anyhow::Result<PathBuf> {
        let exe = dir.join("PresentMon.exe");
        if exe.is_file() {
            return Ok(exe);
        }
        std::fs::create_dir_all(dir)?;
        let client = reqwest::Client::builder().user_agent("Clairvoyance").build()?;
        let rel: serde_json::Value = client.get("https://api.github.com/repos/GameTechDev/PresentMon/releases/latest").send().await?.error_for_status()?.json().await?;
        let asset = rel["assets"]
            .as_array()
            .and_then(|a| {
                a.iter().find(|x| {
                    let n = x["name"].as_str().unwrap_or("").to_lowercase();
                    n.starts_with("presentmon") && n.ends_with("x64.exe") && !n.contains("service") && !n.contains("setup") && !n.contains("capture")
                })
            })
            .and_then(|x| x["browser_download_url"].as_str().map(str::to_string))
            .ok_or_else(|| anyhow::anyhow!("PresentMon download not found"))?;
        let bytes = client.get(&asset).send().await?.error_for_status()?.bytes().await?;
        std::fs::write(&exe, &bytes)?;
        Ok(exe)
    }

    /// Starts PresentMon as administrator (Windows asks once). It stops when League exits.
    /// Starts PresentMon elevated; returns its process handle (as an integer, so it's Send).
    fn launch_elevated(exe: &Path, csv: &Path) -> anyhow::Result<isize> {
        use windows::core::{w, HSTRING};
        use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
        let params = HSTRING::from(format!("--process_name \"{GAME_EXE}\" --output_file \"{}\" --terminate_on_proc_exit", csv.display()));
        let file = HSTRING::from(exe.as_os_str());
        let mut info = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS,
            lpVerb: w!("runas"),
            lpFile: windows::core::PCWSTR(file.as_ptr()),
            lpParameters: windows::core::PCWSTR(params.as_ptr()),
            nShow: 0, // hidden
            ..Default::default()
        };
        unsafe { ShellExecuteExW(&mut info) }.map_err(|e| anyhow::anyhow!("PresentMon wasn't started (admin prompt declined?): {e}"))?;
        Ok(info.hProcess.0 as isize)
    }

    /// True once the process has exited (or the handle is invalid).
    fn has_exited(h: isize) -> bool {
        use windows::Win32::Foundation::{HANDLE, WAIT_TIMEOUT};
        use windows::Win32::System::Threading::WaitForSingleObject;
        h == 0 || unsafe { WaitForSingleObject(HANDLE(h as *mut _), 0) } != WAIT_TIMEOUT
    }

    struct Acc {
        n: f64,
        s: cv_capture::win::perf::PerfSnapshot,
        app_cpu: f64,
        app_ram: f64,
    }

    pub async fn run(st: Arc<AppState>, phase_secs: u64) {
        let result = run_inner(&st, phase_secs).await;
        // Always restore normal recording.
        let _ = st.cmd.send(EngineCommand::ReloadSettings(Box::new(st.engine_settings(&st.settings()))));
        if let Err(e) = result {
            log::warn!("performance test: {e:#}");
            set(&st, |s| {
                s.state = "error".into();
                s.phase = None;
                s.message = Some(format!("{e:#}"));
            });
            st.platform.speak("Performance test stopped before the end. The results weren't saved.", 70);
        }
    }

    async fn run_inner(st: &Arc<AppState>, phase_secs: u64) -> anyhow::Result<()> {
        let started = chrono::Local::now();
        let out_dir = st.save_dir().join("perf-tests");
        std::fs::create_dir_all(&out_dir)?;
        let csv = out_dir.join(format!("presentmon_{}.csv", started.format("%Y-%m-%d_%H-%M-%S")));
        set(st, |s| {
            *s = PerfTestStatus { state: "preparing".into(), message: Some("Getting PresentMon (frame-time measurement)…".into()), ..Default::default() };
        });
        let mut notes = Vec::new();
        let mut pm_started: Option<Instant> = None;
        let mut pm_process: isize = 0;
        match ensure_presentmon(&st.paths.data_dir.join("tools")).await {
            Ok(exe) => match launch_elevated(&exe, &csv) {
                Ok(h) => {
                    pm_started = Some(Instant::now());
                    pm_process = h;
                }
                Err(e) => notes.push(format!("{e:#}. FPS wasn't measured; CPU/GPU still were.")),
            },
            Err(e) => notes.push(format!("PresentMon couldn't be downloaded ({e:#}). FPS wasn't measured; CPU/GPU still were.")),
        }

        // No automatic recording during the test: the test controls the recorders.
        let mut quiet = st.engine_settings(&st.settings());
        quiet.auto_record = false;
        let _ = st.cmd.send(EngineCommand::ReloadSettings(Box::new(quiet)));

        set(st, |s| {
            s.state = "waiting_for_game".into();
            s.message = Some("Start a League game (Practice Tool is ideal). The test begins by itself once you're in game.".into());
        });
        let wait_start = Instant::now();
        loop {
            if st.perf_cancel.load(std::sync::atomic::Ordering::SeqCst) {
                anyhow::bail!("cancelled");
            }
            let l = st.live.lock().unwrap().clone();
            if l.game_id.as_deref() == Some("league") && l.phase == cv_core::game::MatchPhase::InProgress {
                break;
            }
            if wait_start.elapsed() > Duration::from_secs(30 * 60) {
                anyhow::bail!("no League game started within 30 minutes");
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        let total_min = ((phase_secs + 6) * 2 + 10) as f64 / 60.0;
        st.platform.speak(
            &format!("Performance test starts in 10 seconds. Keep playing for about {} minutes, until you hear: performance test finished.", total_min.ceil() as u64),
            70,
        );
        tokio::time::sleep(Duration::from_secs(10)).await;

        let settings = st.engine_settings(&st.settings());
        let target = CaptureTarget { exe: GAME_EXE.into(), display_capture_only: false };
        let rec_dir = out_dir.join("_recordings");
        let opts = RecordOptions {
            output_dir: rec_dir.clone(),
            encoder: settings.video.encoder.clone(),
            quality: settings.video.quality.clone(),
            fps: settings.video.fps,
            height: settings.video.height,
            replay_buffer_secs: settings.video.replay_buffer_secs,
            record_mic: settings.video.record_mic,
            display_capture: settings.video.display_capture,
            full_video: true,
        };
        let phases: Vec<(&str, Option<Arc<dyn Recorder>>)> = vec![("Not recording", None), ("Built-in recorder", Some(st.recorder.clone() as Arc<dyn Recorder>))];
        let mut windows: Vec<(String, f64, f64, Acc)> = Vec::new();
        let mut encoder = String::new();
        for (name, rec) in phases {
            if st.perf_cancel.load(std::sync::atomic::Ordering::SeqCst) {
                anyhow::bail!("cancelled");
            }
            if let Some(r) = &rec {
                r.ensure_connected().await?;
                r.prepare(&target, &opts).await?;
                r.start_recording().await?;
                if let Some(e) = r.status().await.encoder {
                    if encoder.is_empty() {
                        encoder = e;
                    }
                }
                tokio::time::sleep(Duration::from_secs(3)).await; // let it settle
            }
            st.platform.speak(&format!("Test phase: {name}"), 70);
            let pids = cv_capture::win::window::pids_for_exe(GAME_EXE);
            let t0 = pm_started.map(|p| p.elapsed().as_secs_f64()).unwrap_or(0.0);
            let mut pc = cv_capture::win::perf::PerfCounters::new();
            let _ = st.platform.perf_sample();
            let mut acc = Acc { n: 0.0, s: Default::default(), app_cpu: 0.0, app_ram: 0.0 };
            let end = Instant::now() + Duration::from_secs(phase_secs);
            while Instant::now() < end {
                set(st, |s| {
                    s.state = "running".into();
                    s.phase = Some(name.into());
                    s.phase_ends_in = Some(end.saturating_duration_since(Instant::now()).as_secs_f64());
                    s.message = None;
                });
                tokio::time::sleep(Duration::from_secs(1)).await;
                if st.perf_cancel.load(std::sync::atomic::Ordering::SeqCst) {
                    if let Some(r) = &rec {
                        let _ = r.stop_recording().await;
                        let _ = r.finish().await;
                    }
                    let _ = std::fs::remove_dir_all(&rec_dir);
                    anyhow::bail!("cancelled");
                }
                let in_game = {
                    let l = st.live.lock().unwrap();
                    l.game_id.as_deref() == Some("league") && l.phase == cv_core::game::MatchPhase::InProgress
                };
                if !in_game {
                    if let Some(r) = &rec {
                        let _ = r.stop_recording().await;
                        let _ = r.finish().await;
                    }
                    let _ = std::fs::remove_dir_all(&rec_dir);
                    anyhow::bail!(
                        "You left the game during the \"{name}\" phase, before the test finished. Run it again and keep playing until you hear \"Performance test finished\"."
                    );
                }
                if let Some(p) = pc.as_mut() {
                    let x = p.sample("League of Legends", pids.first().copied());
                    acc.s.gpu_3d += x.gpu_3d;
                    acc.s.gpu_encode += x.gpu_encode;
                    acc.s.game_gpu_3d += x.game_gpu_3d;
                    acc.s.cpu_total += x.cpu_total;
                    acc.s.game_cpu += x.game_cpu;
                }
                if let Some((cpu, ram)) = st.platform.perf_sample() {
                    acc.app_cpu += cpu;
                    acc.app_ram = acc.app_ram.max(ram);
                }
                acc.n += 1.0;
            }
            let t1 = pm_started.map(|p| p.elapsed().as_secs_f64()).unwrap_or(0.0);
            if let Some(r) = &rec {
                if let Ok(path) = r.stop_recording().await {
                    let _ = std::fs::remove_file(path);
                }
                let _ = r.finish().await;
            }
            log::info!("performance test phase {name}: PresentMon time {t0:.1}-{t1:.1} s");
            windows.push((name.to_string(), t0, t1, acc));
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
        let _ = std::fs::remove_dir_all(&rec_dir);
        // PresentMon only writes its file completely when it exits, which it does when the
        // game closes. So the FPS numbers are ready once you leave the game.
        if pm_process != 0 && !has_exited(pm_process) {
            st.platform.speak("Performance test finished. Leave the game to see the results.", 70);
            set(st, |s| {
                s.state = "waiting_for_exit".into();
                s.phase = None;
                s.message = Some("Measuring done. Leave the game (close League) to see the results.".into());
            });
            let wait = Instant::now();
            while !has_exited(pm_process) && wait.elapsed() < Duration::from_secs(60 * 60) {
                if st.perf_cancel.load(std::sync::atomic::Ordering::SeqCst) {
                    notes.push("Results shown before PresentMon finished writing, so FPS may be missing.".into());
                    break;
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        } else {
            st.platform.speak("Performance test finished. You can leave the game.", 70);
        }
        if pm_process != 0 {
            unsafe {
                let _ = windows::Win32::Foundation::CloseHandle(windows::Win32::Foundation::HANDLE(pm_process as *mut _));
            }
        }
        set(st, |s| {
            s.state = "analyzing".into();
            s.phase = None;
            s.message = None;
        });
        tokio::time::sleep(Duration::from_secs(2)).await;

        let rows = std::fs::read_to_string(&csv).map(|t| parse_presentmon(&t, GAME_EXE)).unwrap_or_default();
        let fps_source = if rows.is_empty() {
            if pm_started.is_some() {
                notes.push("PresentMon ran but recorded no League frames (keep the game in focus during the test).".into());
            }
            "unavailable (read FPS in game with Ctrl+F)".to_string()
        } else {
            "PresentMon (frame times from Windows)".to_string()
        };
        // PresentMon's clock starts at its first frame; align on the end of the trace as a check.
        let mut phases_out = Vec::new();
        for (name, t0, t1, a) in windows {
            let n = a.n.max(1.0);
            let fps = fps_stats(&rows, t0 + 3.0, t1 - 1.0);
            phases_out.push(PhaseResult {
                name,
                secs: t1 - t0,
                frames: fps.map(|f| f.3).unwrap_or(0),
                fps_avg: fps.map(|f| f.0),
                fps_1_low: fps.map(|f| f.1),
                frametime_p99_ms: fps.map(|f| f.2),
                game_cpu: a.s.game_cpu / n,
                total_cpu: a.s.cpu_total / n,
                app_cpu: a.app_cpu / n,
                app_ram_mb: a.app_ram,
                gpu_3d: a.s.gpu_3d / n,
                gpu_encode: a.s.gpu_encode / n,
                game_gpu_3d: a.s.game_gpu_3d / n,
            });
        }
        let report = PerfReport {
            started_at: started.format("%Y-%m-%d %H:%M").to_string(),
            gpu: st.gpu.as_ref().map(|g| g.name.clone()).unwrap_or_default(),
            encoder,
            fps_source,
            phases: phases_out,
            notes,
        };
        let text = report.to_text();
        let base = out_dir.join(format!("perf-test_{}", started.format("%Y-%m-%d_%H-%M-%S")));
        std::fs::write(base.with_extension("json"), serde_json::to_vec_pretty(&report)?)?;
        std::fs::write(base.with_extension("txt"), &text)?;
        log::info!("performance test:\n{text}");
        set(st, |s| {
            s.state = "done".into();
            s.report = Some(report);
            s.report_text = Some(text);
            s.report_path = Some(base.with_extension("txt").to_string_lossy().to_string());
        });
        Ok(())
    }
}

#[cfg(windows)]
pub use run::run;

#[cfg(not(windows))]
pub async fn run(st: std::sync::Arc<crate::state::AppState>, _phase_secs: u64) {
    st.perf.lock().unwrap().state = "error".into();
    st.perf.lock().unwrap().message = Some("The performance test only runs on Windows.".into());
}
