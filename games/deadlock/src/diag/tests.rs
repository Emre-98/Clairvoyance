use super::*;

/// Collects the lines instead of writing a file.
#[derive(Default)]
struct Mem(Vec<(String, String)>);

impl Sink for Mem {
    fn line(&mut self, source: &str, text: &str) {
        self.0.push((source.to_string(), text.to_string()));
    }
}

impl Mem {
    /// The lines since the last call, as "source: text".
    fn take(&mut self) -> Vec<String> {
        self.0.drain(..).map(|(s, t)| format!("{s}: {t}")).collect()
    }
}

fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("cv-deadlock-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn append(path: &Path, text: &str) {
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path).unwrap();
    f.write_all(text.as_bytes()).unwrap();
}

#[test]
fn a_log_that_appears_is_followed_line_by_line() {
    let dir = temp("tail");
    let path = dir.join("console.log");
    let mut out = Mem::default();
    let mut tail = Tail::new(path.clone(), false, false);
    tail.poll(&mut out);
    assert!(out.take().is_empty(), "no file yet: nothing to say");

    append(&path, "10/09 14:03:22 [Client] one\r\n10/09 14:03:23 half");
    tail.poll(&mut out);
    let lines = out.take();
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(lines[0].starts_with("console.log: (the file appeared: "), "{lines:?}");
    assert_eq!(lines[1], "console.log: 10/09 14:03:22 [Client] one", "the line's own timestamp is kept, the CR isn't");

    // A line still being written waits for its line break.
    append(&path, " line\n\n   \nthird\n");
    tail.poll(&mut out);
    assert_eq!(out.take(), ["console.log: 10/09 14:03:23 half line", "console.log: third"], "blank lines are skipped");

    tail.poll(&mut out);
    assert!(out.take().is_empty(), "nothing new");

    // The game closes in the middle of a line.
    append(&path, "last words");
    tail.poll(&mut out);
    assert!(out.take().is_empty());
    tail.flush(&mut out);
    assert_eq!(out.take(), ["console.log: last words"]);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn lines_from_before_watching_are_marked() {
    let dir = temp("backlog");
    let path = dir.join("console.log");
    append(&path, "old one\nold two\n");
    let mut out = Mem::default();
    let mut tail = Tail::new(path.clone(), false, false);
    tail.poll(&mut out);
    let lines = out.take();
    assert!(lines[0].contains("16 bytes were already there") && lines[0].contains("marked *"), "{lines:?}");
    assert_eq!(lines[1..], ["console.log*: old one", "console.log*: old two"]);
    append(&path, "new\n");
    tail.poll(&mut out);
    assert_eq!(out.take(), ["console.log: new"]);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn steam_logs_are_followed_from_their_end() {
    let dir = temp("skip");
    let path = dir.join("stats_log.txt");
    append(&path, "[2026-10-09 08:00:02] yesterday\n");
    let mut out = Mem::default();
    let mut tail = Tail::new(path.clone(), true, false);
    tail.poll(&mut out);
    let lines = out.take();
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("from its end"), "{lines:?}");
    append(&path, "[2026-10-09 14:03:22] now\n");
    tail.poll(&mut out);
    assert_eq!(out.take(), ["stats_log.txt: [2026-10-09 14:03:22] now"]);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn a_log_started_over_is_read_from_its_start() {
    let dir = temp("truncate");
    let path = dir.join("console.log");
    append(&path, "first run, a long line\n");
    let mut out = Mem::default();
    let mut tail = Tail::new(path.clone(), false, true);
    tail.poll(&mut out);
    assert_eq!(out.take().len(), 2);

    // The game starts again and writes the file anew.
    std::fs::write(&path, "second run\n").unwrap();
    tail.poll(&mut out);
    let lines = out.take();
    assert!(lines[0].contains("got shorter: 23 -> 11 bytes"), "{lines:?}");
    assert_eq!(lines[1], "console.log: second run", "not marked: it was written while watching");

    std::fs::remove_file(&path).unwrap();
    tail.poll(&mut out);
    assert_eq!(out.take(), ["console.log: (the file is gone)"]);
    append(&path, "third run\n");
    tail.poll(&mut out);
    let lines = out.take();
    assert!(lines[0].contains("appeared") && lines[1] == "console.log: third run", "{lines:?}");
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn folder_changes_are_logged_by_name_size_and_time_only() {
    let dir = temp("dir");
    let replays = dir.join("replays");
    let mut out = Mem::default();
    let mut watch = DirWatch::new(replays.clone(), "replays", 1, true);
    assert!(watch.poll(&mut out).is_empty());
    assert!(watch.poll(&mut out).is_empty());
    let lines = out.take();
    assert_eq!(lines.len(), 1, "a missing folder is said once: {lines:?}");
    assert!(lines[0].starts_with("replays: (no such folder: "));

    // "Download Replay" creates the folder and a growing .dem.
    std::fs::create_dir_all(replays.join("sub").join("deeper")).unwrap();
    append(&replays.join("12345.dem"), "PBDEMS2");
    append(&replays.join("sub").join("note.txt"), "x");
    append(&replays.join("sub").join("deeper").join("hidden.txt"), "beyond the depth");
    let touched = watch.poll(&mut out);
    assert_eq!(touched, [replays.join("12345.dem"), replays.join("sub").join("note.txt")]);
    let lines = out.take();
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert!(lines[0].contains("the folder appeared"));
    assert!(lines[1].starts_with("replays: added 12345.dem (7 bytes, written 20"), "{lines:?}");
    assert!(!lines.iter().any(|l| l.contains("PBDEMS2") || l.contains("hidden")), "never the content, never deeper than asked");

    append(&replays.join("12345.dem"), "more");
    std::fs::remove_file(replays.join("sub").join("note.txt")).unwrap();
    let touched = watch.poll(&mut out);
    assert_eq!(touched, [replays.join("12345.dem")]);
    let lines = out.take();
    assert!(lines[0].starts_with("replays: changed 12345.dem (11 bytes, written 20") && lines[0].ends_with("was 7 bytes)"), "{lines:?}");
    assert!(lines[1].starts_with("replays: removed sub") && lines[1].ends_with("note.txt"), "{lines:?}");
    assert!(watch.poll(&mut out).is_empty() && out.take().is_empty(), "nothing changed: nothing logged");
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn files_already_there_are_counted_not_reported_as_new() {
    let dir = temp("existing");
    append(&dir.join("timeline_1.json"), "{}");
    let mut out = Mem::default();
    let mut listed = DirWatch::new(dir.clone(), "steam-timeline", 0, true);
    assert!(listed.poll(&mut out).is_empty());
    let lines = out.take();
    assert!(lines[0].contains("1 files already there; newest: timeline_1.json ("), "{lines:?}");
    assert!(lines[1].starts_with("steam-timeline: existing timeline_1.json (2 bytes"), "{lines:?}");
    let mut quiet = DirWatch::new(dir.clone(), "steam-httpcache", 0, false);
    quiet.poll(&mut out);
    assert_eq!(out.take().len(), 1, "big folders: only the count");
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn small_text_files_are_copied_in_pieces() {
    let dir = temp("dump");
    let path = dir.join("timeline_1422450.json");
    let long = "x".repeat(DUMP_CHUNK + 10);
    std::fs::write(&path, format!("{{\n  \"entries\": [\n    {{ \"title\": \"{long}\" }}\n  ]\n}}\n")).unwrap();
    let mut out = Mem::default();
    assert!(wants_dump(&path) && !wants_dump(&dir.join("12345.dem")) && !wants_dump(&dir.join("gamerecording.pb")));
    dump(&path, "steam-timeline", &mut out);
    let lines = out.take();
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(lines[0].starts_with("steam-timeline: content of timeline_1422450.json [1/2]: { \"entries\": [ { \"title\": \"xxx"), "{lines:?}");
    assert!(lines[1].starts_with("steam-timeline: content of timeline_1422450.json [2/2]: x") && lines[1].ends_with("\" } ] }"), "{lines:?}");
    std::fs::write(&path, "").unwrap();
    dump(&path, "steam-timeline", &mut out);
    assert_eq!(out.take(), ["steam-timeline: content of timeline_1422450.json: (empty)"]);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn log_rows_have_time_elapsed_source_and_one_line_of_text() {
    let dir = temp("logfile");
    let mut log = LogFile::create(&dir).unwrap();
    log.line("console.log", "tab\there\r\nnext");
    log.line("window", &"y".repeat(MAX_LINE_CHARS + 5));
    let path = log.path.clone();
    drop(log);
    assert_eq!(latest_log(&dir), Some(path.clone()));
    let text = std::fs::read_to_string(&path).unwrap();
    let rows: Vec<Vec<&str>> = text.lines().map(|l| l.split('\t').collect()).collect();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].len(), 4, "tabs and line breaks inside the text are replaced: {:?}", rows[0]);
    assert_eq!(rows[0][0].len(), "2026-10-09 14:03:22.417".len());
    assert!(rows[0][1].starts_with("+0."), "{:?}", rows[0]);
    assert_eq!(rows[0][2..], ["console.log", "tab here  next"]);
    assert!(rows[1][3].ends_with("y (cut)") && rows[1][3].len() == MAX_LINE_CHARS + " (cut)".len());
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn only_the_newest_logs_are_kept() {
    let dir = temp("prune");
    for i in 0..KEEP_LOGS + 3 {
        append(&dir.join(format!("{LOG_PREFIX}19990101-0000{i:02}.log")), "old");
    }
    append(&dir.join("clairvoyance.log"), "not ours");
    let session = Session::start(&dir).unwrap();
    let ours = logs_in(&dir);
    assert_eq!(ours.len(), KEEP_LOGS);
    assert_eq!(ours.last().map(PathBuf::as_path), Some(session.path()), "the new log is the newest");
    assert!(dir.join("clairvoyance.log").exists());
    session.finish("test");
    let text = std::fs::read_to_string(latest_log(&dir).unwrap()).unwrap();
    assert!(text.contains("Deadlock match signals") && text.contains("end of the log: test."), "{text}");
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn only_steams_own_logs_are_followed() {
    let dir = temp("steamlogs");
    for name in ["stats_log.txt", "content_log.txt", "gameoverlay_ui.previous.txt", "webhelper.txt", "cef_log.txt", "connection_log_27015.txt", "bootstrap_log.bak"] {
        append(&dir.join(name), "x\n");
    }
    assert_eq!(steam_log_files(&dir), [dir.join("content_log.txt"), dir.join("stats_log.txt")]);
    std::fs::remove_dir_all(dir).ok();
}
