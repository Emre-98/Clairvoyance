//! Translates Live Client Data API events into shared timeline events.
//! Pure functions, so they're unit-tested without a running game.

use cv_core::{EventKind, GameEvent};
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize)]
pub struct EventList {
    #[serde(rename = "Events", default)]
    pub events: Vec<RawEvent>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct RawEvent {
    #[serde(rename = "EventID")]
    pub event_id: i64,
    pub event_name: String,
    pub event_time: f64,
    #[serde(default)]
    pub killer_name: Option<String>,
    #[serde(default)]
    pub victim_name: Option<String>,
    #[serde(default)]
    pub assisters: Vec<String>,
    #[serde(default)]
    pub kill_streak: Option<u32>,
    #[serde(default)]
    pub recipient: Option<String>,
    #[serde(default)]
    pub acer: Option<String>,
    #[serde(default)]
    pub acing_team: Option<String>,
    #[serde(default)]
    pub dragon_type: Option<String>,
    /// "True" / "False" as a string in the API.
    #[serde(default)]
    pub stolen: Option<serde_json::Value>,
    #[serde(default)]
    pub result: Option<String>,
    #[serde(default)]
    pub turret_killed: Option<String>,
    #[serde(default)]
    pub inhib_killed: Option<String>,
}

impl RawEvent {
    fn is_stolen(&self) -> bool {
        match &self.stolen {
            Some(serde_json::Value::String(s)) => s.eq_ignore_ascii_case("true"),
            Some(serde_json::Value::Bool(b)) => *b,
            _ => false,
        }
    }
}

/// Who "I" am in this match.
#[derive(Debug, Clone, Default)]
pub struct Ctx {
    /// Lower-case names that identify the player (Riot ID with and without tag).
    pub me: Vec<String>,
    /// "ORDER" or "CHAOS".
    pub team: Option<String>,
    /// Lower-case player name (with and without tag) -> champion display name.
    pub champions: HashMap<String, String>,
    /// Lower-case player name -> champion id ("MonkeyKing"), for portraits.
    pub ids: HashMap<String, String>,
}

impl Ctx {
    pub fn is_me(&self, name: &str) -> bool {
        let n = name.trim().to_lowercase();
        !n.is_empty() && self.me.contains(&n)
    }

    pub fn is_me_any(&self, names: &[String]) -> bool {
        names.iter().any(|n| self.is_me(n))
    }

    /// Champion ids of the named players (unknown names and non-players skipped).
    pub fn ids_of<'a>(&self, names: impl IntoIterator<Item = &'a str>) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        for n in names {
            if let Some(id) = self.ids.get(&n.trim().to_lowercase()) {
                if !v.contains(id) {
                    v.push(id.clone());
                }
            }
        }
        v
    }

    /// Champion name for a player, or a readable name for towers/minions/monsters.
    pub fn who(&self, name: &str) -> String {
        if let Some(c) = self.champions.get(&name.trim().to_lowercase()) {
            return c.clone();
        }
        let lower = name.to_lowercase();
        if lower.starts_with("turret") {
            "a tower".into()
        } else if lower.contains("minion") {
            "minions".into()
        } else if lower.contains("baron") {
            "Baron".into()
        } else if lower.contains("dragon") {
            "a dragon".into()
        } else if lower.starts_with("sru_") || lower.starts_with("ha_") {
            "a monster".into()
        } else if name.is_empty() {
            "unknown".into()
        } else {
            // Players not in the list (shouldn't happen): show the name without the tag.
            name.split('#').next().unwrap_or(name).to_string()
        }
    }
}

pub fn dragon_name(t: Option<&str>) -> String {
    match t.unwrap_or("").to_ascii_lowercase().as_str() {
        "fire" => "Infernal Drake".into(),
        "earth" => "Mountain Drake".into(),
        "water" => "Ocean Drake".into(),
        "air" => "Cloud Drake".into(),
        "hextech" => "Hextech Drake".into(),
        "chemtech" => "Chemtech Drake".into(),
        "elder" => "Elder Dragon".into(),
        "" => "Dragon".into(),
        other => {
            let mut c = other.chars();
            let cap = c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default();
            format!("{cap} Drake")
        }
    }
}

/// A structure from its API name: lane ("Top" / "Mid" / "Bot"), tier ("Outer" ...) and side.
/// Turrets: `Turret_T2_C_05_A` = red side (T2), mid (C), outer (05). Summoner's Rift numbers:
/// top / bot 03 outer, 02 inner, 01 inhibitor turret; mid 05 outer, 04 inner, 03 inhibitor
/// turret, 02 / 01 nexus turrets. Inhibitors: `Barracks_T1_R1`. Other maps: lane only.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Structure {
    pub lane: Option<&'static str>,
    pub tier: Option<&'static str>,
    pub side: Option<&'static str>,
}

pub fn structure(name: &str) -> Structure {
    let parts: Vec<&str> = name.split('_').collect();
    let side = parts.get(1).and_then(|t| match *t {
        "T1" => Some("Blue"),
        "T2" => Some("Red"),
        _ => None,
    });
    let lane_of = |c: char| match c {
        'L' => Some("Top"),
        'C' => Some("Mid"),
        'R' => Some("Bot"),
        _ => None,
    };
    match parts.first().map(|p| p.to_ascii_lowercase()).as_deref() {
        Some("turret") => {
            let lane = parts.get(2).and_then(|l| l.chars().next()).and_then(lane_of);
            let n: Option<u32> = parts.get(3).and_then(|n| n.parse().ok());
            let tier = match (lane, n) {
                (Some("Mid"), Some(5)) | (Some("Top" | "Bot"), Some(3)) => Some("Outer"),
                (Some("Mid"), Some(4)) | (Some("Top" | "Bot"), Some(2)) => Some("Inner"),
                (Some("Mid"), Some(3)) | (Some("Top" | "Bot"), Some(1)) => Some("Inhibitor"),
                (Some("Mid"), Some(1 | 2)) => Some("Nexus"),
                _ => None,
            };
            // Two mid "lane" nexus turrets stand in the base, not in mid.
            let lane = if tier == Some("Nexus") { None } else { lane };
            Structure { lane, tier, side }
        }
        Some("barracks") => Structure { lane: parts.get(2).and_then(|l| l.chars().next()).and_then(lane_of), tier: None, side },
        _ => Structure::default(),
    }
}

/// "the mid outer tower", "a nexus tower", "a tower".
fn tower_phrase(s: &Structure) -> String {
    match (s.lane, s.tier) {
        (_, Some("Nexus")) => "a nexus tower".into(),
        (Some(l), Some(t)) => format!("the {} {} tower", l.to_lowercase(), t.to_lowercase()),
        (Some(l), None) => format!("a {} tower", l.to_lowercase()),
        _ => "a tower".into(),
    }
}

fn multikill_name(n: u32) -> &'static str {
    match n {
        2 => "Double kill",
        3 => "Triple kill",
        4 => "Quadra kill",
        n if n >= 5 => "Penta kill",
        _ => "Multikill",
    }
}

fn assist_details(e: &RawEvent, ctx: &Ctx) -> String {
    if e.assisters.is_empty() {
        String::new()
    } else {
        format!("Assists: {}", e.assisters.iter().map(|a| ctx.who(a)).collect::<Vec<_>>().join(", "))
    }
}

fn id(e: &RawEvent, suffix: &str) -> String {
    format!("lol-{}{}", e.event_id, suffix)
}

/// Returns the timeline events for one API event (usually 0 or 1).
pub fn translate(e: &RawEvent, ctx: &Ctx) -> Vec<GameEvent> {
    let t = e.event_time;
    let killer = e.killer_name.as_deref().unwrap_or("");
    let i_killed = ctx.is_me(killer);
    let i_helped = ctx.is_me_any(&e.assisters);
    let mut out = Vec::new();
    match e.event_name.as_str() {
        "GameStart" => out.push(GameEvent::new(id(e, ""), EventKind::GameStart, t, "Game start")),
        "GameEnd" => {
            let title = match e.result.as_deref() {
                Some("Win") => "Victory",
                Some("Lose") => "Defeat",
                _ => "Game end",
            };
            out.push(GameEvent::new(id(e, ""), EventKind::GameEnd, t, title));
        }
        "ChampionKill" => {
            let victim = e.victim_name.as_deref().unwrap_or("");
            if i_killed {
                out.push(
                    GameEvent::new(id(e, "-k"), EventKind::Kill, t, format!("Killed {}", ctx.who(victim)))
                        .with_details(assist_details(e, ctx))
                        .with_who(ctx.ids_of([victim])),
                );
            } else if ctx.is_me(victim) {
                let d = assist_details(e, ctx);
                out.push(GameEvent::new(id(e, "-d"), EventKind::Death, t, format!("Killed by {}", ctx.who(killer))).with_details(d).with_who(ctx.ids_of([killer])));
            } else if i_helped {
                out.push(
                    GameEvent::new(id(e, "-a"), EventKind::Assist, t, format!("Assist on {}", ctx.who(victim)))
                        .with_details(format!("Killed by {}", ctx.who(killer)))
                        .with_who(ctx.ids_of([victim, killer])),
                );
            }
        }
        "Multikill" if i_killed => {
            let n = e.kill_streak.unwrap_or(2);
            out.push(GameEvent::new(id(e, ""), EventKind::Multikill, t, multikill_name(n)));
        }
        "FirstBlood" if e.recipient.as_deref().is_some_and(|r| ctx.is_me(r)) => {
            out.push(GameEvent::new(id(e, ""), EventKind::FirstBlood, t, "First blood"));
        }
        "Ace" => {
            let acer = e.acer.as_deref().unwrap_or("");
            if ctx.is_me(acer) {
                out.push(GameEvent::new(id(e, ""), EventKind::Ace, t, "Ace"));
            } else if ctx.team.is_some() && e.acing_team.as_deref() == ctx.team.as_deref() {
                out.push(GameEvent::new(id(e, ""), EventKind::Ace, t, "Team ace").with_details(format!("Final kill by {}", ctx.who(acer))));
            }
        }
        "TurretKilled" | "FirstBrick" if i_killed || i_helped => {
            // FirstBrick duplicates the TurretKilled event; keep only the turret one.
            if e.event_name == "TurretKilled" {
                let st = structure(e.turret_killed.as_deref().unwrap_or(""));
                let what = tower_phrase(&st);
                let title = if i_killed { format!("Destroyed {what}") } else { format!("Helped destroy {what}") };
                out.push(
                    structure_event(e, ctx, EventKind::Tower, title, &st, i_killed)
                        .fact("Tower", st.tier.map(|t| if t == "Inhibitor" { "Inhibitor turret".to_string() } else { format!("{t} turret") }).unwrap_or_default()),
                );
            }
        }
        "InhibKilled" if i_killed || i_helped => {
            let st = structure(e.inhib_killed.as_deref().unwrap_or(""));
            let what = st.lane.map(|l| format!("the {} inhibitor", l.to_lowercase())).unwrap_or_else(|| "an inhibitor".into());
            let title = if i_killed { format!("Destroyed {what}") } else { format!("Helped destroy {what}") };
            out.push(structure_event(e, ctx, EventKind::Inhibitor, title, &st, i_killed));
        }
        "DragonKill" if i_killed || i_helped => {
            let name = dragon_name(e.dragon_type.as_deref());
            out.push(objective(e, ctx, EventKind::Dragon, &name, i_killed));
        }
        "HeraldKill" if i_killed || i_helped => out.push(objective(e, ctx, EventKind::Herald, "Rift Herald", i_killed)),
        "BaronKill" if i_killed || i_helped => out.push(objective(e, ctx, EventKind::Baron, "Baron Nashor", i_killed)),
        "HordeKill" if i_killed || i_helped => out.push(objective(e, ctx, EventKind::Objective, "Voidgrub", i_killed)),
        "AtakhanKill" if i_killed || i_helped => out.push(objective(e, ctx, EventKind::Objective, "Atakhan", i_killed)),
        other if other.ends_with("Kill") && other != "ChampionKill" && (i_killed || i_helped) => {
            // Objectives added to the game later: still show them.
            let name = other.trim_end_matches("Kill");
            out.push(objective(e, ctx, EventKind::Objective, name, i_killed));
        }
        _ => {}
    }
    out
}

/// Who was there: the last hit (a champion, or minions) first, then the assists.
fn involved(e: &RawEvent, ctx: &Ctx) -> Vec<String> {
    let killer = e.killer_name.as_deref().unwrap_or("");
    ctx.ids_of(std::iter::once(killer).chain(e.assisters.iter().map(|a| a.as_str())))
}

fn structure_event(e: &RawEvent, ctx: &Ctx, kind: EventKind, title: String, st: &Structure, i_killed: bool) -> GameEvent {
    let killer = e.killer_name.as_deref().unwrap_or("");
    GameEvent::new(id(e, ""), kind, e.event_time, title)
        .with_details(assist_details(e, ctx))
        .fact("Lane", st.lane.unwrap_or_default())
        .fact("Side", st.side.map(|s| format!("{s} side")).unwrap_or_default())
        .fact("Last hit", if i_killed { "You".to_string() } else { ctx.who(killer) })
        .with_who(involved(e, ctx))
}

fn objective(e: &RawEvent, ctx: &Ctx, kind: EventKind, name: &str, i_killed: bool) -> GameEvent {
    let steal = e.is_stolen();
    let verb = if steal {
        "Stole"
    } else if i_killed {
        "Took"
    } else {
        "Helped take"
    };
    let killer = e.killer_name.as_deref().unwrap_or("");
    GameEvent::new(id(e, ""), kind, e.event_time, format!("{verb} {name}"))
        .with_details(assist_details(e, ctx))
        .fact("Last hit", if i_killed { "You".to_string() } else { ctx.who(killer) })
        .fact("Stolen", if steal { "Yes, from the enemy team".to_string() } else { String::new() })
        .with_who(involved(e, ctx))
        .stolen(steal)
}

pub fn mode_name(game_mode: &str) -> String {
    match game_mode {
        "CLASSIC" => "Summoner's Rift".into(),
        "ARAM" => "ARAM".into(),
        "CHERRY" => "Arena".into(),
        "URF" | "ARURF" => "URF".into(),
        "PRACTICETOOL" => "Practice Tool".into(),
        "TUTORIAL" | "TUTORIAL_MODULE_1" | "TUTORIAL_MODULE_2" | "TUTORIAL_MODULE_3" => "Tutorial".into(),
        "NEXUSBLITZ" => "Nexus Blitz".into(),
        "ONEFORALL" => "One for All".into(),
        "SWIFTPLAY" => "Swiftplay".into(),
        "STRAWBERRY" => "Swarm".into(),
        other => {
            let mut c = other.to_lowercase().chars().collect::<Vec<_>>();
            if let Some(f) = c.first_mut() {
                *f = f.to_ascii_uppercase();
            }
            c.into_iter().collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> Ctx {
        let mut champions = HashMap::new();
        for (n, c) in [("me#euw", "Ahri"), ("me", "Ahri"), ("enemy#na1", "Zed"), ("enemy", "Zed"), ("mate", "Lee Sin")] {
            champions.insert(n.to_string(), c.to_string());
        }
        let ids = champions.iter().map(|(k, v)| (k.clone(), v.replace(' ', ""))).collect();
        Ctx { me: vec!["me#euw".into(), "me".into()], team: Some("ORDER".into()), champions, ids }
    }

    fn parse(json: &str) -> Vec<RawEvent> {
        serde_json::from_str::<EventList>(json).unwrap().events
    }

    #[test]
    fn kills_deaths_assists() {
        let evs = parse(
            r#"{"Events":[
            {"EventID":0,"EventName":"GameStart","EventTime":0.05},
            {"EventID":1,"EventName":"ChampionKill","EventTime":300.5,"KillerName":"Me","VictimName":"Enemy","Assisters":["Mate"]},
            {"EventID":2,"EventName":"ChampionKill","EventTime":400.0,"KillerName":"Enemy#NA1","VictimName":"Me#EUW","Assisters":[]},
            {"EventID":3,"EventName":"ChampionKill","EventTime":500.0,"KillerName":"Mate","VictimName":"Enemy","Assisters":["Me"]},
            {"EventID":4,"EventName":"ChampionKill","EventTime":510.0,"KillerName":"Turret_T2_L_03_A","VictimName":"Me","Assisters":[]},
            {"EventID":5,"EventName":"ChampionKill","EventTime":520.0,"KillerName":"Enemy","VictimName":"Mate","Assisters":[]},
            {"EventID":6,"EventName":"Multikill","EventTime":301.0,"KillerName":"Me","KillStreak":3},
            {"EventID":7,"EventName":"FirstBlood","EventTime":300.5,"Recipient":"Me"}
        ]}"#,
        );
        let c = ctx();
        let out: Vec<GameEvent> = evs.iter().flat_map(|e| translate(e, &c)).collect();
        let kinds: Vec<_> = out.iter().map(|e| e.kind).collect();
        assert_eq!(kinds, vec![EventKind::GameStart, EventKind::Kill, EventKind::Death, EventKind::Assist, EventKind::Death, EventKind::Multikill, EventKind::FirstBlood]);
        assert_eq!(out[1].title, "Killed Zed");
        assert_eq!(out[1].details.as_deref(), Some("Assists: Lee Sin"));
        assert_eq!(out[2].title, "Killed by Zed");
        assert_eq!(out[3].title, "Assist on Zed");
        assert_eq!(out[4].title, "Killed by a tower");
        assert_eq!(out[5].title, "Triple kill");
    }

    #[test]
    fn objectives_and_steals() {
        let evs = parse(
            r#"{"Events":[
            {"EventID":10,"EventName":"DragonKill","EventTime":600,"DragonType":"Fire","Stolen":"False","KillerName":"Mate","Assisters":["Me"]},
            {"EventID":11,"EventName":"BaronKill","EventTime":1500,"Stolen":"True","KillerName":"Me","Assisters":[]},
            {"EventID":12,"EventName":"HeraldKill","EventTime":800,"Stolen":"False","KillerName":"Enemy","Assisters":[]},
            {"EventID":13,"EventName":"TurretKilled","EventTime":900,"TurretKilled":"Turret_T2_L_03_A","KillerName":"Me","Assisters":[]},
            {"EventID":14,"EventName":"FirstBrick","EventTime":900,"KillerName":"Me"},
            {"EventID":15,"EventName":"InhibKilled","EventTime":1600,"InhibKilled":"Barracks_T2_L1","KillerName":"Minion_T1L1S04N0005","Assisters":["Me"]},
            {"EventID":16,"EventName":"HordeKill","EventTime":400,"KillerName":"Me","Assisters":[],"Stolen":"False"},
            {"EventID":17,"EventName":"Ace","EventTime":1700,"Acer":"Mate","AcingTeam":"ORDER"},
            {"EventID":18,"EventName":"GameEnd","EventTime":1800,"Result":"Win"}
        ]}"#,
        );
        let c = ctx();
        let out: Vec<GameEvent> = evs.iter().flat_map(|e| translate(e, &c)).collect();
        let titles: Vec<_> = out.iter().map(|e| e.title.as_str()).collect();
        assert_eq!(titles, vec!["Helped take Infernal Drake", "Stole Baron Nashor", "Destroyed the top outer tower", "Helped destroy the top inhibitor", "Took Voidgrub", "Team ace", "Victory"]);
        let f = |i: usize, k: &str| out[i].facts.iter().find(|(l, _)| l == k).map(|(_, v)| v.clone());
        assert_eq!(f(2, "Lane").as_deref(), Some("Top"));
        assert_eq!(f(2, "Tower").as_deref(), Some("Outer turret"));
        assert_eq!(f(2, "Side").as_deref(), Some("Red side"));
        assert_eq!(f(2, "Last hit").as_deref(), Some("You"));
        assert_eq!(f(3, "Last hit").as_deref(), Some("minions"));
        assert_eq!(f(0, "Last hit").as_deref(), Some("Lee Sin"));
        assert_eq!(out[0].who, vec!["LeeSin".to_string(), "Ahri".to_string()], "last hit first, then the assists");
        assert_eq!(f(1, "Stolen").as_deref(), Some("Yes, from the enemy team"));
        assert!(out[1].steal);
        assert_eq!(out[1].kind, EventKind::Baron);
        assert!(!out[0].steal);
    }

    #[test]
    fn structures() {
        let s = |n: &str| {
            let s = structure(n);
            (s.lane, s.tier, s.side)
        };
        assert_eq!(s("Turret_T2_C_05_A"), (Some("Mid"), Some("Outer"), Some("Red")));
        assert_eq!(s("Turret_T1_L_02_A"), (Some("Top"), Some("Inner"), Some("Blue")));
        assert_eq!(s("Turret_T1_R_01_A"), (Some("Bot"), Some("Inhibitor"), Some("Blue")));
        assert_eq!(s("Turret_T2_C_03_A"), (Some("Mid"), Some("Inhibitor"), Some("Red")));
        assert_eq!(s("Turret_T2_C_01_A"), (None, Some("Nexus"), Some("Red")));
        assert_eq!(s("Barracks_T2_R1"), (Some("Bot"), None, Some("Red")));
        assert_eq!(s("Turret_T1_C_08_A"), (Some("Mid"), None, Some("Blue")), "ARAM numbers: lane only");
        assert_eq!(s("Something"), (None, None, None));
        assert_eq!(tower_phrase(&structure("Turret_T2_C_02_A")), "a nexus tower");
        assert_eq!(tower_phrase(&structure("Turret_T2_C_05_A")), "the mid outer tower");
        assert_eq!(tower_phrase(&structure("x")), "a tower");
    }

    #[test]
    fn names() {
        assert_eq!(dragon_name(Some("Elder")), "Elder Dragon");
        assert_eq!(dragon_name(Some("mystery")), "Mystery Drake");
        assert_eq!(mode_name("CLASSIC"), "Summoner's Rift");
        assert_eq!(mode_name("NEWMODE"), "Newmode");
    }
}
