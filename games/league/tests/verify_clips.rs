//! Layer 2 on real recordings: 15 s clips cut from the owner's games (not in the repo; set
//! CV_SAMPLES to their folder). Run:
//! `CV_SAMPLES=... cargo test -p cv-game-league --test verify_clips -- --ignored --nocapture`

use cv_core::game::{FrameSource, KeyMark, PlayerInfo, Region, Rgb};
use cv_core::session::GameSession;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Decodes with the ffmpeg CLI, sequentially from the start (exact timestamps): keyframes only
/// for big regions, every frame for small ones (the R icon), cached per region.
struct Ffmpeg {
    path: PathBuf,
    size: (u32, u32),
    duration: f64,
    keyframes: Vec<f64>,
    cache: HashMap<(u32, u32, u32, u32), Vec<(f64, Rgb)>>,
}

fn probe(path: &Path, args: &[&str]) -> String {
    let o = Command::new("ffprobe").args(["-v", "error"]).args(args).arg(path).output().expect("ffprobe");
    String::from_utf8_lossy(&o.stdout).into_owned()
}

impl Ffmpeg {
    fn open(path: &Path) -> Ffmpeg {
        let wh = probe(path, &["-select_streams", "v", "-show_entries", "stream=width,height", "-of", "csv=p=0"]);
        let mut it = wh.trim().split(',').map(|x| x.parse::<u32>().unwrap());
        let size = (it.next().unwrap(), it.next().unwrap());
        let duration = probe(path, &["-show_entries", "format=duration", "-of", "csv=p=0"]).trim().parse().unwrap();
        let keyframes = probe(path, &["-skip_frame", "nokey", "-select_streams", "v", "-show_entries", "frame=pts_time", "-of", "csv=p=0"])
            .lines()
            .filter_map(|l| l.trim().trim_end_matches(',').parse().ok())
            .collect();
        Ffmpeg { path: path.into(), size, duration, keyframes, cache: HashMap::new() }
    }

    fn decoded(&mut self, r: Region) -> &Vec<(f64, Rgb)> {
        let key_only = r.w * r.h > 120 * 120;
        let path = self.path.clone();
        let size = self.size;
        self.cache.entry((r.x, r.y, r.w, r.h)).or_insert_with(|| {
            let mut c = Command::new("ffmpeg");
            c.args(["-v", "info"]);
            if key_only {
                c.args(["-skip_frame", "nokey"]);
            }
            c.arg("-i").arg(&path);
            // Same NV12 → RGB conversion as the Windows hardware decoder path.
            let b = cv_capture::nv12::aligned_box(r, size.0, size.1);
            c.args(["-an", "-fps_mode", "passthrough", "-vf", &format!("crop={}:{}:{}:{},showinfo", b.w, b.h, b.x, b.y), "-f", "rawvideo", "-pix_fmt", "nv12", "-"]);
            let o = c.output().expect("ffmpeg");
            let err = String::from_utf8_lossy(&o.stderr);
            let times: Vec<f64> = err
                .lines()
                .filter_map(|l| l.split("pts_time:").nth(1))
                .filter_map(|x| x.split_whitespace().next()?.parse().ok())
                .collect();
            let n = (b.w * b.h * 3 / 2) as usize;
            assert_eq!(o.stdout.len() / n, times.len());
            let ylen = (b.w * b.h) as usize;
            o.stdout
                .chunks_exact(n)
                .zip(times)
                .map(|(d, t)| (t, cv_capture::nv12::to_rgb(&d[..ylen], b.w as usize, &d[ylen..], b.w as usize, b, r)))
                .collect()
        })
    }
}

impl FrameSource for Ffmpeg {
    fn size(&self) -> (u32, u32) {
        self.size
    }
    fn duration(&self) -> f64 {
        self.duration
    }
    fn keyframes(&self) -> &[f64] {
        &self.keyframes
    }
    fn frame_at(&mut self, t: f64, region: Region) -> anyhow::Result<Option<(f64, Rgb)>> {
        Ok(self.decoded(region).iter().find(|(pt, _)| *pt >= t - 1e-3).cloned())
    }
    fn frames(&mut self, t0: f64, t1: f64, region: Region, f: &mut dyn FnMut(f64, &Rgb) -> bool) -> anyhow::Result<()> {
        for (t, img) in self.decoded(region).iter().filter(|(pt, _)| *pt >= t0 - 1e-3 && *pt <= t1) {
            if !f(*t, img) {
                break;
            }
        }
        Ok(())
    }
}

/// (clip, cut start in the full video, champion, ult presses in full-video seconds).
const CLIPS: &[(&str, f64, &str, &[f64])] = &[
    ("yunara-300", 298.059, "Yunara", &[]),
    ("yunara-826", 824.428, "Yunara", &[829.8]),
    ("yunara-980", 978.987, "Yunara", &[983.9]),
    ("yunara-1449", 1448.122, "Yunara", &[1454.4]),
    ("caitlyn-200", 199.678, "Caitlyn", &[]),
    ("caitlyn-544", 543.249, "Caitlyn", &[547.7]),
    ("caitlyn-676", 675.436, "Caitlyn", &[679.8]),
    ("caitlyn-995", 994.471, "Caitlyn", &[998.0]),
    ("twitch-200", 198.898, "Twitch", &[]),
    ("twitch-425", 423.566, "Twitch", &[428.9]),
    ("twitch-464", 463.567, "Twitch", &[468.1]),
    ("twitch-1016", 1015.360, "Twitch", &[1019.9]),
];

#[test]
#[ignore]
fn ult_casts_in_real_clips() {
    let dir = PathBuf::from(std::env::var("CV_SAMPLES").unwrap_or("/mnt/user-data/uploads/GameRecorder/.claude-tmp/samples".into()));
    if !dir.join("caitlyn-544.mp4").exists() {
        eprintln!("no sample clips in {}", dir.display());
        return;
    }
    let rules = cv_game_league::ult::UltRules::builtin();
    let (mut tp, mut fp, mut fneg, mut errs) = (0, 0, 0, Vec::new());
    for (name, cut, champ, presses) in CLIPS {
        let mut v = Ffmpeg::open(&dir.join(format!("{name}.mp4")));
        let mut s = GameSession::new(name.to_string(), "league", "League of Legends", chrono::Local::now());
        s.player = Some(PlayerInfo { character_id: Some(champ.to_string()), ..Default::default() });
        s.video_offset = 0.0;
        // Decoys: presses with no cast (5 s after a cast = on cooldown; 5 s into the pre-level-6
        // clips = not learned). They must end up "pressed, no cast".
        let decoys: Vec<f64> = if presses.is_empty() { vec![cut + 5.0] } else { presses.iter().map(|p| p + 5.0).collect() };
        s.key_presses = presses
            .iter()
            .chain(&decoys)
            .map(|p| KeyMark { game_time: p - cut, action: "ult".into(), key: "R".into(), accepted: true, reason: None })
            .collect();
        let t = std::time::Instant::now();
        cv_game_league::verify::verify(&mut s, &mut v, &rules, &|| false).unwrap();
        let ver = s.verification.as_ref().unwrap();
        let used: Vec<f64> = s.events.iter().filter(|e| e.kind == cv_core::EventKind::UltUsed).map(|e| e.game_time + cut).collect();
        println!(
            "{name}: {} conf {:.2}, casts at {:?}, presses {:?} ({} ms)",
            ver.status,
            ver.confidence,
            used.iter().map(|x| format!("{x:.3}")).collect::<Vec<_>>(),
            presses,
            t.elapsed().as_millis()
        );
        assert_eq!(ver.status, "verified", "{name}");
        let no_cast: Vec<f64> = s.events.iter().filter(|e| e.kind == cv_core::EventKind::UltUnconfirmed).map(|e| e.game_time + cut).collect();
        assert_eq!(no_cast.len(), decoys.len(), "{name}: {no_cast:?}");
        assert_eq!(ver.confirmed, presses.len(), "{name}");
        for p in presses.iter() {
            match used.iter().find(|c| **c >= p - 0.1 && **c <= p + 1.6) {
                Some(c) => {
                    tp += 1;
                    errs.push(c - p);
                }
                None => fneg += 1,
            }
        }
        fp += used.iter().filter(|c| !presses.iter().any(|p| **c >= p - 0.1 && **c <= p + 1.6)).count();
    }
    println!("casts: {tp} found, {fneg} missed, {fp} extra; press→cast delays {:?}", errs.iter().map(|x| format!("{x:.2}")).collect::<Vec<_>>());
    assert_eq!(fneg, 0);
    assert_eq!(fp, 0);
}

/// Windows where the first version saw casts that weren't (the icon dimmed by a stun / out of
/// mana, then the cooldown showing again), plus a press while dead. Clips from samples2.
/// (clip, champion, expected casts, presses in clip seconds that must stay without a cast)
const TRICKY: &[(&str, &str, usize, &[f64])] = &[
    ("yunara-1011", "Yunara", 0, &[]),
    ("yunara-1106", "Yunara", 0, &[]),
    ("caitlyn-701", "Caitlyn", 0, &[]),
    ("caitlyn-858", "Caitlyn", 1, &[]),
    ("caitlyn-898", "Caitlyn", 0, &[]),
    ("caitlyn-1025", "Caitlyn", 0, &[]),
    ("twitch-987", "Twitch", 0, &[5.0]),
];

#[test]
#[ignore]
fn no_casts_from_stuns_or_death() {
    let dir = PathBuf::from(std::env::var("CV_SAMPLES2").unwrap_or("/mnt/user-data/uploads/GameRecorder/.claude-tmp/samples2".into()));
    if !dir.join("caitlyn-858.mp4").exists() {
        eprintln!("no sample clips in {}", dir.display());
        return;
    }
    let rules = cv_game_league::ult::UltRules::builtin();
    for (name, champ, want, presses) in TRICKY {
        let mut v = Ffmpeg::open(&dir.join(format!("{name}.mp4")));
        let mut s = GameSession::new(name.to_string(), "league", "League of Legends", chrono::Local::now());
        s.player = Some(PlayerInfo { character_id: Some(champ.to_string()), ..Default::default() });
        s.key_presses = presses.iter().map(|p| KeyMark { game_time: *p, action: "ult".into(), key: "R".into(), accepted: true, reason: None }).collect();
        cv_game_league::verify::verify(&mut s, &mut v, &rules, &|| false).unwrap();
        let ver = s.verification.as_ref().unwrap();
        println!("{name}: {} casts, {} presses without a cast", ver.casts, ver.unconfirmed);
        assert_eq!(ver.casts, *want, "{name}");
        assert_eq!(ver.unconfirmed, presses.len(), "{name}");
    }
}

/// Any recording + its session.json: `CV_VIDEO=... CV_SESSION=... [CV_OFFSET=-30.1]`. Prints the
/// casts and outcomes (for the owner's scripted tests).
#[test]
#[ignore]
fn one_recording() {
    let (Ok(video), Ok(session)) = (std::env::var("CV_VIDEO"), std::env::var("CV_SESSION")) else {
        eprintln!("set CV_VIDEO and CV_SESSION");
        return;
    };
    let text = std::fs::read_to_string(&session).unwrap();
    let mut s: GameSession = serde_json::from_str(text.trim_start_matches('\u{feff}')).unwrap();
    if let Ok(o) = std::env::var("CV_OFFSET") {
        s.video_offset = o.parse().unwrap();
    }
    let mut v = Ffmpeg::open(Path::new(&video));
    let t = std::time::Instant::now();
    cv_game_league::verify::verify(&mut s, &mut v, &cv_game_league::ult::UltRules::builtin(), &|| false).unwrap();
    println!("{} ms; {}", t.elapsed().as_millis(), serde_json::to_string(&s.verification).unwrap());
    for e in s.events.iter().filter(|e| e.id.starts_with("ult")) {
        println!("RESULT {:.3} {:?} {} | {}", e.game_time + s.video_offset, e.kind, e.title, e.details.clone().unwrap_or_default());
    }
}
