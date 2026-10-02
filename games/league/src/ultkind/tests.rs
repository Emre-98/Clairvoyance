use super::*;
use crate::ult::UltRules;

fn rule(kind: UltKind, dur: f64) -> KindRule {
    KindRule { kind, duration: vec![dur], ..KindRule::normal() }
}
fn p(t: f64) -> PressIn {
    PressIn { t, ok: true }
}
fn never(_: f64) -> bool {
    false
}
/// (used, recasts, form swaps, no-casts) and the "Ult used" times.
fn count(l: &[Label]) -> (usize, usize, usize, usize, Vec<f64>) {
    let mut used = Vec::new();
    let (mut r, mut f, mut n) = (0, 0, 0);
    for x in l {
        match *x {
            Label::Used { at, .. } => used.push(at),
            Label::Recast { .. } => r += 1,
            Label::FormSwap { .. } => f += 1,
            Label::NoCast { .. } => n += 1,
        }
    }
    used.sort_by(|a, b| a.total_cmp(b));
    (used.len(), r, f, n, used)
}

// ---------- rules file ----------

#[test]
fn rules_file_kinds() {
    let r = UltRules::builtin();
    assert_eq!(r.version, 2);
    for (c, k) in [
        ("Annie", UltKind::Command),
        ("Ivern", UltKind::Command),
        ("Shaco", UltKind::Command),
        ("Yorick", UltKind::Command),
        ("Viktor", UltKind::Command),
        ("Ahri", UltKind::MultiCast),
        ("Zed", UltKind::MultiCast),
        ("Sylas", UltKind::MultiCast),
        ("Leblanc", UltKind::MultiCast),
        ("Jayce", UltKind::Transform),
        ("nidalee", UltKind::Transform),
        ("Udyr", UltKind::Transform),
        ("KogMaw", UltKind::ChargesOrReset),
        ("Corki", UltKind::ChargesOrReset),
        ("Shyvana", UltKind::Normal),
    ] {
        assert_eq!(r.kind_for(c, None).kind, k, "{c}");
    }
    assert!(r.kind_for("Corki", None).ammo && r.kind_for("Teemo", None).ammo);
    assert!(!r.kind_for("KogMaw", None).ammo);
    assert_eq!(r.kind_for("Annie", None).cap(Some(2)), 45.0);
    assert_eq!(r.kind_for("Sylas", None).grace, 20.0);
    assert_eq!(r.kind_for("Ahri", None).ends, vec![End::Cooldown, End::Duration]);
    // Not listed: normal, unless Data Dragon's text looks like a recast.
    assert_eq!(r.kind_for("Caitlyn", None).kind, UltKind::Normal);
    assert_eq!(r.kind_for("Newchamp", Some(UltKind::MultiCast)).kind, UltKind::MultiCast);
    assert_eq!(r.kind_for("Newchamp", Some(UltKind::MultiCast)).cap(None), DEFAULT_EPISODE);
    assert_eq!(r.kind_for("Annie", Some(UltKind::Normal)).kind, UltKind::Command, "the rules file wins");
    // Every non-normal kind skips the live cooldown filter.
    assert!(r.skips_cooldown("Velkoz") && r.skips_cooldown("Udyr"));
    // A v1 file (no kinds) still parses: everything normal.
    let v1 = UltRules::parse(r#"{"version":1,"skip_cooldown_filter":{"Ahri":"x"}}"#).unwrap();
    assert_eq!(v1.kind_for("Annie", None).kind, UltKind::Normal);
    assert!(UltRules::parse(r#"{"kinds":{"Annie":{"kind":"summon"}}}"#).is_err(), "unknown kind");
}

// ---------- Data Dragon scan ----------

fn ddragon() -> serde_json::Value {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/ddragon/r-spells.json");
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

fn scan(c: &serde_json::Value) -> (Option<UltKind>, Vec<&'static str>) {
    let cd: Vec<f64> = c["cooldown"].as_array().unwrap().iter().filter_map(|x| x.as_f64()).collect();
    let ammo = c["maxammo"].as_str().and_then(|s| s.parse().ok());
    classify_text(c["description"].as_str().unwrap_or(""), c["tooltip"].as_str().unwrap_or(""), ammo, &cd)
}

#[test]
fn ddragon_scan_finds_the_known_cases() {
    let d = ddragon();
    let ch = d["champions"].as_object().unwrap();
    assert!(ch.len() >= 170, "{} champions", ch.len());
    let k = |n: &str| scan(&ch[n]).0;
    assert_eq!(k("Annie"), Some(UltKind::MultiCast), "recast text (the rules file says command)");
    assert_eq!(k("Ivern"), Some(UltKind::Command));
    assert_eq!(k("Ahri"), Some(UltKind::MultiCast));
    assert_eq!(k("Jayce"), Some(UltKind::Transform));
    assert_eq!(k("Nidalee"), Some(UltKind::Transform));
    assert_eq!(k("Corki"), Some(UltKind::ChargesOrReset));
    assert_eq!(k("Teemo"), Some(UltKind::ChargesOrReset));
    assert_eq!(k("Kassadin"), Some(UltKind::ChargesOrReset), "'each subsequent use'");
    assert_eq!(k("Caitlyn"), None);
    assert_eq!(k("Ashe"), None, "'distance' isn't 'stance'");
    assert_eq!(k("Zeri"), None, "'discharges' isn't 'charges'");
}

#[test]
fn every_scan_candidate_is_decided_in_the_rules_file() {
    // The rules file must decide every champion the scan flags (a recast-like R that's neither
    // listed nor marked normal fails here: re-run the scan after a patch and check it), and every
    // listed kind the text doesn't show must say where it was checked (the League wiki).
    let r = UltRules::builtin();
    let d = ddragon();
    let mut missing = Vec::new();
    let mut unbacked = Vec::new();
    for (name, c) in d["champions"].as_object().unwrap() {
        let (guess, hits) = scan(c);
        let listed = r.kind_rule(name);
        if matches!(guess, Some(UltKind::MultiCast | UltKind::Command)) && listed.is_none() && !OK_UNLISTED.contains(&name.as_str()) {
            missing.push(format!("{name} {hits:?}"));
        }
        if let Some(l) = listed {
            let short_cd = c["cooldown"].as_array().unwrap().iter().all(|x| x.as_f64().unwrap_or(99.0) <= 10.0);
            let backed = guess.is_some() || l.note.contains("wiki") || (matches!(l.kind, UltKind::ChargesOrReset | UltKind::Transform) && short_cd);
            if l.kind != UltKind::Normal && !backed {
                unbacked.push(format!("{name} ({:?})", l.kind));
            }
        }
    }
    assert!(missing.is_empty(), "scan candidates not in ult_rules.json: {missing:?}");
    assert!(unbacked.is_empty(), "listed but neither the R text, a short cooldown nor a wiki check backs it: {unbacked:?}");
}

/// Flagged by the scan, checked and left normal (one press = one ult): the "recast" in their text
/// is a refresh on takedowns that the cooldown on the bar shows as a new cast anyway.
const OK_UNLISTED: [&str; 0] = [];

// ---------- episodes ----------

#[test]
fn command_annie_one_ult_many_commands() {
    // Tibbers at 100: the R icon shows Tibbers' command icon until he's gone (145), then the
    // cooldown. 10 commands; a press on cooldown; the next ult at 260.
    let sig = Signals {
        casts: vec![CastSig { t: 145.0, long: true }, CastSig { t: 260.1, long: false }],
        alt: vec![(100.1, 145.0), (260.1, 300.0)],
        ready: vec![80.0, 90.0, 240.0, 250.0],
        deaths: vec![],
    };
    let mut presses = vec![p(99.9)];
    presses.extend((0..10).map(|i| p(102.0 + i as f64 * 2.0)));
    presses.push(p(150.0));
    presses.push(p(260.0));
    let l = label(&rule(UltKind::Command, 45.0), 1.5, &sig, &presses, &never);
    let (used, rec, _, no, at) = count(&l);
    assert_eq!((used, rec, no), (2, 10, 1), "{l:?}");
    assert_eq!(at, vec![100.1, 260.1]);
    assert!(l.contains(&Label::Used { at: 100.1, press: Some(0) }));
    assert!(l.contains(&Label::NoCast { press: 11 }), "on cooldown after Tibbers died");
}

#[test]
fn command_ends_when_the_icon_is_ready_again() {
    // Ivern-like with an icon end: back to the ready look at 130 (no cooldown seen in between).
    let r = KindRule { ends: vec![End::Icon, End::Duration], ..rule(UltKind::Command, 60.0) };
    let sig = Signals { casts: vec![], alt: vec![(100.0, 129.5)], ready: vec![90.0, 130.0, 131.0], deaths: vec![] };
    let e = episodes(&r, &sig);
    assert_eq!(e, vec![Episode { start: 100.0, end: 130.0 }]);
}

#[test]
fn multi_cast_ahri_three_dashes_and_presses_at_the_end() {
    // Each dash shows a short lockout; the long cooldown appears after the last one (210).
    let r = KindRule { ends: vec![End::Cooldown, End::Duration], ..rule(UltKind::MultiCast, 20.0) };
    let sig = Signals {
        casts: vec![CastSig { t: 200.2, long: false }, CastSig { t: 202.2, long: false }, CastSig { t: 210.0, long: true }],
        ..Default::default()
    };
    let presses = [p(200.0), p(202.0), p(209.8), p(210.4), p(300.0)];
    let l = label(&r, 1.5, &sig, &presses, &never);
    assert_eq!(count(&l).0, 1);
    assert!(l.contains(&Label::Used { at: 200.2, press: Some(0) }));
    assert!(l.contains(&Label::Recast { press: 1, episode: 0 }));
    assert!(l.contains(&Label::Recast { press: 2, episode: 0 }), "right before the end: still the same ult");
    assert!(l.contains(&Label::NoCast { press: 3 }), "right after the end: on cooldown");
    assert!(l.contains(&Label::NoCast { press: 4 }));
}

#[test]
fn a_new_ult_right_after_the_old_one_ends() {
    // Akali-like with a short cooldown: the first ult ends with its cooldown at 110, the next
    // activation (115) is a new ult.
    let r = rule(UltKind::MultiCast, 10.5);
    let sig = Signals { casts: vec![CastSig { t: 100.0, long: false }, CastSig { t: 110.0, long: true }, CastSig { t: 115.0, long: false }], ..Default::default() };
    let l = label(&r, 1.2, &sig, &[p(99.9), p(109.8), p(114.9), p(116.0)], &never);
    let (used, rec, _, no, at) = count(&l);
    assert_eq!((used, rec, no), (2, 2, 0), "{l:?}");
    assert_eq!(at, vec![100.0, 115.0]);
}

#[test]
fn ult_shown_late_on_the_bar_uses_the_press_time() {
    // A recast window before the cooldown shows (and no other sign): "Ult used" at the first
    // press, not when the bar changed 9 s later.
    let r = KindRule { ends: vec![End::Cooldown, End::Duration], ..rule(UltKind::MultiCast, 20.0) };
    let sig = Signals { casts: vec![CastSig { t: 209.0, long: true }], ..Default::default() };
    let l = label(&r, 15.0, &sig, &[p(200.0), p(203.0), p(206.0)], &never);
    assert_eq!(count(&l).4, vec![200.0]);
    assert_eq!(count(&l).1, 2);
}

#[test]
fn death_ends_a_multi_cast_but_not_a_summon() {
    let sig = Signals { casts: vec![CastSig { t: 300.0, long: false }], deaths: vec![302.0], ..Default::default() };
    let presses = [p(299.9), p(301.0), PressIn { t: 302.5, ok: false }, p(303.0)];
    let l = label(&rule(UltKind::MultiCast, 7.5), 1.2, &sig, &presses, &never);
    assert_eq!(count(&l).0..count(&l).0 + 1, 1..2);
    assert!(l.contains(&Label::Recast { press: 1, episode: 0 }));
    assert!(l.contains(&Label::NoCast { press: 2 }), "dead");
    assert!(l.contains(&Label::NoCast { press: 3 }), "after the death the recast is gone");
    // A summon outlives its owner: the episode goes on.
    let l = label(&rule(UltKind::Command, 45.0), 1.2, &sig, &presses, &never);
    assert!(l.contains(&Label::Recast { press: 3, episode: 0 }));
    assert!(l.contains(&Label::NoCast { press: 2 }), "a press while dead is never a recast");
}

#[test]
fn sylas_steals_a_recast_ult() {
    // Hijack at 400 (R shows the stolen ult's icon), the stolen Ahri cast at 450 puts Hijack on
    // cooldown, her two more dashes follow: one ult, three recasts. 480: nothing.
    let r = UltRules::builtin().kind_for("Sylas", None);
    let sig = Signals { casts: vec![CastSig { t: 450.2, long: true }], alt: vec![(400.1, 450.2)], ready: vec![390.0], deaths: vec![] };
    let l = label(&r, 1.2, &sig, &[p(400.0), p(450.0), p(452.0), p(455.0), p(480.0)], &never);
    let (used, rec, _, no, at) = count(&l);
    assert_eq!((used, rec, no), (1, 3, 1), "{l:?}");
    assert_eq!(at, vec![400.1]);
}

#[test]
fn transform_every_cast_is_a_form_swap() {
    let sig = Signals { casts: (0..6).map(|i| CastSig { t: 100.0 + i as f64 * 7.0, long: true }).collect(), ..Default::default() };
    let presses: Vec<PressIn> = (0..6).map(|i| p(99.9 + i as f64 * 7.0)).chain([p(101.0)]).collect();
    let l = label(&rule(UltKind::Transform, 0.0), 1.2, &sig, &presses, &never);
    let (used, _, swaps, no, _) = count(&l);
    assert_eq!((used, swaps, no), (0, 6, 1), "a press on the 6 s cooldown is no cast");
}

#[test]
fn charges_every_press_is_a_cast() {
    // Kog'Maw: each shot shows its short cooldown.
    let sig = Signals { casts: vec![CastSig { t: 10.1, long: false }, CastSig { t: 12.1, long: false }], ..Default::default() };
    let l = label(&rule(UltKind::ChargesOrReset, 0.0), 1.2, &sig, &[p(10.0), p(12.0)], &never);
    assert_eq!(count(&l).0, 2);
    // Corki: with charges left there's no cooldown on the icon; a press while it isn't on
    // cooldown is a cast, one while it is (out of charges) or typed in chat isn't.
    let corki = UltRules::builtin().kind_for("Corki", None);
    let ready = |t: f64| t < 60.0;
    let l = label(&corki, 1.2, &Signals::default(), &[p(50.0), p(52.0), p(54.0), p(61.0), PressIn { t: 55.0, ok: false }], &ready);
    let (used, _, _, no, at) = count(&l);
    assert_eq!((used, no), (3, 2));
    assert_eq!(at, vec![50.0, 52.0, 54.0]);
}

#[test]
fn normal_is_the_v13_matching() {
    let casts = [10.1, 31.0, 70.0];
    let presses = [10.0, 30.0, 30.5, 50.0];
    let sig = Signals { casts: casts.iter().map(|&t| CastSig { t, long: true }).collect(), ..Default::default() };
    let l = label(&KindRule::normal(), 1.5, &sig, &presses.iter().map(|&t| p(t)).collect::<Vec<_>>(), &never);
    let o = crate::verify::match_up(&casts, &presses, 1.5, false);
    assert_eq!(l.len(), o.len());
    assert_eq!(count(&l).0, 3);
    assert_eq!(count(&l).3, 2);
}

// ---------- live ----------

#[test]
fn live_episodes() {
    let mut e = LiveEpisodes::default();
    assert_eq!(e.press(UltKind::Command, 45.0, 100.0), LivePress::Used);
    assert_eq!(e.press(UltKind::Command, 45.0, 110.0), LivePress::Recast);
    assert_eq!(e.press(UltKind::Command, 45.0, 146.0), LivePress::Recast, "cap + 1 s slack");
    assert_eq!(e.press(UltKind::Command, 45.0, 147.0), LivePress::Used, "a new ult after the cap");
    // The R ability's name back to normal ends it early.
    e.ended(160.0);
    assert_eq!(e.press(UltKind::Command, 45.0, 161.0), LivePress::Used);
    // ...but not in the first half second (the name may not have changed yet).
    e.ended(161.2);
    assert_eq!(e.press(UltKind::Command, 45.0, 162.0), LivePress::Recast);
    let mut e = LiveEpisodes::default();
    assert_eq!(e.press(UltKind::Transform, 0.0, 1.0), LivePress::FormSwap);
    assert_eq!(e.press(UltKind::ChargesOrReset, 0.0, 1.0), LivePress::Used);
    assert_eq!(e.press(UltKind::Normal, 0.0, 1.0), LivePress::Used);
}
