//! Timing of the input-recording code paths on a worst-case 33 min game (developer tool):
//! `cargo run --release -p cv-core --example inputbench [secs]`
use cv_core::input::{self, stats, Record, Rect, WindowInfo};
use std::time::Instant;

fn main() {
    let dur: f64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(1970.0);
    let mut seed = 0x2545F4914F6CDD1Du64;
    let mut rnd = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed >> 11) as f64 / (1u64 << 53) as f64
    };
    let full = Rect { x: 0, y: 0, w: 3840, h: 2160 };
    let mut r = vec![Record::Window { t: 0, info: WindowInfo { client: full, frame: full, dpi: 144 } }, Record::Focus { t: 0, focused: true }];
    let (mut x, mut y, mut vx, mut vy) = (0.5f64, 0.5f64, 0.0f64, 0.0f64);
    for i in 0..(dur * 250.0) as i64 {
        let t = i * 4000;
        if rnd() < 0.01 {
            vx = (rnd() - 0.5) * 0.04;
            vy = (rnd() - 0.5) * 0.04;
        }
        vx *= 0.97;
        vy *= 0.97;
        x = (x + vx + (rnd() - 0.5) * 0.002).clamp(0.02, 0.98);
        y = (y + vy + (rnd() - 0.5) * 0.002).clamp(0.02, 0.98);
        r.push(Record::Cursor { t, x: (x * input::UNIT) as i32, y: (y * input::UNIT) as i32 });
        if rnd() < 0.01 {
            r.push(Record::Button { t: t + 100, button: 2, down: true });
            r.push(Record::Button { t: t + 60_000, button: 2, down: false });
        }
        if rnd() < 0.006 {
            r.push(Record::Key { t: t + 300, vk: b'Q', down: true });
            r.push(Record::Key { t: t + 90_000, vk: b'Q', down: false });
        }
    }
    let p = std::env::temp_dir().join("inputbench.input");
    input::write_file(&p, 250, &input::Meta::default(), &r).unwrap();
    let f = input::read(&p).unwrap();
    let an = stats::Analysis::from_file(&f);
    let h = stats::heatmap(&an, 0.0, an.end, stats::HEAT_W, stats::HEAT_H);
    input::compress_in_place(&p, Some(&h)).unwrap();
    for _ in 0..3 {
        let t0 = Instant::now();
        let b = std::fs::read(&p).unwrap();
        let t1 = Instant::now();
        let f = input::parse(&b).unwrap();
        let t2 = Instant::now();
        let an = stats::Analysis::from_file(&f);
        let t3 = Instant::now();
        let pl = stats::ui_payload(&an, f.heatmap.as_ref(), f.rate);
        let t4 = Instant::now();
        let ms = |a: Instant, b: Instant| (b - a).as_secs_f64() * 1000.0;
        println!(
            "{} records: read {:.1} ms, parse {:.1} ms, analysis {:.1} ms, payload {:.1} ms ({:.1} MB) = {:.1} ms",
            f.records.len(),
            ms(t0, t1),
            ms(t1, t2),
            ms(t2, t3),
            ms(t3, t4),
            pl.len() as f64 / 1e6,
            ms(t0, t4)
        );
    }
}
