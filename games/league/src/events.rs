//! Translates Live Client Data API events into shared timeline events.
//! Pure functions, so they're unit-tested without a running game.

use gr_core::{EventKind, GameEvent};
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
}

impl Ctx {
    pub fn is_me(&self, name: &str) -> bool {
        let n = name.trim().to_lowercase();
        !n.is_empty() && self.me.iter().any(|m| *m == n)
    }

    pub fn is_me_any(&self, names: &[String]) -> bool {
        names.iter().any(|n| self.is_me(n))
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
                out.push(GameEvent::new(id(e, "-k"), EventKind::Kill, t, format!("Killed {}", ctx.who(victim))).with_details(assist_details(e, ctx)));
            } else if ctx.is_me(victim) {
                let d = assist_details(e, ctx);
                out.push(GameEvent::new(id(e, "-d"), EventKind::Death, t, format!("Killed by {}", ctx.who(killer))).with_details(d));
            } else if i_helped {
                out.push(
                    GameEvent::new(id(e, "-a"), EventKind::Assist, t, format!("Assist on {}", ctx.who(victim)))
                        .with_details(format!("Killed by {}", ctx.who(killer))),
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
                let title = if i_killed { "Destroyed a tower" } else { "Helped destroy a tower" };
                out.push(GameEvent::new(id(e, ""), EventKind::Tower, t, title).with_details(assist_details(e, ctx)));
            }
        }
        "InhibKilled" if i_killed || i_helped => {
            let title = if i_killed { "Destroyed an inhibitor" } else { "Helped destroy an inhibitor" };
            out.push(GameEvent::new(id(e, ""), EventKind::Inhibitor, t, title).with_details(assist_details(e, ctx)));
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

fn objective(e: &RawEvent, ctx: &Ctx, kind: EventKind, name: &str, i_killed: bool) -> GameEvent {
    let steal = e.is_stolen();
    let verb = if steal { "Stole" } else if i_killed { "Took" } else { "Helped take" };
    GameEvent::new(id(e, ""), kind, e.event_time, format!("{verb} {name}"))
        .with_details(assist_details(e, ctx))
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
        Ctx { me: vec!["me#euw".into(), "me".into()], team: Some("ORDER".into()), champions }
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
        assert_eq!(
            kinds,
            vec![EventKind::GameStart, EventKind::Kill, EventKind::Death, EventKind::Assist, EventKind::Death, EventKind::Multikill, EventKind::FirstBlood]
        );
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
        assert_eq!(
            titles,
            vec!["Helped take Infernal Drake", "Stole Baron Nashor", "Destroyed a tower", "Helped destroy an inhibitor", "Took Voidgrub", "Team ace", "Victory"]
        );
        assert!(out[1].steal);
        assert_eq!(out[1].kind, EventKind::Baron);
        assert!(!out[0].steal);
    }

    #[test]
    fn names() {
        assert_eq!(dragon_name(Some("Elder")), "Elder Dragon");
        assert_eq!(dragon_name(Some("mystery")), "Mystery Drake");
        assert_eq!(mode_name("CLASSIC"), "Summoner's Rift");
        assert_eq!(mode_name("NEWMODE"), "Newmode");
    }
}
