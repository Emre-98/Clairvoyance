//! A fake League Live Client Data API (plain HTTP) that plays a scripted match.
//! Used by "Settings > Advanced > Simulate a League game" and by tests, so the whole
//! pipeline (detection, recording, events, offset, timeline) can be tried without playing.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct MockOptions {
    pub port: u16,
    /// Game seconds per real second.
    pub speed: f64,
    /// Match length in game seconds.
    pub length: f64,
    /// Real seconds of "loading screen" (API not answering) before the clock starts.
    pub loading_secs: f64,
    /// Real seconds the "game process" stays open after the match ends.
    pub linger_secs: f64,
    pub player: String,
    pub champion: String,
}

impl Default for MockOptions {
    fn default() -> Self {
        Self {
            port: 2998,
            speed: 4.0,
            length: 600.0,
            loading_secs: 8.0,
            linger_secs: 8.0,
            player: "Tester#EUW".into(),
            champion: "Ahri".into(),
        }
    }
}

pub struct MockHandle {
    /// True while the fake "League of Legends.exe" is running.
    pub running: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    pub base_url: String,
}

impl MockHandle {
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
        self.running.store(false, Ordering::SeqCst);
    }
}

struct Script {
    opts: MockOptions,
    start: Instant,
}

const ALLIES: [(&str, &str, &str); 4] = [("Mate1#EUW", "Lee Sin", "LeeSin"), ("Mate2#EUW", "Jinx", "Jinx"), ("Mate3#EUW", "Thresh", "Thresh"), ("Mate4#EUW", "Garen", "Garen")];
const ENEMIES: [(&str, &str, &str); 5] =
    [("Enemy1#NA1", "Zed", "Zed"), ("Enemy2#NA1", "Vi", "Vi"), ("Enemy3#NA1", "Caitlyn", "Caitlyn"), ("Enemy4#NA1", "Lux", "Lux"), ("Enemy5#NA1", "Darius", "Darius")];

impl Script {
    fn game_time(&self) -> Option<f64> {
        let real = self.start.elapsed().as_secs_f64();
        if real < self.opts.loading_secs {
            return None;
        }
        Some(((real - self.opts.loading_secs) * self.opts.speed).min(self.opts.length))
    }

    fn me(&self) -> String {
        self.opts.player.clone()
    }

    fn me_short(&self) -> String {
        self.opts.player.split('#').next().unwrap_or("").to_string()
    }

    /// All events up to game time `t` (fractions of the match length).
    fn events(&self, t: f64) -> Vec<Value> {
        let l = self.opts.length;
        let me = self.me_short();
        let e = |n: usize| ENEMIES[n].0.split('#').next().unwrap().to_string();
        let a = |n: usize| ALLIES[n].0.split('#').next().unwrap().to_string();
        let script: Vec<(f64, Value)> = vec![
            (0.0, json!({"EventName":"GameStart"})),
            (0.10, json!({"EventName":"MinionsSpawning"})),
            (0.18, json!({"EventName":"ChampionKill","KillerName":me,"VictimName":e(0),"Assisters":[a(0)]})),
            (0.18, json!({"EventName":"FirstBlood","Recipient":me})),
            (0.22, json!({"EventName":"HordeKill","KillerName":me,"Assisters":[],"Stolen":"False"})),
            (0.27, json!({"EventName":"ChampionKill","KillerName":e(1),"VictimName":me,"Assisters":[e(0)]})),
            (0.33, json!({"EventName":"DragonKill","DragonType":"Fire","Stolen":"False","KillerName":a(0),"Assisters":[me]})),
            (0.38, json!({"EventName":"ChampionKill","KillerName":a(1),"VictimName":e(2),"Assisters":[me]})),
            (0.42, json!({"EventName":"TurretKilled","TurretKilled":"Turret_T2_C_05_A","KillerName":me,"Assisters":[]})),
            (0.47, json!({"EventName":"HeraldKill","Stolen":"True","KillerName":me,"Assisters":[]})),
            (0.55, json!({"EventName":"ChampionKill","KillerName":me,"VictimName":e(3),"Assisters":[]})),
            (0.553, json!({"EventName":"ChampionKill","KillerName":me,"VictimName":e(4),"Assisters":[]})),
            (0.556, json!({"EventName":"ChampionKill","KillerName":me,"VictimName":e(1),"Assisters":[a(2)]})),
            (0.556, json!({"EventName":"Multikill","KillerName":me,"KillStreak":3})),
            (0.62, json!({"EventName":"ChampionKill","KillerName":"Turret_T2_L_03_A","VictimName":me,"Assisters":[]})),
            (0.70, json!({"EventName":"BaronKill","Stolen":"False","KillerName":a(0),"Assisters":[me, a(1)]})),
            (0.78, json!({"EventName":"InhibKilled","InhibKilled":"Barracks_T2_C1","KillerName":me,"Assisters":[a(1)]})),
            (0.85, json!({"EventName":"ChampionKill","KillerName":a(3),"VictimName":e(0),"Assisters":[me]})),
            (0.86, json!({"EventName":"Ace","Acer":a(3),"AcingTeam":"ORDER"})),
            (1.0, json!({"EventName":"GameEnd","Result":"Win"})),
        ];
        script
            .into_iter()
            .enumerate()
            .filter(|(_, (f, _))| f * l <= t)
            .map(|(i, (f, mut v))| {
                v["EventID"] = json!(i);
                v["EventTime"] = json!(if f == 0.0 { 0.05 } else { f * l });
                v
            })
            .collect()
    }

    fn scores_at(&self, t: f64) -> (u32, u32, u32) {
        let evs = self.events(t);
        let me = self.me_short();
        let mut k = (0, 0, 0);
        for e in evs.iter().filter(|e| e["EventName"] == "ChampionKill") {
            if e["KillerName"] == me.as_str() {
                k.0 += 1;
            } else if e["VictimName"] == me.as_str() {
                k.1 += 1;
            } else if e["Assisters"].as_array().is_some_and(|a| a.iter().any(|x| x == me.as_str())) {
                k.2 += 1;
            }
        }
        k
    }

    fn player_list(&self, t: f64) -> Value {
        let (k, d, a) = self.scores_at(t);
        let cs = (t / 60.0 * 7.5) as u32;
        let mut list = vec![json!({
            "riotId": self.me(), "riotIdGameName": self.me_short(), "summonerName": self.me_short(),
            "championName": self.opts.champion, "rawChampionName": format!("game_character_displayname_{}", self.opts.champion.replace(' ', "")),
            "team": "ORDER", "level": (1.0 + t / 90.0).min(18.0) as u32, "isBot": false,
            "scores": {"kills": k, "deaths": d, "assists": a, "creepScore": cs, "wardScore": t / 60.0 * 0.9},
            "items": [{"price": 1100, "count": 1}, {"price": 300, "count": 1}]
        })];
        for (team, players) in [("ORDER", &ALLIES[..]), ("CHAOS", &ENEMIES[..])] {
            for (riot, champ, raw) in players {
                list.push(json!({
                    "riotId": riot, "riotIdGameName": riot.split('#').next().unwrap(), "summonerName": riot.split('#').next().unwrap(),
                    "championName": champ, "rawChampionName": format!("game_character_displayname_{raw}"), "team": team, "level": 6,
                    "scores": {"kills": 1, "deaths": 1, "assists": 1, "creepScore": 50, "wardScore": 3.0}, "items": []
                }));
            }
        }
        Value::Array(list)
    }

    fn respond(&self, path: &str) -> Option<Value> {
        let t = self.game_time()?;
        let path = path.split('?').next().unwrap_or(path);
        Some(match path.trim_start_matches("/liveclientdata/") {
            "gamestats" => json!({"gameMode": "CLASSIC", "gameTime": t, "mapName": "Map11", "mapNumber": 11, "mapTerrain": "Default"}),
            "activeplayername" => json!(self.me()),
            "activeplayer" => json!({"currentGold": 250.0 + t * 1.2, "level": 10, "riotId": self.me(), "summonerName": self.me_short()}),
            "playerlist" => self.player_list(t),
            "eventdata" => json!({"Events": self.events(t)}),
            "allgamedata" => json!({"gameData": {"gameTime": t, "gameMode": "CLASSIC"}, "events": {"Events": self.events(t)}}),
            _ => return None,
        })
    }
}

fn handle(mut stream: TcpStream, script: &Script) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    });
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            return;
        }
        let path = line.split_whitespace().nth(1).unwrap_or("/").to_string();
        // Skip headers.
        loop {
            let mut h = String::new();
            if reader.read_line(&mut h).unwrap_or(0) == 0 || h == "\r\n" || h == "\n" {
                break;
            }
        }
        let (status, body) = match script.respond(&path) {
            Some(v) => ("200 OK", v.to_string()),
            None => ("404 Not Found", r#"{"errorCode":"RESOURCE_NOT_FOUND","httpStatus":404}"#.to_string()),
        };
        let resp = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n{body}", body.len());
        if stream.write_all(resp.as_bytes()).is_err() {
            return;
        }
    }
}

/// Starts the fake game. The "process" runs from now until the match ends + linger time.
pub fn spawn(opts: MockOptions) -> std::io::Result<MockHandle> {
    let listener = TcpListener::bind(("127.0.0.1", opts.port))?;
    listener.set_nonblocking(true)?;
    let running = Arc::new(AtomicBool::new(true));
    let stop = Arc::new(AtomicBool::new(false));
    let base_url = format!("http://127.0.0.1:{}/liveclientdata", opts.port);
    let script = Arc::new(Script { opts: opts.clone(), start: Instant::now() });
    let total_real = opts.loading_secs + opts.length / opts.speed.max(0.01) + opts.linger_secs;
    let (r, s) = (running.clone(), stop.clone());
    std::thread::Builder::new().name("mock-league".into()).spawn(move || {
        while !s.load(Ordering::SeqCst) && script.start.elapsed().as_secs_f64() < total_real {
            match listener.accept() {
                Ok((stream, _)) => {
                    let _ = stream.set_nonblocking(false);
                    let sc = script.clone();
                    std::thread::spawn(move || handle(stream, &sc));
                }
                Err(_) => std::thread::sleep(Duration::from_millis(50)),
            }
        }
        r.store(false, Ordering::SeqCst);
    })?;
    Ok(MockHandle { running, stop, base_url })
}
