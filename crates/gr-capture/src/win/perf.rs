//! System performance counters (no admin rights needed): GPU engine utilization per process
//! (3D and the Video Encode engine NVENC/AMF/QSV runs on), CPU per process and in total.

use std::collections::HashMap;
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::System::Performance::*;

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct PerfSnapshot {
    /// Whole-GPU 3D engine use in % (sum over processes, capped at 100).
    pub gpu_3d: f64,
    /// Video encode engine use in % (NVENC/AMF/QSV).
    pub gpu_encode: f64,
    /// 3D engine use by the watched game process.
    pub game_gpu_3d: f64,
    /// All GPU engines used by this app.
    pub app_gpu: f64,
    /// Total CPU use of the PC in %.
    pub cpu_total: f64,
    /// CPU use of the game process, % of the whole CPU.
    pub game_cpu: f64,
}

pub struct PerfCounters {
    query: PDH_HQUERY,
    gpu: PDH_HCOUNTER,
    cpu_total: PDH_HCOUNTER,
    proc_cpu: PDH_HCOUNTER,
    cores: f64,
}

unsafe impl Send for PerfCounters {}

fn array(counter: PDH_HCOUNTER) -> Vec<(String, f64)> {
    let mut out = Vec::new();
    unsafe {
        let mut size = 0u32;
        let mut count = 0u32;
        let r = PdhGetFormattedCounterArrayW(counter, PDH_FMT_DOUBLE, &mut size, &mut count, None);
        if r != PDH_MORE_DATA || size == 0 {
            return out;
        }
        let mut buf = vec![0u8; size as usize + 16];
        let items = buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
        if PdhGetFormattedCounterArrayW(counter, PDH_FMT_DOUBLE, &mut size, &mut count, Some(items)) != 0 {
            return out;
        }
        for i in 0..count as usize {
            let it = &*items.add(i);
            if it.FmtValue.CStatus == PDH_CSTATUS_VALID_DATA || it.FmtValue.CStatus == 1 {
                let name = it.szName.to_string().unwrap_or_default();
                out.push((name, it.FmtValue.Anonymous.doubleValue));
            }
        }
    }
    out
}

impl PerfCounters {
    pub fn new() -> Option<PerfCounters> {
        unsafe {
            let mut q = PDH_HQUERY::default();
            if PdhOpenQueryW(PCWSTR::null(), 0, &mut q) != 0 {
                return None;
            }
            let add = |path: &str| -> PDH_HCOUNTER {
                let mut c = PDH_HCOUNTER::default();
                let _ = PdhAddEnglishCounterW(q, &HSTRING::from(path), 0, &mut c);
                c
            };
            let gpu = add("\\GPU Engine(*)\\Utilization Percentage");
            let cpu_total = add("\\Processor(_Total)\\% Processor Time");
            let proc_cpu = add("\\Process(*)\\% Processor Time");
            let _ = PdhCollectQueryData(q);
            let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1) as f64;
            Some(PerfCounters { query: q, gpu, cpu_total, proc_cpu, cores })
        }
    }

    /// Values since the previous call. `game_exe` without ".exe", e.g. "League of Legends".
    pub fn sample(&mut self, game_exe: &str, game_pid: Option<u32>) -> PerfSnapshot {
        let mut s = PerfSnapshot::default();
        unsafe {
            if PdhCollectQueryData(self.query) != 0 {
                return s;
            }
            let me = std::process::id();
            let mut by_type: HashMap<String, f64> = HashMap::new();
            for (name, v) in array(self.gpu) {
                // pid_1234_luid_0x..._phys_0_eng_3_engtype_VideoEncode
                let ty = name.rsplit("engtype_").next().unwrap_or("").to_string();
                let pid: u32 = name.strip_prefix("pid_").and_then(|r| r.split('_').next()).and_then(|p| p.parse().ok()).unwrap_or(0);
                *by_type.entry(ty.clone()).or_default() += v;
                if pid == me {
                    s.app_gpu += v;
                }
                if Some(pid) == game_pid && ty == "3D" {
                    s.game_gpu_3d += v;
                }
            }
            s.gpu_3d = by_type.get("3D").copied().unwrap_or(0.0).min(100.0);
            s.gpu_encode = by_type.iter().filter(|(k, _)| k.contains("Encode")).map(|(_, v)| *v).sum::<f64>().min(100.0);
            let mut v = PDH_FMT_COUNTERVALUE::default();
            if PdhGetFormattedCounterValue(self.cpu_total, PDH_FMT_DOUBLE, None, &mut v) == 0 {
                s.cpu_total = v.Anonymous.doubleValue;
            }
            let want = game_exe.trim_end_matches(".exe").to_lowercase();
            for (name, v) in array(self.proc_cpu) {
                // Instances are "name", "name#1", ...
                let base = name.split('#').next().unwrap_or("").to_lowercase();
                if base == want {
                    s.game_cpu += v / self.cores;
                }
            }
        }
        s
    }
}

impl Drop for PerfCounters {
    fn drop(&mut self) {
        unsafe {
            let _ = PdhCloseQuery(self.query);
        }
    }
}
