//! "Start with Windows": a value under HKCU\...\Run that launches the app minimized.

#[cfg(windows)]
pub fn set(enabled: bool) -> anyhow::Result<()> {
    use windows::core::w;
    use windows::Win32::System::Registry::{RegDeleteKeyValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ};
    let key = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
    let name = w!("GameRecorder");
    unsafe {
        if enabled {
            let exe = std::env::current_exe()?;
            let value = format!("\"{}\" --minimized", exe.display());
            let wide: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
            let r = RegSetKeyValueW(HKEY_CURRENT_USER, key, name, REG_SZ.0, Some(wide.as_ptr() as *const _), (wide.len() * 2) as u32);
            if r.is_err() {
                anyhow::bail!("couldn't write the autostart registry value ({:?})", r);
            }
        } else {
            let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, key, name);
        }
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn set(_enabled: bool) -> anyhow::Result<()> {
    Ok(())
}
