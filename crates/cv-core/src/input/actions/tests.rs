use super::*;
use crate::input::stats::Analysis;
use crate::input::{Rect, Record, WindowInfo, UNIT};

const RATE: u32 = 250;
const VK_Q: u8 = b'Q';
const VK_W: u8 = b'W';
const VK_CTRL: u8 = 0x11;
const VK_SHIFT: u8 = 0x10;
const VK_ALT: u8 = 0x12;

fn us(s: f64) -> i64 {
    (s * 1e6).round() as i64
}
fn u(v: f64) -> i32 {
    (v * UNIT).round() as i32
}
fn cur(t: f64, x: f64, y: f64) -> Record {
    Record::Cursor { t: us(t), x: u(x), y: u(y) }
}
fn key(t: f64, vk: u8, down: bool) -> Record {
    Record::Key { t: us(t), vk, down }
}
fn start() -> Vec<Record> {
    let full = Rect { x: 0, y: 0, w: 1920, h: 1080 };
    vec![Record::Window { t: 0, info: WindowInfo { client: full, frame: full, dpi: 96 } }, Record::Focus { t: 0, focused: true }]
}

fn act(id: &str, label: &str, binds: Vec<ActionBind>) -> ActionKey {
    ActionKey { id: id.into(), label: label.into(), icon: None, category: "ability".into(), size: 1.0, color: "#fff".into(), default_key: label.into(), binds }
}
fn qw() -> Vec<ActionKey> {
    vec![
        act("spell1", "Q", vec![ActionBind::key(VK_Q), ActionBind::key(VK_Q).with(false, true, false), ActionBind::key(VK_Q).with(false, false, true)]),
        act("spell2", "W", vec![ActionBind::key(VK_W)]),
    ]
}

/// A cursor moving right at 250 Hz from x = 0.1 (0.4 per second).
fn moving(t0: f64, t1: f64) -> Vec<Record> {
    let mut r = Vec::new();
    let mut t = t0;
    while t <= t1 + 1e-9 {
        r.push(cur(t, 0.1 + 0.4 * t, 0.5));
        t += 1.0 / RATE as f64;
    }
    r
}

#[test]
fn interpolated_between_the_two_surrounding_samples() {
    let mut r = start();
    r.extend(moving(0.0, 2.0));
    let an = Analysis::from_records(&r);
    // 1.0013 s: between the samples at 1.000 and 1.004, 1/3 of the way.
    let (x, y) = cursor_at(&an.moves, 1.0013, RATE).unwrap();
    assert!((x as f64 - (0.1 + 0.4 * 1.0013)).abs() < 2e-5, "x {x}");
    assert!((y - 0.5).abs() < 1e-6);
    // Exactly on a sample.
    let (x, _) = cursor_at(&an.moves, 1.5, RATE).unwrap();
    assert!((x as f64 - 0.7).abs() < 2e-5);
    // Not the nearest sample: halfway is halfway.
    let (x, _) = cursor_at(&an.moves, 1.002, RATE).unwrap();
    assert!((x as f64 - (0.1 + 0.4 * 1.002)).abs() < 2e-5, "x {x}");
    // Before the first sample: no position.
    assert!(cursor_at(&an.moves, -0.1, RATE).is_none());
}

fn near(p: Option<(f32, f32)>, x: f32, y: f32) -> bool {
    p.is_some_and(|(a, b)| (a - x).abs() < 1e-4 && (b - y).abs() < 1e-4)
}

#[test]
fn rest_then_move_and_stroke_breaks() {
    // At rest at 0.2 from 1.0 s, next sample (moved) at 3.0 s: the cursor was still at 0.2 until
    // one sample period before 3.0.
    let mut r = start();
    r.push(cur(1.0, 0.2, 0.2));
    r.push(cur(3.0, 0.6, 0.2));
    let an = Analysis::from_records(&r);
    assert!(near(cursor_at(&an.moves, 2.0, RATE), 0.2, 0.2));
    assert!(near(cursor_at(&an.moves, 2.996, RATE), 0.2, 0.2));
    let (x, _) = cursor_at(&an.moves, 2.998, RATE).unwrap();
    assert!((x - 0.4).abs() < 1e-4, "halfway through the last period: {x}");
    // A new stroke (focus came back) after t: never interpolated across it.
    let mut r = start();
    r.push(cur(1.0, 0.2, 0.2));
    r.push(Record::Focus { t: us(1.5), focused: false });
    r.push(Record::Focus { t: us(2.0), focused: true });
    r.push(cur(2.001, 0.9, 0.9));
    let an = Analysis::from_records(&r);
    assert!(near(cursor_at(&an.moves, 2.0005, RATE), 0.2, 0.2), "not interpolated across a stroke break");
}

#[test]
fn key_down_time_to_video_frame() {
    // 60 fps frame times with a late frame (variable frame rate).
    let frames: Vec<f64> = (0..10).map(|i| i as f64 / 60.0).chain([0.2, 0.25]).collect();
    assert_eq!(frame_of(&frames, 0.0), 0.0);
    assert_eq!(frame_of(&frames, 0.01), 0.0, "inside frame 0");
    assert_eq!(frame_of(&frames, 1.0 / 60.0), 1.0 / 60.0, "exactly on a frame: that frame");
    assert_eq!(frame_of(&frames, 2.0 / 60.0 - 1e-6), 1.0 / 60.0, "just before the next frame");
    assert_eq!(frame_of(&frames, 0.21), 0.2);
    assert_eq!(frame_of(&frames, 9.0), 0.25, "after the last frame");
    assert_eq!(frame_of(&[], 1.234), 1.234, "unknown frames: the time itself");
    // Presses carry both times.
    let mut r = start();
    r.extend(moving(0.0, 1.0));
    r.push(key(0.1234, VK_Q, true));
    let an = Analysis::from_records(&r);
    let p = presses(&an, &qw(), RATE, &frames);
    assert_eq!(p.len(), 1);
    assert!((p[0].t - 0.1234).abs() < 1e-9);
    assert_eq!(p[0].show, 7.0 / 60.0);
    assert!(p[0].show <= p[0].t && p[0].t < p[0].show + 1.0 / 60.0, "shown on the frame containing the key-down");
}

#[test]
fn modifiers_and_level_up_combos() {
    let mut r = start();
    r.extend(moving(0.0, 10.0));
    // Q, Ctrl+Q (level up: nothing), Shift+Q (quick cast), Alt+Q (self cast), Ctrl+Shift+Q
    // (no such bind), W, Ctrl+W.
    r.extend([key(1.0, VK_Q, true), key(1.05, VK_Q, false)]);
    r.extend([key(2.0, VK_CTRL, true), key(2.1, VK_Q, true), key(2.15, VK_Q, false), key(2.2, VK_CTRL, false)]);
    r.extend([key(3.0, VK_SHIFT, true), key(3.1, VK_Q, true), key(3.15, VK_Q, false), key(3.2, VK_SHIFT, false)]);
    r.extend([key(4.0, VK_ALT, true), key(4.1, VK_Q, true), key(4.15, VK_Q, false), key(4.2, VK_ALT, false)]);
    r.extend([key(5.0, VK_CTRL, true), key(5.01, VK_SHIFT, true), key(5.1, VK_Q, true), key(5.15, VK_Q, false), key(5.2, VK_SHIFT, false), key(5.21, VK_CTRL, false)]);
    r.extend([key(6.0, VK_W, true), key(6.05, VK_W, false)]);
    r.extend([key(7.0, VK_CTRL, true), key(7.1, VK_W, true), key(7.2, VK_CTRL, false)]);
    let an = Analysis::from_records(&r);
    let p = presses(&an, &qw(), RATE, &[]);
    let got: Vec<(f64, usize)> = p.iter().map(|p| ((p.t * 10.0).round() / 10.0, p.action)).collect();
    assert_eq!(got, vec![(1.0, 0), (3.1, 0), (4.1, 0), (6.0, 1)]);
    assert!(p.iter().all(|p| p.hint.is_none()));
}

#[test]
fn held_modifiers_are_forgotten_when_focus_changes() {
    // Ctrl went down, focus was lost (its release not recorded), focus came back: a plain Q
    // afterwards is a cast, not a level-up.
    let mut r = start();
    r.extend(moving(0.0, 5.0));
    r.push(key(1.0, VK_CTRL, true));
    r.push(Record::Focus { t: us(1.5), focused: false });
    r.push(Record::Focus { t: us(2.5), focused: true });
    r.push(key(3.0, VK_Q, true));
    // A press while unfocused is never recorded, but if one were, it gets no bubble.
    r.push(key(2.0, VK_W, true));
    let an = Analysis::from_records(&r);
    let p = presses(&an, &qw(), RATE, &[]);
    assert_eq!(p.iter().map(|p| p.action).collect::<Vec<_>>(), vec![0]);
    assert!((p[0].t - 3.0).abs() < 1e-9);
}

#[test]
fn rebound_keys_mouse_binds_and_hints() {
    let actions = vec![
        // Q rebound to A, plus mouse button 4.
        act("spell1", "Q", vec![ActionBind::key(b'A'), ActionBind::mouse(4)]),
        // Item slot 4 on its default key 5, and rebound to 4 (its label): no hint either way.
        ActionKey { default_key: "5".into(), ..act("item4", "4", vec![ActionBind::key(b'5'), ActionBind::key(b'4')]) },
    ];
    let mut r = start();
    r.extend(moving(0.0, 5.0));
    r.push(key(1.0, b'A', true));
    r.push(key(1.5, VK_Q, true)); // Q is unbound now: nothing
    r.push(Record::Button { t: us(2.0), button: 4, down: true });
    r.push(Record::Button { t: us(2.5), button: 1, down: true }); // left click: nothing
    r.push(key(3.0, b'5', true));
    r.push(key(3.5, b'4', true));
    let an = Analysis::from_records(&r);
    let p = presses(&an, &actions, RATE, &[]);
    let got: Vec<(usize, Option<&str>, u8)> = p.iter().map(|p| (p.action, p.hint.as_deref(), p.button)).collect();
    assert_eq!(got, vec![(0, Some("A"), 0), (0, Some("M4"), 4), (1, None, 0), (1, None, 0)]);
    // The click's position is the cursor's at that time.
    assert!((p[1].x as f64 - (0.1 + 0.4 * 2.0)).abs() < 2e-5);
}

#[test]
fn saved_binds_or_the_games_defaults() {
    let saved = vec![act("spell1", "Q", vec![ActionBind::key(b'A')])];
    let (a, was_saved) = session_actions(Some(&saved), qw);
    assert!(was_saved);
    assert_eq!(a, saved);
    let (a, was_saved) = session_actions(None, qw);
    assert!(!was_saved);
    assert_eq!(a, qw());
    let (_, was_saved) = session_actions(Some(&[]), qw);
    assert!(!was_saved, "an empty list is no saved binds");
}

#[test]
fn key_names_and_arrays() {
    assert_eq!(vk_from_name("Q"), Some(b'Q'));
    assert_eq!(vk_from_name("q"), Some(b'Q'));
    assert_eq!(vk_from_name("5"), Some(b'5'));
    assert_eq!(vk_from_name("F3"), Some(0x72));
    assert_eq!(vk_from_name("Num4"), Some(0x64));
    assert_eq!(vk_from_name("Key189"), Some(189));
    assert_eq!(vk_from_name("Space"), Some(0x20));
    assert_eq!(vk_from_name("`"), Some(0xC0));
    assert_eq!(vk_from_name("Nope"), None);
    assert_eq!(short_key_label(189), "-");
    assert_eq!(short_key_label(0x20), "Spc");
    let p = vec![
        ActionPress { t: 1.0, show: 0.99, x: 0.1, y: 0.2, action: 0, hint: Some("A".into()), button: 0, state: PressState::Normal },
        ActionPress { t: 2.0, show: 1.99, x: 0.3, y: 0.4, action: 1, hint: None, button: 0, state: PressState::Unconfirmed },
        ActionPress { t: 3.0, show: 2.99, x: 0.5, y: 0.6, action: 0, hint: Some("A".into()), button: 0, state: PressState::Confirmed },
    ];
    let a = to_arrays(&p);
    assert_eq!(a.hint, vec![1, 0, 1]);
    assert_eq!(a.hints, vec!["A".to_string()]);
    assert_eq!(a.state, vec![0, 2, 1]);
}
