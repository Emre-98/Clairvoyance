use super::*;
use cv_core::game::KeyMark;
use cv_core::input::actions::{presses, ActionPress, PressState};
use cv_core::input::stats::Analysis;
use cv_core::input::{Record, Rect, WindowInfo, UNIT};
use cv_core::session::{GameSession, Verification};
use cv_core::GameEvent;

/// The owner's real input.ini (League 16.19), trimmed: R binds as in the ult tests, plus a
/// few others the way League writes them.
const OWNER_INI: &str = "[GameEvents]
evtSmartPlusSelfCastWithIndicatorSpell4=null
evtSmartPlusSelfCastSpell4=[<Unbound>]
evtSmartCastWithIndicatorSpell4=[Alt][r]
evtSmartCastSpell4=[r]
evtSelfCastSpell4=[Shift][r]
evtNormalCastSpell4=null
evtLevelSpell4=[Ctrl][r]
evtCastSpell4=[<Unbound>]
evtSmartCastSpell1=[q]
evtCastSpell1=[<Unbound>]
evtLevelSpell1=[Ctrl][q]
evtPlayerMoveClick=[Button 2],[Shift][Button 2]
";

fn labels(a: &ActionKey) -> Vec<String> {
    a.binds
        .iter()
        .map(|b| {
            let mut s = String::new();
            if b.ctrl {
                s += "Ctrl+";
            }
            if b.shift {
                s += "Shift+";
            }
            if b.alt {
                s += "Alt+";
            }
            s + &b.physical()
        })
        .collect()
}
fn get<'a>(a: &'a [ActionKey], id: &str) -> &'a ActionKey {
    a.iter().find(|x| x.id == id).unwrap()
}

#[test]
fn defaults_when_nothing_is_set() {
    let a = default_actions();
    let ids: Vec<&str> = a.iter().map(|x| x.id.as_str()).collect();
    assert_eq!(ids, ["spell1", "spell2", "spell3", "spell4", "summoner1", "summoner2", "item1", "item2", "item3", "item4", "item5", "item6", "ward"]);
    assert_eq!(labels(get(&a, "spell1")), ["Q", "Shift+Q", "Alt+Q"]);
    assert_eq!(labels(get(&a, "spell4")), ["R", "Shift+R", "Alt+R"]);
    assert_eq!(labels(get(&a, "summoner1")), ["D", "Shift+D", "Alt+D"]);
    assert_eq!(labels(get(&a, "summoner2")), ["F", "Shift+F", "Alt+F"]);
    // Item slots 1 2 3 5 6 7, trinket 4; labels are the slots.
    let items: Vec<(String, String)> = ["item1", "item2", "item3", "item4", "item5", "item6", "ward"]
        .iter()
        .map(|i| (get(&a, i).label.clone(), get(&a, i).binds[0].physical()))
        .collect();
    assert_eq!(items, [("1", "1"), ("2", "2"), ("3", "3"), ("4", "5"), ("5", "6"), ("6", "7"), ("Ward", "4")].map(|(a, b)| (a.to_string(), b.to_string())));
    assert_eq!(get(&a, "ward").icon.as_deref(), Some("ward"));
    // Categories and sizes by importance: Q W E R biggest, then summoners, the ward, items smallest.
    let size = |id| get(&a, id).size;
    assert!(["spell1", "spell2", "spell3", "spell4"].iter().all(|&s| size(s) == size("spell1")));
    assert!(size("spell1") > size("summoner1") && size("summoner1") == size("summoner2"));
    assert!(size("summoner1") > size("ward") && size("ward") > size("item1"));
    assert_eq!(categories().iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["ability", "summoner", "item", "ward"]);
    // No level-up (Ctrl) bind anywhere.
    assert!(a.iter().all(|x| x.binds.iter().all(|b| !b.ctrl)));
}

#[test]
fn owner_binds_quick_self_and_indicator_variants() {
    let a = parse_actions(OWNER_INI);
    // Same R binds as the ult tracking reads from this file.
    assert_eq!(labels(get(&a, "spell4")), ["R", "Alt+R", "Shift+R"]);
    let ult: Vec<String> = crate::ult::parse_input_ini(OWNER_INI).cast.iter().map(|b| b.label()).collect();
    assert_eq!(ult, ["R", "Alt+R", "Shift+R"]);
    // Q: quick cast only (cast unbound), self-cast default kept.
    assert_eq!(labels(get(&a, "spell1")), ["Q", "Alt+Q"]);
}

#[test]
fn rebinds_several_binds_and_mouse_buttons() {
    let ini = "evtCastSpell1=[a]\nevtSmartCastSpell1=[Shift][a]\nevtSelfCastSpell1=[<Unbound>]\n\
               evtSmartCastWithIndicatorSpell2=[Ctrl][w]\nevtNormalCastSpell3=[e],[Button 4]\n\
               evtSmartPlusSelfCastAvatarSpell1=[Alt][Shift][d]\nevtCastAvatarSpell2=[Space]\n\
               evtUseItem1=[z]\nevtSelfCastItem2=[<Unbound>]\nevtUseVisionItem=[Ctrl][Shift][-]\n";
    let a = parse_actions(ini);
    assert_eq!(labels(get(&a, "spell1")), ["A", "Shift+A"]);
    assert_eq!(get(&a, "spell1").default_key, "Q");
    // A Ctrl bind that IS a cast bind (with indicator) counts.
    assert_eq!(labels(get(&a, "spell2")), ["W", "Shift+W", "Ctrl+W", "Alt+W"]);
    assert_eq!(labels(get(&a, "spell3")), ["E", "Shift+E", "Alt+E", "M4"]);
    assert_eq!(labels(get(&a, "summoner1")), ["D", "Shift+D", "Alt+D", "Shift+Alt+D"]);
    assert_eq!(labels(get(&a, "summoner2")), ["Spc", "Shift+F", "Alt+F"]);
    assert_eq!(labels(get(&a, "item1")), ["Z", "Shift+1", "Alt+1"]);
    assert_eq!(labels(get(&a, "item2")), ["2", "Shift+2"]);
    assert_eq!(labels(get(&a, "ward")), ["Ctrl+Shift+-", "Shift+4", "Alt+4"]);
    // Arrow keys (League writes "Right Arrow").
    let a = parse_actions("evtCastSpell2=[Right Arrow],[Ctrl][Up Arrow]\n");
    assert_eq!(labels(get(&a, "spell2"))[..2], ["→", "Ctrl+↑"]);
    // PersistedSettings-style names are matched without case.
    let a = parse_actions("EVTCASTSPELL1=[x]\n");
    assert_eq!(labels(get(&a, "spell1"))[0], "X");
}

fn us(s: f64) -> i64 {
    (s * 1e6).round() as i64
}

/// A game where the cursor moves right; then these keys (vk, down) at these times.
fn analysis(keys: &[(f64, u8, bool)], buttons: &[(f64, u8)]) -> Analysis {
    let full = Rect { x: 0, y: 0, w: 1920, h: 1080 };
    let mut r = vec![Record::Window { t: 0, info: WindowInfo { client: full, frame: full, dpi: 96 } }, Record::Focus { t: 0, focused: true }];
    for i in 0..(30 * 250) {
        let t = i as f64 / 250.0;
        r.push(Record::Cursor { t: us(t), x: ((0.1 + 0.02 * t) * UNIT) as i32, y: (0.5 * UNIT) as i32 });
    }
    for &(t, vk, down) in keys {
        r.push(Record::Key { t: us(t), vk, down });
    }
    for &(t, b) in buttons {
        r.push(Record::Button { t: us(t), button: b, down: true });
    }
    Analysis::from_records(&r)
}

fn tap(t: f64, vk: u8) -> [(f64, u8, bool); 2] {
    [(t, vk, true), (t + 0.04, vk, false)]
}
fn with(t: f64, m: u8, vk: u8) -> [(f64, u8, bool); 4] {
    [(t - 0.05, m, true), (t, vk, true), (t + 0.04, vk, false), (t + 0.06, m, false)]
}

#[test]
fn presses_with_league_binds() {
    // Q rebound to A; owner's R binds; defaults for the rest.
    let ini = format!("{OWNER_INI}evtSmartCastSpell1=[a]\n");
    let acts = parse_actions(&ini);
    let mut keys: Vec<(f64, u8, bool)> = Vec::new();
    keys.extend(tap(1.0, b'A')); // Q (rebound)
    keys.extend(tap(1.5, b'Q')); // Q key now unbound: nothing
    keys.extend(with(2.0, 0x11, b'A')); // Ctrl+A: nothing (not a cast bind)
    keys.extend(with(3.0, 0x11, b'Q')); // Ctrl+Q level up: nothing
    keys.extend(with(4.0, 0x11, b'R')); // Ctrl+R level up: nothing
    keys.extend(tap(5.0, b'R')); // R
    keys.extend(with(6.0, 0x12, b'R')); // Alt+R (with indicator)
    keys.extend(with(7.0, 0x10, b'R')); // Shift+R (self cast)
    keys.extend(tap(8.0, b'D'));
    keys.extend(tap(9.0, b'F'));
    keys.extend(tap(10.0, b'1'));
    keys.extend(tap(11.0, b'5')); // item slot 4
    keys.extend(tap(12.0, b'7')); // item slot 6
    keys.extend(tap(13.0, b'4')); // ward
    keys.extend(with(14.0, 0x10, b'4')); // Shift+4: quick-cast ward
    keys.extend(with(15.0, 0x11, b'1')); // Ctrl+1: nothing
    keys.extend(tap(16.0, b'B')); // recall: not an action key
    let an = analysis(&keys, &[(17.0, 4)]);
    let p = presses(&an, &acts, 250, &[]);
    let got: Vec<(i64, &str, Option<&str>)> = p.iter().map(|p| (p.t.round() as i64, acts[p.action].id.as_str(), p.hint.as_deref())).collect();
    assert_eq!(
        got,
        vec![
            (1, "spell1", Some("A")),
            (5, "spell4", None),
            (6, "spell4", None),
            (7, "spell4", None),
            (8, "summoner1", None),
            (9, "summoner2", None),
            (10, "item1", None),
            (11, "item4", None),
            (12, "item6", None),
            (13, "ward", None),
            (14, "ward", None),
        ]
    );
    assert_eq!(acts[p[0].action].label, "Q", "the label is the action, not the key");
}

#[test]
fn missing_settings_files_and_persisted_settings_first() {
    let dir = std::env::temp_dir().join(format!("cv-actions-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("Config")).unwrap();
    // Nothing there: None (the module keeps League's defaults).
    assert!(read_actions(&dir).is_none());
    // input.ini only.
    std::fs::write(dir.join("Config").join("input.ini"), "[GameEvents]\nevtCastSpell2=[t]\n").unwrap();
    assert_eq!(labels(get(&read_actions(&dir).unwrap(), "spell2"))[0], "T");
    // PersistedSettings.json with binds wins.
    let json = r#"{"files":[{"name":"Input.ini","sections":[{"name":"GameEvents","settings":[{"name":"evtCastSpell2","value":"[y]"},{"name":"evtCastSpell4","value":"[r]"}]}]}]}"#;
    std::fs::write(dir.join("Config").join("PersistedSettings.json"), json).unwrap();
    assert_eq!(labels(get(&read_actions(&dir).unwrap(), "spell2"))[0], "Y");
    // ... unless it has no binds: input.ini again (same rule as the ult tracking).
    std::fs::write(dir.join("Config").join("PersistedSettings.json"), r#"{"files":[]}"#).unwrap();
    assert_eq!(labels(get(&read_actions(&dir).unwrap(), "spell2"))[0], "T");
    // Unreadable JSON: input.ini.
    std::fs::write(dir.join("Config").join("PersistedSettings.json"), "{ nope").unwrap();
    assert_eq!(labels(get(&read_actions(&dir).unwrap(), "spell2"))[0], "T");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn manual_ult_key_and_saved_binds_vs_defaults() {
    let manual = crate::ult::Bind::plain("G");
    let a = with_manual_ult(default_actions(), Some(&manual));
    assert_eq!(labels(get(&a, "spell4")), ["R", "Shift+R", "Alt+R", "G"]);
    assert_eq!(with_manual_ult(default_actions(), None), default_actions());
    // A recording with its binds saved uses them; an older one League's defaults.
    let saved = parse_actions("evtCastSpell1=[a]\n");
    let (a, was) = cv_core::input::actions::session_actions(Some(&saved), default_actions);
    assert!(was && labels(get(&a, "spell1"))[0] == "A");
    let (a, was) = cv_core::input::actions::session_actions(None, default_actions);
    assert!(!was && labels(get(&a, "spell1"))[0] == "Q");
    // The binds survive session.json.
    let mut s = GameSession::new("x".into(), "league", "League of Legends", chrono::Local::now());
    s.action_keys = Some(saved.clone());
    let back: GameSession = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
    assert_eq!(back.action_keys, Some(saved));
    // Older session.json without the field.
    let old = serde_json::to_string(&GameSession::new("y".into(), "league", "League of Legends", chrono::Local::now())).unwrap();
    assert!(!old.contains("action_keys"));
    assert!(serde_json::from_str::<GameSession>(&old).unwrap().action_keys.is_none());
}

/// The owner's real key binds (League 16.19, PersistedSettings.json's Input.ini, 2026-10-02):
/// quick cast on the plain keys, Alt = with indicator, Shift = self cast, items on 1-6, the
/// trinket on C, "Item7" (another slot in 16.x) on Right Arrow and "=".
const OWNER_PERSISTED: &str = "evtCastAvatarSpell1=[<Unbound>]
evtCastAvatarSpell2=[<Unbound>]
evtCastRoleBound=[F4]
evtCastSpell1=[<Unbound>]
evtCastSpell2=[<Unbound>]
evtCastSpell3=[<Unbound>]
evtCastSpell4=[<Unbound>]
evtLevelSpell1=[Ctrl][q]
evtLevelSpell2=[Ctrl][w]
evtLevelSpell3=[Ctrl][e]
evtLevelSpell4=[Ctrl][r]
evtNormalCastAvatarSpell1=null
evtNormalCastAvatarSpell2=null
evtNormalCastItem1=null
evtNormalCastItem2=null
evtNormalCastItem3=null
evtNormalCastItem4=null
evtNormalCastItem5=null
evtNormalCastItem6=null
evtNormalCastRoleBound=
evtNormalCastSpell1=null
evtNormalCastSpell2=null
evtNormalCastSpell3=null
evtNormalCastSpell4=null
evtNormalCastVisionItem=null
evtSelfCastAvatarSpell1=[Shift][d]
evtSelfCastAvatarSpell2=[Shift][f]
evtSelfCastItem1=[<Unbound>]
evtSelfCastItem2=[<Unbound>]
evtSelfCastItem3=[<Unbound>]
evtSelfCastItem4=[<Unbound>]
evtSelfCastItem5=[<Unbound>]
evtSelfCastItem6=[<Unbound>]
evtSelfCastRoleBound=[Alt][v],
evtSelfCastSpell1=[Shift][q]
evtSelfCastSpell2=[Shift][w]
evtSelfCastSpell3=[Shift][e]
evtSelfCastSpell4=[Shift][r]
evtSelfCastVisionItem=[<Unbound>]
evtSmartCastAvatarSpell1=[d]
evtSmartCastAvatarSpell2=[f]
evtSmartCastItem1=[1]
evtSmartCastItem2=[2]
evtSmartCastItem3=[3]
evtSmartCastItem4=[4]
evtSmartCastItem5=[5]
evtSmartCastItem6=[6]
evtSmartCastRoleBound=[Shift][v]
evtSmartCastSpell1=[q]
evtSmartCastSpell2=[w]
evtSmartCastSpell3=[e]
evtSmartCastSpell4=[r]
evtSmartCastVisionItem=[c]
evtSmartCastWithIndicatorAvatarSpell1=[Alt][d]
evtSmartCastWithIndicatorAvatarSpell2=[Alt][f]
evtSmartCastWithIndicatorItem1=[Alt][1]
evtSmartCastWithIndicatorItem2=[Alt][2]
evtSmartCastWithIndicatorItem3=[Alt][3]
evtSmartCastWithIndicatorItem4=[Alt][4]
evtSmartCastWithIndicatorItem5=[Alt][5]
evtSmartCastWithIndicatorItem6=[Alt][6]
evtSmartCastWithIndicatorRoleBound=
evtSmartCastWithIndicatorSpell1=[Alt][q]
evtSmartCastWithIndicatorSpell2=[Alt][w]
evtSmartCastWithIndicatorSpell3=[Alt][e]
evtSmartCastWithIndicatorSpell4=[Alt][r]
evtSmartCastWithIndicatorVisionItem=[Alt][c]
evtSmartPlusSelfCastAvatarSpell1=[<Unbound>]
evtSmartPlusSelfCastAvatarSpell2=[<Unbound>]
evtSmartPlusSelfCastItem1=null
evtSmartPlusSelfCastItem2=null
evtSmartPlusSelfCastItem3=null
evtSmartPlusSelfCastItem4=null
evtSmartPlusSelfCastItem5=null
evtSmartPlusSelfCastItem6=null
evtSmartPlusSelfCastRoleBound=
evtSmartPlusSelfCastSpell1=[<Unbound>]
evtSmartPlusSelfCastSpell2=[<Unbound>]
evtSmartPlusSelfCastSpell3=[<Unbound>]
evtSmartPlusSelfCastSpell4=[<Unbound>]
evtSmartPlusSelfCastVisionItem=null
evtSmartPlusSelfCastWithIndicatorAvatarSpell1=null
evtSmartPlusSelfCastWithIndicatorAvatarSpell2=null
evtSmartPlusSelfCastWithIndicatorItem1=null
evtSmartPlusSelfCastWithIndicatorItem2=null
evtSmartPlusSelfCastWithIndicatorItem3=null
evtSmartPlusSelfCastWithIndicatorItem4=null
evtSmartPlusSelfCastWithIndicatorItem5=null
evtSmartPlusSelfCastWithIndicatorItem6=null
evtSmartPlusSelfCastWithIndicatorRoleBound=
evtSmartPlusSelfCastWithIndicatorSpell1=null
evtSmartPlusSelfCastWithIndicatorSpell2=null
evtSmartPlusSelfCastWithIndicatorSpell3=null
evtSmartPlusSelfCastWithIndicatorSpell4=null
evtSmartPlusSelfCastWithIndicatorVisionItem=null
evtUseItem1=[<Unbound>]
evtUseItem2=[<Unbound>]
evtUseItem3=[<Unbound>]
evtUseItem4=[<Unbound>]
evtUseItem5=[<Unbound>]
evtUseItem6=[<Unbound>]
evtUseItem7=[Right Arrow],[=]
evtUseVisionItem=[<Unbound>]
";

#[test]
fn owner_persisted_settings_16_19() {
    let a = parse_actions(OWNER_PERSISTED);
    assert_eq!(labels(get(&a, "spell1")), ["Q", "Alt+Q", "Shift+Q"]);
    assert_eq!(labels(get(&a, "spell4")), ["R", "Alt+R", "Shift+R"]);
    assert_eq!(labels(get(&a, "summoner1")), ["D", "Alt+D", "Shift+D"]);
    for (i, k) in ["1", "2", "3", "4", "5", "6"].iter().enumerate() {
        assert_eq!(labels(get(&a, &format!("item{}", i + 1))), [k.to_string(), format!("Alt+{k}")]);
    }
    assert_eq!(labels(get(&a, "ward")), ["C", "Alt+C"], "Item7 isn't the trinket");
    assert!(a.iter().all(|x| x.binds.iter().all(|b| !b.ctrl)), "level-ups are never casts");
    // Presses: item slot 4 on 4 (its label: no hint), the trinket on C (hint), Ctrl+Q nothing.
    let mut keys: Vec<(f64, u8, bool)> = Vec::new();
    keys.extend(tap(1.0, b'4'));
    keys.extend(tap(2.0, b'C'));
    keys.extend(with(3.0, 0x11, b'Q'));
    keys.extend(with(4.0, 0x10, b'Q'));
    let p = presses(&analysis(&keys, &[]), &a, 250, &[]);
    let got: Vec<(&str, Option<&str>)> = p.iter().map(|p| (a[p.action].id.as_str(), p.hint.as_deref())).collect();
    assert_eq!(got, vec![("item4", None), ("ward", Some("C")), ("spell1", None)]);
}

// ---------- ult tie-in ----------

fn r_press(t: f64) -> ActionPress {
    ActionPress { t, show: t, x: 0.5, y: 0.5, action: 3, hint: None, button: 0, state: PressState::Normal }
}
fn mark(gt: f64, accepted: bool, reason: Option<&str>) -> KeyMark {
    KeyMark { game_time: gt, action: "ult".into(), key: "R".into(), accepted, reason: reason.map(str::to_string) }
}
fn verified() -> Verification {
    serde_json::from_value(serde_json::json!({
        "version": 2, "at": chrono::Local::now(), "status": "verified", "confidence": 0.9,
        "casts": 1, "confirmed": 1, "video_only": 0, "unconfirmed": 2, "analysis_ms": 1, "details": {}
    }))
    .unwrap()
}

#[test]
fn ult_used_is_solid_no_cast_is_faded() {
    let acts = default_actions();
    let mut s = GameSession::new("x".into(), "league", "League of Legends", chrono::Local::now());
    s.video_offset = 30.0;
    // Three presses at game 100.00 (used), 100.40 and 101.00 (no cast); the input clock is
    // 40 ms behind the logged times (clock estimate), which is corrected.
    s.key_presses = vec![mark(100.0, true, None), mark(100.4, false, Some("cooldown (80 s left)")), mark(101.0, false, Some("cooldown (79 s left)"))];
    s.events = vec![
        GameEvent::new("ult-100.2", EventKind::UltUsed, 100.2, "Ult used"),
        GameEvent::new("ultp-100.40", EventKind::UltUnconfirmed, 100.4, "Ult pressed, no cast"),
        GameEvent::new("ultp-101.00", EventKind::UltUnconfirmed, 101.0, "Ult pressed, no cast"),
    ];
    s.verification = Some(verified());
    let mut p = vec![r_press(129.96), r_press(130.36), r_press(130.96), r_press(140.0)];
    // A Q press in between is left alone.
    p.insert(1, ActionPress { action: 0, ..r_press(130.1) });
    press_states(&s, &acts, &mut p);
    let st: Vec<PressState> = p.iter().map(|p| p.state).collect();
    assert_eq!(st, vec![PressState::Confirmed, PressState::Normal, PressState::Unconfirmed, PressState::Unconfirmed, PressState::Unconfirmed]);
}

#[test]
fn ult_without_a_check_and_old_recordings() {
    let acts = default_actions();
    let mut s = GameSession::new("x".into(), "league", "League of Legends", chrono::Local::now());
    // Not checked (skipped: unknown HUD): live-accepted presses solid, filtered ones faded, the
    // same as the markers ("Ult pressed (unverified)" only for accepted ones).
    s.key_presses = vec![mark(10.0, true, None), mark(10.5, false, Some("cooldown"))];
    let mut p = vec![r_press(10.0), r_press(10.5)];
    press_states(&s, &acts, &mut p);
    assert_eq!(p.iter().map(|p| p.state).collect::<Vec<_>>(), vec![PressState::Normal, PressState::Unconfirmed]);
    // No ult presses logged at all (e.g. a game without the ult key log): untouched.
    s.key_presses.clear();
    let mut p = vec![r_press(10.0)];
    press_states(&s, &acts, &mut p);
    assert_eq!(p[0].state, PressState::Normal);
}

#[test]
fn ult_spam_pairs_one_to_one() {
    // R spam 100 ms apart (Practice Tool, no cooldowns): every second press was a cast.
    let acts = default_actions();
    let mut s = GameSession::new("x".into(), "league", "League of Legends", chrono::Local::now());
    s.verification = Some(verified());
    for i in 0..20 {
        let gt = 50.0 + i as f64 * 0.1;
        s.key_presses.push(mark(gt, true, None));
        if i % 2 == 1 {
            s.events.push(GameEvent::new(format!("ultp-{gt:.2}"), EventKind::UltUnconfirmed, gt, "Ult pressed, no cast"));
        }
    }
    let mut p: Vec<ActionPress> = (0..20).map(|i| r_press(50.0 + i as f64 * 0.1 + 0.03)).collect();
    press_states(&s, &acts, &mut p);
    for (i, q) in p.iter().enumerate() {
        assert_eq!(q.state, if i % 2 == 1 { PressState::Unconfirmed } else { PressState::Confirmed }, "press {i}");
    }
}
