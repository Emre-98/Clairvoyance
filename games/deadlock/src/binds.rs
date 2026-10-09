//! Deadlock's key binds, read from the file the game itself keeps them in.
//!
//! Checked on the owner's PC (2026-10-09, game build 6766): the gameplay binds are **not** in the
//! Source engine's `user_keys_*.vcfg` files (those hold console binds only: the owner's has just
//! `F7 = toggleconsole`, and `game\citadel\cfg\user_keys_default.vcfg` has no ability at all).
//! They are in `userdata\<account>\1422450\remote\cfg\citadelkeys_personal.lst`, a Valve
//! KeyValues text file that lists every action with its key:
//!
//! ```text
//! "KeyBindings"
//! {
//!     "Keys"
//!     {
//!         "Ability1"          { "Key" "1" }
//!         "AbilityUpgrade1"   { "Key" "1" "Modifier" "ALT" }
//!         "CancelAbility"     { "Key" "SPACE" "Key2" "NONE" }
//!     }
//! }
//! ```
//!
//! The file is only read, with nothing held open. Actions it doesn't list (or no file at all:
//! binds never opened) use the game's defaults.

use crate::paths;
use cv_core::game::KeyPress;
use std::path::{Path, PathBuf};

/// A node of a Valve KeyValues text file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kv {
    Value(String),
    Block(Vec<(String, Kv)>),
}

impl Kv {
    /// The first child called `key` (any case).
    pub fn get(&self, key: &str) -> Option<&Kv> {
        self.items().iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v)
    }
    /// The text of the child called `key`.
    pub fn text(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            Kv::Value(v) => Some(v.as_str()),
            Kv::Block(_) => None,
        }
    }
    pub fn items(&self) -> &[(String, Kv)] {
        match self {
            Kv::Block(items) => items,
            Kv::Value(_) => &[],
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Token {
    Text(String),
    Open,
    Close,
}

fn tokens(text: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' => out.push(Token::Open),
            '}' => out.push(Token::Close),
            '"' => {
                let mut s = String::new();
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' => s.extend(chars.next()),
                        c => s.push(c),
                    }
                }
                out.push(Token::Text(s));
            }
            // A comment up to the end of the line.
            '/' if chars.peek() == Some(&'/') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            c if c.is_whitespace() => {}
            // A word without quotes.
            c => {
                let mut s = String::from(c);
                while let Some(n) = chars.next_if(|&n| !n.is_whitespace() && !matches!(n, '{' | '}' | '"')) {
                    s.push(n);
                }
                out.push(Token::Text(s));
            }
        }
    }
    out
}

fn block(tokens: &mut std::vec::IntoIter<Token>) -> Vec<(String, Kv)> {
    let mut items = Vec::new();
    while let Some(token) = tokens.next() {
        let key = match token {
            Token::Text(key) => key,
            Token::Close => break,
            Token::Open => continue,
        };
        match tokens.next() {
            Some(Token::Text(value)) => items.push((key, Kv::Value(value))),
            Some(Token::Open) => {
                let inner = block(tokens);
                items.push((key, Kv::Block(inner)));
            }
            Some(Token::Close) | None => break,
        }
    }
    items
}

/// Reads a KeyValues text; whatever is broken or cut off is left out.
pub fn parse_kv(text: &str) -> Kv {
    Kv::Block(block(&mut tokens(text).into_iter()))
}

/// A key as Valve's files name it ("Z", "SPACE", "UPARROW"), in the form key presses arrive in
/// ([`KeyPress::key`]: "Z", "Space", "Key38"). `None` = not a keyboard key the app sees: mouse
/// buttons, the wheel, a controller, and Shift / Ctrl / Alt on their own.
///
/// Seen in the owner's file: letters, digits, `-`, `=`, SPACE, ENTER, TAB, ESCAPE, BACKSPACE,
/// F1-F7, the four arrows, SHIFT, CTRL, ALT, MOUSE1-3, MWHEELUP / MWHEELDOWN, NONE. The other
/// names are the ones Valve's engine uses everywhere (`game\core\cfg\user_keys_default.vcfg`).
pub fn key_name(valve: &str) -> Option<String> {
    let vk = |code: u32| Some(format!("Key{code}"));
    let up = valve.trim().to_ascii_uppercase();
    let mut chars = up.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        return match c {
            'A'..='Z' | '0'..='9' | '`' => Some(c.to_string()),
            ';' => vk(186),
            '=' => vk(187),
            ',' => vk(188),
            '-' => vk(189),
            '.' => vk(190),
            '/' => vk(191),
            '[' => vk(219),
            '\\' => vk(220),
            ']' => vk(221),
            '\'' => vk(222),
            _ => None,
        };
    }
    let name = |s: &str| Some(s.to_string());
    match up.as_str() {
        "SPACE" => name("Space"),
        "ENTER" | "KP_ENTER" => name("Enter"),
        "ESCAPE" => name("Escape"),
        "TAB" => name("Tab"),
        "BACKSPACE" => name("Backspace"),
        "INS" => name("Insert"),
        "DEL" => name("Delete"),
        "HOME" => name("Home"),
        "END" => name("End"),
        "PGUP" => name("Pageup"),
        "PGDN" => name("Pagedown"),
        "PAUSE" => name("Pause"),
        "CAPSLOCK" => vk(20),
        "LEFTARROW" => vk(37),
        "UPARROW" => vk(38),
        "RIGHTARROW" => vk(39),
        "DOWNARROW" => vk(40),
        "SEMICOLON" => vk(186),
        "KP_MULTIPLY" => vk(106),
        "KP_PLUS" => vk(107),
        "KP_MINUS" => vk(109),
        "KP_DEL" => vk(110),
        "KP_SLASH" | "KP_DIVIDE" => vk(111),
        "KP_INS" | "KP_0" => name("Num0"),
        "KP_END" | "KP_1" => name("Num1"),
        "KP_DOWNARROW" | "KP_2" => name("Num2"),
        "KP_PGDN" | "KP_3" => name("Num3"),
        "KP_LEFTARROW" | "KP_4" => name("Num4"),
        "KP_5" => name("Num5"),
        "KP_RIGHTARROW" | "KP_6" => name("Num6"),
        "KP_HOME" | "KP_7" => name("Num7"),
        "KP_UPARROW" | "KP_8" => name("Num8"),
        "KP_PGUP" | "KP_9" => name("Num9"),
        f if f.strip_prefix('F').is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())) => Some(f.to_string()),
        _ => None,
    }
}

/// The key held with a bind's key ("Modifier" in the file).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modifier {
    Shift,
    Ctrl,
    Alt,
}

impl Modifier {
    fn parse(valve: &str) -> Option<Modifier> {
        match valve.trim().to_ascii_uppercase().as_str() {
            "SHIFT" | "LSHIFT" | "RSHIFT" => Some(Modifier::Shift),
            "CTRL" | "LCTRL" | "RCTRL" => Some(Modifier::Ctrl),
            "ALT" | "LALT" | "RALT" => Some(Modifier::Alt),
            _ => None,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Modifier::Shift => "Shift",
            Modifier::Ctrl => "Ctrl",
            Modifier::Alt => "Alt",
        }
    }
    fn held(self, k: &KeyPress) -> bool {
        match self {
            Modifier::Shift => k.shift,
            Modifier::Ctrl => k.ctrl,
            Modifier::Alt => k.alt,
        }
    }
}

/// One key an action is bound to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bind {
    /// The key as the file names it ("Z", "SPACE", "MOUSE4").
    pub valve: String,
    /// The key as key presses name it; `None` = one the app doesn't see (see [`key_name`]).
    pub key: Option<String>,
    pub modifier: Option<Modifier>,
}

impl Bind {
    /// `None` for an empty slot ("NONE").
    fn new(valve: &str, modifier: Option<&str>) -> Option<Bind> {
        let valve = valve.trim().to_ascii_uppercase();
        if valve.is_empty() || valve == "NONE" {
            return None;
        }
        Some(Bind { key: key_name(&valve), modifier: modifier.and_then(Modifier::parse), valve })
    }

    /// For the timeline: "1", "Alt+1", "Space", "Mouse4".
    pub fn label(&self) -> String {
        let key = match self.valve.as_str() {
            v if v.chars().count() == 1 => v.to_string(),
            "UPARROW" => "Up".into(),
            "DOWNARROW" => "Down".into(),
            "LEFTARROW" => "Left".into(),
            "RIGHTARROW" => "Right".into(),
            v => {
                let mut rest = v.chars();
                rest.next().map(|first| first.to_string() + &rest.as_str().to_ascii_lowercase()).unwrap_or_default()
            }
        };
        match self.modifier {
            Some(m) => format!("{}+{key}", m.label()),
            None => key,
        }
    }
}

/// What a key does, of the things this module follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Abilities 1-4; the fourth is the hero's ultimate.
    Ability(u8),
    /// The four active item slots.
    Item(u8),
    Melee,
    /// "Melee parry / throw held item" in the game's settings.
    Parry,
    /// "Open shop": while it is open the number keys switch its tabs and letters go to its search.
    Shop,
    /// "Chat (team)" and "Chat (all)": typing follows.
    Chat,
}

/// The actions followed, by their names in the game's file.
const ACTIONS: [(&str, Action); 13] = [
    ("Ability1", Action::Ability(1)),
    ("Ability2", Action::Ability(2)),
    ("Ability3", Action::Ability(3)),
    ("Ability4", Action::Ability(4)),
    ("Item1", Action::Item(1)),
    ("Item2", Action::Item(2)),
    ("Item3", Action::Item(3)),
    ("Item4", Action::Item(4)),
    ("AbilityMelee", Action::Melee),
    ("HeldItem", Action::Parry),
    ("OpenHeroSheet", Action::Shop),
    ("ChatTeam", Action::Chat),
    ("Chat", Action::Chat),
];

/// The game's default binds for the actions followed and for the chords on their keys, exactly
/// as the owner's file has them (he changed only the console key).
const DEFAULTS: &str = r#"
"KeyBindings"
{
    "Keys"
    {
        "AbilityMelee" { "Key" "Q" }
        "Ability1" { "Key" "1" }
        "Ability2" { "Key" "2" }
        "Ability3" { "Key" "3" }
        "Ability4" { "Key" "4" }
        "Item1" { "Key" "Z" }
        "Item2" { "Key" "X" }
        "Item3" { "Key" "C" }
        "Item4" { "Key" "V" }
        "HeldItem" { "Key" "F" }
        "OpenHeroSheet" { "Key" "B" }
        "ChatTeam" { "Key" "ENTER" }
        "Chat" { "Key" "ENTER" "Modifier" "SHIFT" }
        "AbilityUpgrade1" { "Key" "1" "Modifier" "ALT" }
        "AbilityUpgrade2" { "Key" "2" "Modifier" "ALT" }
        "AbilityUpgrade3" { "Key" "3" "Modifier" "ALT" }
        "AbilityUpgrade4" { "Key" "4" "Modifier" "ALT" }
    }
}
"#;

/// Every action of a binds file with its keys ("Key" + "Modifier", "Key2" + "Modifier2").
fn entries(text: &str) -> Vec<(String, Vec<Bind>)> {
    let kv = parse_kv(text);
    let Some(keys) = kv.get("KeyBindings").and_then(|k| k.get("Keys")) else { return Vec::new() };
    let binds = |a: &Kv| [("Key", "Modifier"), ("Key2", "Modifier2")].iter().filter_map(|(k, m)| Bind::new(a.text(k)?, a.text(m))).collect::<Vec<_>>();
    keys.items().iter().map(|(name, action)| (name.clone(), binds(action))).collect()
}

/// The binds in effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binds {
    /// Every key of the actions followed.
    actions: Vec<(Action, Bind)>,
    /// Key + modifier of everything else that is bound to a chord: pressing one of those isn't
    /// the plain key's action (Alt+1 upgrades ability 1, it doesn't cast it).
    chords: Vec<(String, Modifier)>,
    /// Read from the game's file (false: the defaults).
    pub from_file: bool,
}

impl Default for Binds {
    fn default() -> Self {
        Binds::parse("")
    }
}

impl Binds {
    /// The binds of a `citadelkeys_personal.lst`; what it doesn't list stays at the defaults.
    pub fn parse(text: &str) -> Binds {
        let mut all = entries(DEFAULTS);
        let file = entries(text);
        let from_file = !file.is_empty();
        for (name, binds) in file {
            match all.iter_mut().find(|(n, _)| n.eq_ignore_ascii_case(&name)) {
                Some(known) => known.1 = binds,
                None => all.push((name, binds)),
            }
        }
        let (mut actions, mut chords) = (Vec::new(), Vec::new());
        for (name, binds) in all {
            let action = ACTIONS.iter().find(|(n, _)| n.eq_ignore_ascii_case(&name)).map(|(_, a)| *a);
            for bind in binds {
                match action {
                    Some(a) => actions.push((a, bind)),
                    None => chords.extend(bind.key.zip(bind.modifier)),
                }
            }
        }
        Binds { actions, chords, from_file }
    }

    /// What this key press does, with the bind it matched. A plain bind also counts with Shift /
    /// Ctrl / Alt held (you cast while dashing or crouching), unless that chord is bound itself.
    pub fn action(&self, k: &KeyPress) -> Option<(Action, &Bind)> {
        let on_key = |b: &Bind| b.key.as_deref().is_some_and(|key| key.eq_ignore_ascii_case(&k.key));
        if let Some((a, b)) = self.actions.iter().find(|(_, b)| on_key(b) && b.modifier.is_some_and(|m| m.held(k))) {
            return Some((*a, b));
        }
        if self.chords.iter().any(|(key, m)| key.eq_ignore_ascii_case(&k.key) && m.held(k)) {
            return None;
        }
        self.actions.iter().find(|(_, b)| on_key(b) && b.modifier.is_none()).map(|(a, b)| (*a, b))
    }

    /// The keys of an action, for the log and the tests.
    pub fn keys(&self, action: Action) -> Vec<String> {
        self.actions.iter().filter(|(a, _)| *a == action).map(|(_, b)| b.label()).collect()
    }

    /// One line for the log: "abilities 1 2 3 4, items Z X C V, melee Q, parry F".
    pub fn summary(&self) -> String {
        let one = |a: &Action| Some(self.keys(*a).join("/")).filter(|k| !k.is_empty()).unwrap_or_else(|| "-".into());
        let keys = |actions: &[Action]| actions.iter().map(&one).collect::<Vec<_>>().join(" ");
        let abilities = [Action::Ability(1), Action::Ability(2), Action::Ability(3), Action::Ability(4)];
        let items = [Action::Item(1), Action::Item(2), Action::Item(3), Action::Item(4)];
        format!("abilities {}, items {}, melee {}, parry {}", keys(&abilities), keys(&items), keys(&[Action::Melee]), keys(&[Action::Parry]))
    }

    /// Binds of the followed actions on keys the app doesn't see (a mouse button, Shift alone).
    pub fn unseen(&self) -> Vec<String> {
        let followed = |a: &Action| !matches!(a, Action::Shop | Action::Chat);
        self.actions.iter().filter(|(a, b)| followed(a) && b.key.is_none()).map(|(a, b)| format!("{a:?} on {}", b.label())).collect()
    }
}

/// The binds file of the Steam account that played last (`userdata\<account>\1422450\remote\cfg`).
pub fn personal_file(steam: &Path) -> Option<PathBuf> {
    let file = |user: PathBuf| user.join(paths::APP_ID).join("remote").join("cfg").join("citadelkeys_personal.lst");
    let dated = |p: PathBuf| Some((std::fs::metadata(&p).and_then(|m| m.modified()).ok()?, p));
    paths::user_dirs(steam).into_iter().map(file).filter_map(dated).max_by_key(|(t, _)| *t).map(|(_, p)| p)
}

/// The binds in effect now: the game's file, or the defaults when there is none.
pub fn load(steam: Option<&Path>) -> Binds {
    let text = steam.and_then(personal_file).and_then(|p| std::fs::read_to_string(p).ok());
    Binds::parse(text.as_deref().unwrap_or(""))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The owner's file (2026-10-09, game build 6766), unchanged.
    const OWNER: &str = include_str!("../tests/fixtures/citadelkeys-personal.txt");

    fn kp(key: &str) -> KeyPress {
        KeyPress { key: key.into(), ctrl: false, shift: false, alt: false }
    }

    #[test]
    fn key_values() {
        let kv = parse_kv("\"a\"\n{\n\t\"b\"\t\t\"1\" // note\n\t\"c\"\n\t{\n\t\t\"d\"\t\"x y\"\n\t}\n\t\"MWHEELUP\"\t\"\"//\"invprev\"\n\tplain word\n}\n");
        let a = kv.get("A").expect("any case");
        assert_eq!(a.text("b"), Some("1"));
        assert_eq!(a.get("c").and_then(|c| c.text("d")), Some("x y"));
        assert_eq!(a.text("MWHEELUP"), Some(""), "a comment right after a value");
        assert_eq!(a.text("plain"), Some("word"));
        assert_eq!(a.text("c"), None, "a block isn't a text");
        // Cut off in the middle (the game is writing it): what is whole is kept.
        let cut = parse_kv("\"KeyBindings\"\n{\n\t\"Keys\"\n\t{\n\t\t\"Ability1\"\n\t\t{\n\t\t\t\"Key\"\t\t\"5\"\n\t\t}\n\t\t\"Ability2\"\n\t\t{\n\t\t\t\"Ke");
        assert_eq!(cut.get("KeyBindings").and_then(|k| k.get("Keys")).and_then(|k| k.get("Ability1")).and_then(|a| a.text("Key")), Some("5"));
        assert_eq!(parse_kv(""), Kv::Block(Vec::new()));
    }

    #[test]
    fn valve_key_names() {
        for (valve, ours) in [("q", "Q"), ("1", "1"), ("SPACE", "Space"), ("ENTER", "Enter"), ("ESCAPE", "Escape"), ("TAB", "Tab"), ("BACKSPACE", "Backspace"), ("F7", "F7")] {
            assert_eq!(key_name(valve).as_deref(), Some(ours), "{valve}");
        }
        for (valve, ours) in [("=", "Key187"), ("-", "Key189"), ("UPARROW", "Key38"), ("DOWNARROW", "Key40"), ("SEMICOLON", "Key186"), ("'", "Key222"), ("KP_5", "Num5")] {
            assert_eq!(key_name(valve).as_deref(), Some(ours), "{valve}");
        }
        for unseen in ["MOUSE1", "MOUSE4", "MWHEELUP", "SHIFT", "CTRL", "ALT", "NONE", "", "A_BUTTON", "FOO"] {
            assert_eq!(key_name(unseen), None, "{unseen}");
        }
    }

    #[test]
    fn the_owners_binds() {
        let b = Binds::parse(OWNER);
        assert!(b.from_file);
        for (action, key) in [(Action::Ability(1), "1"), (Action::Ability(4), "4"), (Action::Item(1), "Z"), (Action::Item(4), "V"), (Action::Melee, "Q"), (Action::Parry, "F")] {
            assert_eq!(b.keys(action), [key], "{action:?}");
            assert_eq!(b.action(&kp(key)).map(|(a, _)| a), Some(action));
        }
        assert_eq!(b.keys(Action::Chat), ["Enter", "Shift+Enter"]);
        assert!(b.unseen().is_empty());
        // His file is the defaults (he changed only the console key).
        assert_eq!(Binds::default().actions, b.actions);
        assert_eq!(b.keys(Action::Shop), ["B"]);
        assert!(!Binds::default().from_file);
        assert_eq!(b.summary(), "abilities 1 2 3 4, items Z X C V, melee Q, parry F");

        // Alt+1 upgrades ability 1; Shift / Ctrl held (dashing, crouching) still casts.
        assert_eq!(b.action(&KeyPress { alt: true, ..kp("1") }), None);
        assert_eq!(b.action(&KeyPress { shift: true, ..kp("1") }).map(|(a, _)| a), Some(Action::Ability(1)));
        assert_eq!(b.action(&KeyPress { ctrl: true, ..kp("Z") }).map(|(a, _)| a), Some(Action::Item(1)));
        // Keys of other actions (reload, move, scoreboard) are nothing here.
        for other in ["R", "W", "Tab", "Space", "G", "E", "5", "F7"] {
            assert_eq!(b.action(&kp(other)), None, "{other}");
        }
    }

    #[test]
    fn changed_binds() {
        let text = r#""KeyBindings" { "Keys" {
            "Ability4" { "Key" "R" "Key2" "MOUSE5" }
            "Reload" { "Key" "4" }
            "Item1" { "Key" "1" "Modifier" "SHIFT" }
            "HeldItem" { "Key" "MOUSE4" "Key2" "NONE" }
            "AbilityMelee" { "Key" "NONE" }
            "Ping" { "Key" "X" "Modifier" "CTRL" }
        } }"#;
        let b = Binds::parse(text);
        assert_eq!(b.keys(Action::Ability(4)), ["R", "Mouse5"]);
        assert_eq!(b.action(&kp("R")).map(|(a, _)| a), Some(Action::Ability(4)));
        assert_eq!(b.action(&kp("4")), None, "4 reloads now");
        // A chord of a followed action wins over the plain key.
        assert_eq!(b.action(&KeyPress { shift: true, ..kp("1") }).map(|(a, x)| (a, x.label())), Some((Action::Item(1), "Shift+1".to_string())));
        assert_eq!(b.action(&kp("1")).map(|(a, _)| a), Some(Action::Ability(1)));
        // Somebody else's chord on the key: not the plain key's action.
        assert_eq!(b.action(&KeyPress { ctrl: true, ..kp("X") }), None);
        assert_eq!(b.action(&kp("X")).map(|(a, _)| a), Some(Action::Item(2)));
        assert_eq!(b.keys(Action::Melee), Vec::<String>::new(), "unbound");
        assert_eq!(b.action(&kp("Q")), None);
        assert_eq!(b.action(&kp("F")), None, "parry is on a mouse button");
        assert_eq!(b.unseen(), ["Ability(4) on Mouse5", "Parry on Mouse4"]);
        assert_eq!(b.summary(), "abilities 1 2 3 R/Mouse5, items Shift+1 X C V, melee -, parry Mouse4");
        // What the file doesn't list stays at the defaults.
        assert_eq!(b.keys(Action::Ability(2)), ["2"]);
    }

    #[test]
    fn the_newest_accounts_file() {
        let steam = std::env::temp_dir().join(format!("cv-deadlock-binds-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&steam);
        assert_eq!(load(Some(steam.as_path())), Binds::default(), "no userdata folder");
        assert_eq!(load(None), Binds::default());
        let cfg = steam.join("userdata").join("1456429708").join(paths::APP_ID).join("remote").join("cfg");
        std::fs::create_dir_all(&cfg).unwrap();
        std::fs::create_dir_all(steam.join("userdata").join("77")).unwrap();
        assert_eq!(personal_file(&steam), None);
        std::fs::write(cfg.join("citadelkeys_personal.lst"), OWNER.replace("\"Key\"\t\t\"Q\"", "\"Key\"\t\t\"E\"")).unwrap();
        assert_eq!(personal_file(&steam), Some(cfg.join("citadelkeys_personal.lst")));
        let b = load(Some(steam.as_path()));
        assert!(b.from_file);
        assert_eq!(b.keys(Action::Melee), ["E"]);
        std::fs::remove_dir_all(steam).ok();
    }
}
