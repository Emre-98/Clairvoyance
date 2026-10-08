//! Clip cutting with ffmpeg. Fast mode copies the video stream (no re-encoding, near
//! zero CPU); precise mode re-encodes on the GPU. ffmpeg is downloaded on demand.

use async_trait::async_trait;
use cv_core::engine::ClipCutter;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const DOWNLOAD_URL: &str = "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-gpl.zip";

pub struct Ffmpeg {
    configured: Mutex<String>,
    local_dir: PathBuf,
    /// GPU encoder for precise mode: "nvenc", "amd", "qsv".
    gpu: String,
}

impl Ffmpeg {
    pub fn new(configured: String, local_dir: PathBuf, gpu: String) -> Self {
        Self { configured: Mutex::new(configured), local_dir, gpu }
    }

    pub fn set_configured(&self, p: String) {
        *self.configured.lock().unwrap() = p;
    }

    pub fn local_exe(&self) -> PathBuf {
        self.local_dir.join(if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" })
    }

    pub fn locate(&self) -> Option<PathBuf> {
        let c = self.configured.lock().unwrap().clone();
        if !c.trim().is_empty() && Path::new(c.trim()).is_file() {
            return Some(PathBuf::from(c.trim()));
        }
        let local = self.local_exe();
        if local.is_file() {
            return Some(local);
        }
        let exe = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
        std::env::var_os("PATH").and_then(|paths| std::env::split_paths(&paths).map(|d| d.join(exe)).find(|p| p.is_file()))
    }

    /// The H.264 encoder for exports that re-encode (the input overlay burned in, the Discord
    /// copy): the GPU's own, else Windows' Media Foundation one, else x264. Tried once (a tenth
    /// of a second of black) and remembered while the app runs.
    pub fn h264_encoder(&self, exe: &Path) -> String {
        static PICKED: Mutex<Option<String>> = Mutex::new(None);
        if let Some(e) = PICKED.lock().unwrap().clone() {
            return e;
        }
        let gpu_enc = match self.gpu.as_str() {
            "amd" => "h264_amf",
            "qsv" => "h264_qsv",
            _ => "h264_nvenc",
        };
        let picked = [gpu_enc, "h264_mf"]
            .into_iter()
            .find(|e| cv_capture::overlay_export::encoder_works(exe, &["-c:v".to_string(), e.to_string()]))
            .unwrap_or("libx264")
            .to_string();
        log::info!("export encoder: {picked}");
        *PICKED.lock().unwrap() = Some(picked.clone());
        picked
    }

    /// Encoder arguments for the input-overlay export (see [`Ffmpeg::h264_encoder`]).
    pub fn overlay_encoder(&self, exe: &Path) -> Vec<String> {
        let e = self.h264_encoder(exe);
        let a: &[&str] = if e == "libx264" { &["-c:v", "libx264", "-preset", "veryfast", "-crf", "20"] } else { &["-c:v", &e, "-b:v", "16M"] };
        a.iter().map(|s| s.to_string()).collect()
    }

    async fn run(&self, exe: &Path, args: &[String]) -> anyhow::Result<()> {
        let mut cmd = tokio::process::Command::new(exe);
        cmd.args(args).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::piped());
        #[cfg(windows)]
        {
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;
            cmd.creation_flags(CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS);
        }
        let out = cmd.output().await?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            let tail: String = err.lines().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join(" | ");
            anyhow::bail!("ffmpeg failed: {tail}");
        }
        Ok(())
    }

    /// Downloads ffmpeg.exe into the app folder. `progress(downloaded, total)`.
    pub async fn download(&self, progress: impl Fn(u64, u64)) -> anyhow::Result<PathBuf> {
        std::fs::create_dir_all(&self.local_dir)?;
        let zip_path = self.local_dir.join("ffmpeg-download.zip");
        let client = reqwest::Client::builder().build()?;
        let mut resp = client.get(DOWNLOAD_URL).send().await?.error_for_status()?;
        let total = resp.content_length().unwrap_or(0);
        let mut file = std::fs::File::create(&zip_path)?;
        let mut done = 0u64;
        let mut last = 0u64;
        while let Some(chunk) = resp.chunk().await? {
            std::io::Write::write_all(&mut file, &chunk)?;
            done += chunk.len() as u64;
            if done - last > 512 * 1024 {
                progress(done, total);
                last = done;
            }
        }
        drop(file);
        progress(done, total);
        let exe = self.local_exe();
        let zp = zip_path.clone();
        let ex = exe.clone();
        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let mut zip = zip::ZipArchive::new(std::fs::File::open(&zp)?)?;
            for i in 0..zip.len() {
                let mut f = zip.by_index(i)?;
                if f.name().ends_with("bin/ffmpeg.exe") || f.name().ends_with("bin/ffmpeg") {
                    let mut out = std::fs::File::create(&ex)?;
                    std::io::copy(&mut f, &mut out)?;
                    return Ok(());
                }
            }
            anyhow::bail!("ffmpeg.exe not found in the download")
        })
        .await??;
        let _ = std::fs::remove_file(&zip_path);
        Ok(exe)
    }
}

#[async_trait]
impl ClipCutter for Ffmpeg {
    async fn cut(&self, video: &Path, start: f64, end: f64, out: &Path, precise: bool) -> anyhow::Result<()> {
        let exe = self.locate().ok_or_else(|| anyhow::anyhow!("ffmpeg isn't installed yet (Settings > Clips > Download ffmpeg)"))?;
        let dur = (end - start).max(0.5);
        let base = |extra: &[&str]| -> Vec<String> {
            let mut a: Vec<String> = vec!["-hide_banner".into(), "-y".into(), "-ss".into(), format!("{start:.3}"), "-i".into(), video.to_string_lossy().to_string(), "-t".into(), format!("{dur:.3}")];
            a.extend(extra.iter().map(|s| s.to_string()));
            a.extend(["-movflags".into(), "+faststart".into(), out.to_string_lossy().to_string()]);
            a
        };
        if !precise {
            return self.run(&exe, &base(&["-map", "0", "-c", "copy", "-avoid_negative_ts", "make_zero"])).await;
        }
        let gpu_enc = match self.gpu.as_str() {
            "amd" => "h264_amf",
            "qsv" => "h264_qsv",
            _ => "h264_nvenc",
        };
        let attempts: [&[&str]; 3] = [
            &["-map", "0", "-c:v", gpu_enc, "-b:v", "12M", "-c:a", "aac", "-b:a", "160k"],
            &["-map", "0", "-c:v", "h264_mf", "-b:v", "12M", "-c:a", "aac", "-b:a", "160k"],
            &["-map", "0", "-c:v", "libx264", "-preset", "veryfast", "-crf", "20", "-c:a", "aac", "-b:a", "160k"],
        ];
        let mut last = None;
        for a in attempts {
            match self.run(&exe, &base(a)).await {
                Ok(()) => return Ok(()),
                Err(e) => last = Some(e),
            }
        }
        Err(last.unwrap())
    }
}
