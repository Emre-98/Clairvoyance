//! Tiny file logger: `%LOCALAPPDATA%\GameRecorder\logs\gamerecorder.log`, rotated at 2 MB.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

struct FileLogger {
    path: PathBuf,
    file: Mutex<Option<std::fs::File>>,
}

impl log::Log for FileLogger {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= log::Level::Info || (m.level() <= log::Level::Debug && m.target().starts_with("gr"))
    }

    fn log(&self, r: &log::Record) {
        if !self.enabled(r.metadata()) {
            return;
        }
        let line = format!("{} {:5} [{}] {}\n", chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"), r.level(), r.target(), r.args());
        #[cfg(debug_assertions)]
        eprint!("{line}");
        let mut g = self.file.lock().unwrap();
        if g.as_ref().and_then(|f| f.metadata().ok()).is_some_and(|m| m.len() > 2 * 1024 * 1024) {
            *g = None;
            let _ = std::fs::rename(&self.path, self.path.with_extension("old.log"));
        }
        if g.is_none() {
            *g = std::fs::OpenOptions::new().create(true).append(true).open(&self.path).ok();
        }
        if let Some(f) = g.as_mut() {
            let _ = f.write_all(line.as_bytes());
        }
    }

    fn flush(&self) {
        if let Some(f) = self.file.lock().unwrap().as_mut() {
            let _ = f.flush();
        }
    }
}

pub fn init(dir: PathBuf) -> PathBuf {
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("gamerecorder.log");
    let logger = FileLogger { path: path.clone(), file: Mutex::new(None) };
    if log::set_boxed_logger(Box::new(logger)).is_ok() {
        log::set_max_level(log::LevelFilter::Debug);
    }
    path
}
