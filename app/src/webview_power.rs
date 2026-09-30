//! Makes the hidden window (nearly) free: while Clairvoyance sits in the tray with its UI
//! kept loaded for instant reopening, the WebView is marked invisible, its page is suspended
//! (no scripts, timers or rendering) and Chromium is asked to trim its memory.

/// `background = true` when the window is being hidden; `false` right after showing it.
#[cfg(windows)]
pub fn set_background(w: &tauri::WebviewWindow, background: bool) {
    let r = w.with_webview(move |pw| unsafe {
        use webview2_com::Microsoft::Web::WebView2::Win32::*;
        use windows_core_062::Interface;
        let c = pw.controller();
        let _ = c.SetIsVisible(!background);
        let Ok(core) = c.CoreWebView2() else { return };
        if let Ok(v19) = core.cast::<ICoreWebView2_19>() {
            let level = if background { COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW } else { COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL };
            let _ = v19.SetMemoryUsageTargetLevel(level);
        }
        if let Ok(v3) = core.cast::<ICoreWebView2_3>() {
            if background {
                let done = webview2_com::TrySuspendCompletedHandler::create(Box::new(|_, _| Ok(())));
                let _ = v3.TrySuspend(&done);
            } else {
                let _ = v3.Resume();
            }
        }
    });
    if let Err(e) = r {
        log::debug!("webview power: {e}");
    }
}

#[cfg(not(windows))]
pub fn set_background(_w: &tauri::WebviewWindow, _background: bool) {}
