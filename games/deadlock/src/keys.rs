//! Your own key presses on a match's timeline: abilities 1-4 (the fourth is the ultimate), the
//! four item slots, melee and parry, with the binds the game has ([`crate::binds`]).
//!
//! A press is all that is known live: Deadlock has no API that says whether the ability was
//! ready, so every press of a followed key is a timeline event unless it is clearly not a cast
//! (typing in the chat, the shop open, the same key again right away). Every press is also kept
//! as a [`KeyMark`], accepted or not, for the check against the replay file later.
//!
//! The engine only passes keys pressed while Deadlock has the focus and a match is in progress.

use crate::binds::{Action, Binds};
use cv_core::game::{KeyMark, KeyPress};
use cv_core::{EventKind, GameEvent};

/// Contact bounce: the same action again this soon is the same press.
const BOUNCE: f64 = 0.08;
/// The chat counts as closed again when no key at all was pressed for this long (nobody types
/// that slowly; it keeps a missed "closed" from hiding the rest of the match).
const CHAT_IDLE: f64 = 15.0;
/// The shop counts as closed again this long after it was opened, for the same reason.
const SHOP_MAX: f64 = 90.0;

impl Action {
    /// Name in key marks and event ids. The ultimate is "ult", as in every game.
    fn id(self) -> String {
        match self {
            Action::Ability(4) => "ult".into(),
            Action::Ability(n) => format!("ability{n}"),
            Action::Item(n) => format!("item{n}"),
            Action::Melee => "melee".into(),
            Action::Parry => "parry".into(),
            Action::Shop => "shop".into(),
            Action::Chat => "chat".into(),
        }
    }

    /// Timeline kind and title; `None` for the keys that only open something.
    fn event(self) -> Option<(EventKind, String)> {
        Some(match self {
            Action::Ability(4) => (EventKind::UltPressed, "Ultimate".into()),
            Action::Ability(n) => (EventKind::AbilityPressed, format!("Ability {n}")),
            Action::Item(n) => (EventKind::ItemPressed, format!("Item {n}")),
            Action::Melee => (EventKind::Melee, "Melee".into()),
            Action::Parry => (EventKind::Parry, "Parry".into()),
            Action::Shop | Action::Chat => return None,
        })
    }

    /// The same action again within this time gets no marker of its own (a mashed key). Melee
    /// swings follow each other quickly on purpose.
    fn repeat_gap(self) -> f64 {
        match self {
            Action::Melee => 0.4,
            _ => 1.0,
        }
    }
}

/// Live state for one match.
#[derive(Debug, Default)]
pub struct Tracker {
    binds: Binds,
    chat_open: bool,
    /// When the shop was opened (match clock).
    shop_open: Option<f64>,
    /// The last key press of any kind.
    last_key: f64,
    /// The last press of each action, and the last one that became an event.
    last_press: Vec<(Action, f64)>,
    last_event: Vec<(Action, f64)>,
    marks: Vec<KeyMark>,
}

fn last(times: &[(Action, f64)], action: Action) -> Option<f64> {
    times.iter().find(|(a, _)| *a == action).map(|(_, t)| *t)
}

fn set_last(times: &mut Vec<(Action, f64)>, action: Action, t: f64) {
    match times.iter_mut().find(|(a, _)| *a == action) {
        Some(known) => known.1 = t,
        None => times.push((action, t)),
    }
}

impl Tracker {
    pub fn new(binds: Binds) -> Self {
        Self { binds, ..Default::default() }
    }

    pub fn binds(&self) -> &Binds {
        &self.binds
    }

    /// A key press during the match, at `game_time` on the match clock. Returns the timeline
    /// event it makes, if any.
    pub fn press(&mut self, k: &KeyPress, game_time: f64) -> Option<GameEvent> {
        if self.chat_open && game_time - self.last_key > CHAT_IDLE {
            self.chat_open = false;
        }
        if self.shop_open.is_some_and(|t| game_time - t > SHOP_MAX) {
            self.shop_open = None;
        }
        self.last_key = game_time;
        // Escape closes whatever is open (when nothing is, it opens the menu: no cast either).
        if k.key == "Escape" {
            self.chat_open = false;
            self.shop_open = None;
            return None;
        }
        let hit = self.binds.action(k).map(|(action, bind)| (action, bind.label()));
        if self.chat_open {
            // Enter sends the line; everything else is typing.
            if k.key == "Enter" {
                self.chat_open = false;
            } else if let Some((action, key)) = hit {
                self.mark(action, key, game_time, Some("chat"));
            }
            return None;
        }
        let (action, key) = hit?;
        match action {
            Action::Chat => {
                self.chat_open = true;
                return None;
            }
            Action::Shop => {
                self.shop_open = if self.shop_open.is_some() { None } else { Some(game_time) };
                return None;
            }
            _ => {}
        }
        if last(&self.last_press, action).is_some_and(|t| (game_time - t).abs() < BOUNCE) {
            return None;
        }
        set_last(&mut self.last_press, action, game_time);
        if self.shop_open.is_some() {
            self.mark(action, key, game_time, Some("shop"));
            return None;
        }
        if last(&self.last_event, action).is_some_and(|t| game_time - t < action.repeat_gap()) {
            self.mark(action, key, game_time, Some("repeat"));
            return None;
        }
        set_last(&mut self.last_event, action, game_time);
        let (kind, title) = action.event()?;
        let event = GameEvent::new(format!("dl-{}-{game_time:.2}", action.id()), kind, game_time, title).fact("Key", key.as_str());
        self.mark(action, key, game_time, None);
        Some(event.with_details(details(action)))
    }

    /// Keeps a press of a followed key (never the shop / chat keys themselves).
    fn mark(&mut self, action: Action, key: String, game_time: f64, reason: Option<&str>) {
        if action.event().is_some() {
            self.marks.push(KeyMark { game_time, action: action.id(), key, accepted: reason.is_none(), reason: reason.map(str::to_string) });
        }
    }

    pub fn take_marks(&mut self) -> Vec<KeyMark> {
        std::mem::take(&mut self.marks)
    }
}

fn details(action: Action) -> &'static str {
    match action {
        Action::Parry => "The parry key was pressed (it also throws a held item such as the Soul Urn). Not checked against the replay.",
        Action::Melee => "The melee key was pressed. Not checked against the replay.",
        Action::Item(_) => "The key of this item slot was pressed. Whether an active item was in it and ready isn't known.",
        _ => "The key was pressed. Whether the ability was ready isn't known until the replay is checked.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kp(key: &str) -> KeyPress {
        KeyPress { key: key.into(), ctrl: false, shift: false, alt: false }
    }

    fn tracker() -> Tracker {
        Tracker::new(Binds::default())
    }

    /// (kind, title) of the event a press makes.
    fn press(t: &mut Tracker, key: &str, at: f64) -> Option<(EventKind, String)> {
        t.press(&kp(key), at).map(|e| (e.kind, e.title))
    }

    #[test]
    fn every_followed_key_makes_its_event() {
        let mut t = tracker();
        let expected = [
            ("1", EventKind::AbilityPressed, "Ability 1", "ability1"),
            ("2", EventKind::AbilityPressed, "Ability 2", "ability2"),
            ("3", EventKind::AbilityPressed, "Ability 3", "ability3"),
            ("4", EventKind::UltPressed, "Ultimate", "ult"),
            ("Z", EventKind::ItemPressed, "Item 1", "item1"),
            ("X", EventKind::ItemPressed, "Item 2", "item2"),
            ("C", EventKind::ItemPressed, "Item 3", "item3"),
            ("V", EventKind::ItemPressed, "Item 4", "item4"),
            ("Q", EventKind::Melee, "Melee", "melee"),
            ("F", EventKind::Parry, "Parry", "parry"),
        ];
        for (i, (key, kind, title, _)) in expected.iter().enumerate() {
            let e = t.press(&kp(key), 100.0 + i as f64).expect(key);
            assert_eq!((e.kind, e.title.as_str()), (*kind, *title));
            assert_eq!(e.facts, [("Key".to_string(), key.to_string())]);
            assert!(e.details.is_some());
        }
        // Keys of other things: nothing, and no mark.
        for other in ["W", "R", "Tab", "Space", "E", "5", "G"] {
            assert_eq!(press(&mut t, other, 120.0), None, "{other}");
        }
        let marks = t.take_marks();
        assert_eq!(marks.iter().map(|m| m.action.as_str()).collect::<Vec<_>>(), expected.map(|e| e.3));
        assert!(marks.iter().all(|m| m.accepted && m.reason.is_none()));
        assert_eq!((marks[3].game_time, marks[3].key.as_str()), (103.0, "4"));
        assert!(t.take_marks().is_empty());
    }

    #[test]
    fn event_ids_are_unique() {
        let mut t = tracker();
        let a = t.press(&kp("1"), 10.0).unwrap();
        let b = t.press(&kp("1"), 12.5).unwrap();
        let c = t.press(&kp("4"), 12.5).unwrap();
        assert_eq!((a.id.as_str(), b.id.as_str(), c.id.as_str()), ("dl-ability1-10.00", "dl-ability1-12.50", "dl-ult-12.50"));
    }

    #[test]
    fn a_mashed_key_is_one_marker() {
        let mut t = tracker();
        assert!(press(&mut t, "4", 50.0).is_some());
        assert_eq!(press(&mut t, "4", 50.03), None, "contact bounce");
        assert_eq!(press(&mut t, "4", 50.3), None);
        assert_eq!(press(&mut t, "4", 50.9), None);
        assert!(press(&mut t, "1", 50.9).is_some(), "another key is its own press");
        assert!(press(&mut t, "4", 51.2).is_some(), "a second later it counts again");
        // Melee swings come quickly on purpose.
        assert!(press(&mut t, "Q", 60.0).is_some());
        assert_eq!(press(&mut t, "Q", 60.2), None);
        assert!(press(&mut t, "Q", 60.5).is_some());
        let ult: Vec<(f64, bool, Option<&str>)> = t.marks.iter().filter(|m| m.action == "ult").map(|m| (m.game_time, m.accepted, m.reason.as_deref())).collect();
        assert_eq!(ult, [(50.0, true, None), (50.3, false, Some("repeat")), (50.9, false, Some("repeat")), (51.2, true, None)], "the bounce leaves no mark");
    }

    #[test]
    fn modifiers() {
        let mut t = tracker();
        assert_eq!(t.press(&KeyPress { alt: true, ..kp("1") }, 10.0), None, "Alt+1 upgrades the ability");
        assert!(t.take_marks().is_empty());
        assert!(t.press(&KeyPress { shift: true, ..kp("1") }, 11.0).is_some(), "cast while dashing");
        assert!(t.press(&KeyPress { ctrl: true, ..kp("2") }, 12.0).is_some(), "cast while crouching");
    }

    #[test]
    fn typing_in_the_chat_is_no_cast() {
        let mut t = tracker();
        assert_eq!(press(&mut t, "Enter", 10.0), None);
        for (i, key) in ["F", "1", "H", "Q"].iter().enumerate() {
            assert_eq!(press(&mut t, key, 11.0 + i as f64), None, "{key}");
        }
        assert_eq!(press(&mut t, "Enter", 15.0), None, "sent");
        assert!(press(&mut t, "1", 16.0).is_some());
        let marks = t.take_marks();
        let got: Vec<(&str, Option<&str>)> = marks.iter().map(|m| (m.action.as_str(), m.reason.as_deref())).collect();
        assert_eq!(got, [("parry", Some("chat")), ("ability1", Some("chat")), ("melee", Some("chat")), ("ability1", None)]);

        // All chat (Shift+Enter), closed with Escape. "B" typed in the chat doesn't open the shop.
        assert_eq!(t.press(&KeyPress { shift: true, ..kp("Enter") }, 20.0), None);
        assert_eq!(press(&mut t, "B", 21.0), None);
        assert_eq!(press(&mut t, "Escape", 22.0), None);
        assert!(press(&mut t, "2", 23.0).is_some());

        // A chat that was never seen closing (closed with the mouse): forgotten after a pause.
        assert_eq!(press(&mut t, "Enter", 30.0), None);
        assert_eq!(press(&mut t, "3", 31.0), None);
        assert!(press(&mut t, "3", 31.0 + CHAT_IDLE + 0.5).is_some());
    }

    #[test]
    fn the_shop_takes_the_keys() {
        let mut t = tracker();
        assert_eq!(press(&mut t, "B", 10.0), None);
        assert_eq!(press(&mut t, "4", 11.0), None, "the Weapon tab");
        assert_eq!(press(&mut t, "B", 12.0), None, "closed");
        assert!(press(&mut t, "4", 13.0).is_some());
        assert_eq!(press(&mut t, "B", 20.0), None);
        assert_eq!(press(&mut t, "Escape", 21.0), None);
        assert!(press(&mut t, "Z", 22.0).is_some());
        // A shop never seen closing: forgotten after a while.
        assert_eq!(press(&mut t, "B", 30.0), None);
        assert_eq!(press(&mut t, "1", 40.0), None);
        assert!(press(&mut t, "1", 30.0 + SHOP_MAX + 1.0).is_some());
        let reasons: Vec<Option<&str>> = t.marks.iter().map(|m| m.reason.as_deref()).collect();
        assert_eq!(reasons, [Some("shop"), None, None, Some("shop"), None]);
    }

    #[test]
    fn the_games_own_binds_are_used() {
        let text = r#""KeyBindings" { "Keys" { "Ability4" { "Key" "R" } "HeldItem" { "Key" "CAPSLOCK" } } }"#;
        let mut t = Tracker::new(Binds::parse(text));
        assert_eq!(press(&mut t, "4", 10.0), None);
        assert_eq!(press(&mut t, "R", 11.0), Some((EventKind::UltPressed, "Ultimate".to_string())));
        let parry = t.press(&kp("Key20"), 12.0).unwrap();
        assert_eq!((parry.kind, parry.facts[0].1.as_str()), (EventKind::Parry, "Capslock"));
    }
}
