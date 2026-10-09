//! Start-up tamper check: release builds are sealed after compiling (crates/cv-seal, run by the
//! Release workflow) and refuse to start if the executable was changed since: a renamed,
//! rebranded or patched copy shows a message and exits. Off in dev and test builds.
//!
//! Runs once, on its own thread, while the app starts (reads and hashes the executable: a few
//! tens of ms of CPU, never during a game unless the app is started in one). An unreadable
//! executable is let through (logged), so an antivirus lock can't stop a real copy.

include!(concat!(env!("OUT_DIR"), "/seal_config.rs"));

/// Where the seal's signature goes (filled in by `cv-seal seal` after the build).
#[used]
static SEAL_REGION: [u8; cv_seal::REGION_LEN] = cv_seal::REGION_INIT;

/// The official download page, masked like the key.
const OFFICIAL: [u8; 64] = cv_seal::mask(*b"https://github.com/Emre-98/Clairvoyance-releases/releases/latest");

pub fn spawn_check() {
    // Keeps the region in the executable even when the check is off.
    std::hint::black_box(&SEAL_REGION);
    if !SEAL_REQUIRED {
        return;
    }
    let _ = std::thread::Builder::new().name("integrity".into()).spawn(|| {
        let t = std::time::Instant::now();
        let exe = match std::env::current_exe().and_then(std::fs::read) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("integrity: can't read the program file ({e}), check skipped");
                return;
            }
        };
        let masked = cv_seal::unmask(SEAL_PUBKEY_MASKED);
        let key: [u8; 32] = std::array::from_fn(|i| masked[i] ^ cv_seal::unmask(SEAL_PUBKEY_MASK)[i]);
        let result = cv_seal::verify(&exe, &key);
        drop(exe);
        match result {
            Ok(()) => log::info!("integrity: ok ({} ms)", t.elapsed().as_millis()),
            Err(e) => {
                log::error!("integrity: {e}");
                refuse();
            }
        }
    });
}

fn refuse() -> ! {
    let url = cv_seal::unmask(OFFICIAL);
    let text = format!("This copy of Clairvoyance has been modified and won't start.\n\nDownload the official version from\n{}", String::from_utf8_lossy(&url));
    #[cfg(windows)]
    unsafe {
        use windows::core::HSTRING;
        use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
        MessageBoxW(None, &HSTRING::from(text), &HSTRING::from("Clairvoyance"), MB_OK | MB_ICONERROR);
    }
    #[cfg(not(windows))]
    eprintln!("{text}");
    std::process::exit(1)
}
