// Fake backend for `npm run dev` in a normal browser (UI work and screenshots only).
// Never included in the production build.
import { synthetic } from "./inputoverlay";
import { syntheticActions } from "./bubbles";
import type { ClipEntry, GameEvent, GameSession, LiveStatus, Scoreboard, SessionSummary, Settings } from "./types";

const listeners: Record<string, ((p: any) => void)[]> = {};
export async function listen(name: string, cb: (p: any) => void) {
  (listeners[name] ??= []).push(cb);
  return () => {};
}
function emit(name: string, payload: any) {
  (listeners[name] ?? []).forEach((cb) => cb(payload));
}
export function fileSrc(p: string) {
  return p.startsWith("/dev-assets") ? p : "";
}

const q = new URLSearchParams(location.search);
// ?video=sample169 plays /dev-assets/sample169.webm in every game (UI tests); its frame times
// come from /dev-assets/<name>.frames.json (made by tests/make-sample.py).
const VIDEO = `/dev-assets/${q.get("video") ?? "sample"}.webm`;
const champs: [string, string][] = [
  ["Ahri", "Ahri"],
  ["Lee Sin", "LeeSin"],
  ["Jinx", "Jinx"],
  ["Thresh", "Thresh"],
  ["Kai'Sa", "Kaisa"],
  ["Wukong", "MonkeyKing"],
  ["Lux", "Lux"],
  ["Darius", "Darius"],
  ["Ezreal", "Ezreal"],
];
const modes = ["Ranked Solo", "Summoner's Rift", "ARAM", "Summoner's Rift", "Arena"];

function ev(id: string, kind: GameEvent["kind"], t: number, title: string, details?: string, steal = false): GameEvent {
  return { id, kind, game_time: t, title, details, steal };
}

const detailEvents: GameEvent[] = [
  ev("0", "game_start", 0.05, "Game start"),
  ev("1", "kill", 14, "Killed Zed", "Assists: Lee Sin"),
  ev("2", "first_blood", 14, "First blood"),
  ev("u1", "ult_pressed", 12.5, "Ult pressed", "Key press. The game can't confirm the ult was cast."),
  ev("u3", "ult_unconfirmed", 18.3, "Ult pressed, no cast", "R pressed. The ult was on cooldown."),
  ev("3", "objective", 22, "Took Voidgrub"),
  ev("4", "death", 31, "Killed by Vi", "Assists: Zed"),
  ev("5", "dragon", 44, "Helped take Infernal Drake", "Assists: Ahri"),
  ev("6", "assist", 52, "Assist on Caitlyn", "Killed by Jinx"),
  {
    ...ev("7", "tower", 61, "Destroyed the mid outer tower"),
    facts: [["Lane", "Mid"], ["Side", "Red side"], ["Last hit", "You"], ["Tower", "Outer turret"], ["Your gold", "≈ +250"]],
    who: ["Ahri", "LeeSin"],
  },
  {
    ...ev("i1", "item_completed", 40.2, "Completed Luden's Echo"),
    icon: { kind: "item", id: "6655", version: "16.20.1" },
    facts: [["Cost", "2750 gold"], ["Item", "1st finished item"], ["Built from", "Lost Chapter, Blasting Wand"]],
  },
  {
    ...ev("s1", "summoner_spell", 30.4, "Flash", "Confirmed from the recording (the spell went on cooldown)."),
    icon: { kind: "spell", id: "SummonerFlash", version: "16.20.1" },
    facts: [["Slot", "D"], ["Key", "D"]],
  },
  {
    ...ev("s2", "summoner_spell", 79.4, "Ignite", "Confirmed from the recording (the spell went on cooldown)."),
    icon: { kind: "spell", id: "SummonerDot", version: "16.20.1" },
    facts: [["Slot", "F"], ["Key", "F"]],
  },
  ev("8", "herald", 68, "Stole Rift Herald", "", true),
  ev("u2", "ult_pressed", 77, "Ult pressed", "Key press. The game can't confirm the ult was cast."),
  ev("u4", "ult_recast", 78.6, "Ult recast", "R pressed again during the same ult (command, second part or early end): not a new ult."),
  ev("9", "kill", 78, "Killed Lux"),
  ev("10", "kill", 79, "Killed Darius"),
  ev("11", "kill", 80, "Killed Vi", "Assists: Thresh"),
  ev("12", "multikill", 80, "Triple kill"),
  ev("m1", "manual_marker", 86, "Marker at 1:26"),
  ev("13", "death", 92, "Killed by a tower"),
  ev("14", "baron", 104, "Helped take Baron Nashor", "Assists: Lee Sin, Jinx"),
  ev("c1", "clip", 106, "Clip saved at 1:46"),
  ev("15", "inhibitor", 115, "Destroyed an inhibitor", "Assists: Jinx"),
  ev("16", "ace", 122, "Team ace", "Final kill by Garen"),
  ev("17", "game_end", 130, "Victory"),
];

/** A scoreboard read every 10 s of the mock game (130 s): everyone levels, farms and buys; you
 * (Ahri) get kills at 0:14 and 1:18-1:20 like the events. */
export const MOCK_SB: Scoreboard = (() => {
  const roster: [string, string, string, string][] = [
    ["Tester#EUW", "Ahri", "Ahri", "ORDER"], ["Mate1#EUW", "Lee Sin", "LeeSin", "ORDER"], ["Mate2#EUW", "Jinx", "Jinx", "ORDER"], ["Mate3#EUW", "Thresh", "Thresh", "ORDER"], ["Mate4#EUW", "Garen", "Garen", "ORDER"],
    ["Enemy1#NA1", "Zed", "Zed", "CHAOS"], ["Enemy2#NA1", "Vi", "Vi", "CHAOS"], ["Enemy3#NA1", "Caitlyn", "Caitlyn", "CHAOS"], ["Enemy4#NA1", "Lux", "Lux", "CHAOS"], ["Enemy5#NA1", "Darius", "Darius", "CHAOS"],
  ];
  const build = [1056, 6655, 3020, 3089, 3157, 3135];
  const state = (i: number, t: number) => ({
    level: Math.min(18, 1 + Math.floor(t / (12 + i))),
    kills: i === 0 ? (t >= 14 ? 1 : 0) + (t >= 78 ? 1 : 0) + (t >= 79 ? 1 : 0) + (t >= 80 ? 1 : 0) : Math.floor(t / (40 + 7 * i)),
    deaths: i === 0 ? (t >= 31 ? 1 : 0) + (t >= 92 ? 1 : 0) : Math.floor(t / (55 + 5 * i)),
    assists: Math.floor(t / (30 + 4 * i)),
    cs: Math.floor((t / 60) * (6 + (i % 4))),
    // Bought items in slots 0.., the trinket (slot 6) after 1:00.
    items: Array.from({ length: t > 60 ? 7 : 6 }, (_, k) => (k === 6 ? 3340 : k < 1 + Math.floor(t / (35 + 3 * i)) ? build[k] : 0)),
    spells: i % 5 === 0 ? ["SummonerFlash", i === 0 ? "SummonerDot" : "SummonerTeleport"] : ["SummonerFlash", i % 5 === 1 ? "SummonerSmite" : "SummonerHeal"],
  });
  const frames: Scoreboard["frames"] = [];
  let last: ReturnType<typeof state>[] = [];
  for (let t = 0; t <= 130; t += 10) {
    const cur = roster.map((_, i) => state(i, t));
    const d = cur
      .map((c, i) => {
        const o = last[i];
        const x: Scoreboard["frames"][number]["d"][number] = { i };
        if (!o || o.level !== c.level) x.lv = c.level;
        if (!o || o.kills !== c.kills) x.k = c.kills;
        if (!o || o.deaths !== c.deaths) x.d = c.deaths;
        if (!o || o.assists !== c.assists) x.a = c.assists;
        if (!o || o.cs !== c.cs) x.cs = c.cs;
        if (!o || JSON.stringify(o.items) !== JSON.stringify(c.items)) x.it = c.items;
        if (!o || JSON.stringify(o.spells) !== JSON.stringify(c.spells)) x.sp = c.spells;
        return x;
      })
      .filter((x) => Object.keys(x).length > 1);
    frames.push({ t, d });
    last = cur;
  }
  return {
    version: "16.20.1",
    players: roster.map(([name, character, character_id, team], i) => ({ name, character, character_id, team, me: i === 0 })),
    names: { "1056": "Doran's Ring", "6655": "Luden's Echo", "3020": "Sorcerer's Shoes", "3089": "Rabadon's Deathcap", "3157": "Zhonya's Hourglass", "3135": "Void Staff", "3340": "Stealth Ward", SummonerFlash: "Flash", SummonerDot: "Ignite", SummonerTeleport: "Teleport", SummonerSmite: "Smite", SummonerHeal: "Heal" },
    frames,
  };
})();

/** APM per 10 s of the mock video (loading screen unfocused, a fight around 1:20). */
export const MOCK_APM: (number | null)[] = [null, null, 96, 142, 168, 150, 131, 205, 262, 188, 140, 176, 159, 120, 84];

// ?markers=600 adds that many extra events spread over the video (timeline speed tests).
const manyEvents: GameEvent[] | null = q.get("markers")
  ? [
      ...detailEvents,
      ...Array.from({ length: Number(q.get("markers")) }, (_, i) => {
        const kinds: GameEvent["kind"][] = ["kill", "death", "assist", "ult_used", "tower", "dragon", "manual_marker"];
        // Deterministic, clustered: groups of 3-5 events close together.
        const t = -19 + ((i * 7919) % 1490) / 10 + (i % 4) * 0.4;
        return ev(`x${i}`, kinds[i % kinds.length], Math.max(-19, Math.min(129, t)), `Event ${i}`);
      }),
    ]
  : null;

function makeSummary(i: number): SessionSummary {
  const [name, id] = champs[i % champs.length];
  const d = new Date(Date.now() - i * 0.55 * 86400000 - 3600000 * (i % 3));
  const win = i % 3 !== 1;
  return {
    id: `s${i}`,
    dir: `C:\\Users\\you\\Videos\\Clairvoyance\\s${i}`,
    game_id: "league",
    game_name: "League of Legends",
    started_at: d.toISOString(),
    player: { name: "Tester#EUW", character: name, character_id: id, team: "ORDER", mode: modes[i % modes.length] },
    stats: { kills: (i * 7) % 13, deaths: (i * 3) % 8, assists: (i * 5) % 17, cs: 150 + i * 13, gold: 11000 + i * 700, level: 16, vision_score: 20 + i, extra: [] },
    result: win ? "win" : "loss",
    video_path: VIDEO,
    thumb_path: q.get("nothumbs") === "1" ? null : `/dev-assets/thumb${i % 12}.jpg`,
    duration: 1500 + i * 97,
    favorite: i === 2,
    kept_clips: i === 4 ? 1 : 0,
    queue_id: [420, 400, 450, 1700, 440][i % 5],
    mode_name: ["Ranked Solo/Duo", "Normal Draft", "ARAM", "Arena", "Ranked Flex"][i % 5],
    record_mode: i % 5 === 3 ? "clips_only" : "full",
    video_removed: false,
    size_bytes: 2.1e9 + i * 1.3e8,
    video_bytes: 2.0e9 + i * 1.3e8,
    event_count: 18 + i,
    clip_count: i % 4,
    thumb_at: 110,
    // Games 2, 5, 8 were recorded before input tracking.
    input_bytes: i % 3 === 2 ? 0 : 1.4e6,
  };
}

// ?n=500 fills the library for performance tests.
const sessions: SessionSummary[] = Array.from({ length: Number(q.get("n") ?? 9) }, (_, i) => makeSummary(i));
sessions[0].duration = 130;

const settings: Settings = {
  first_run_done: q.get("setup") !== "1",
  save_dir: "",
  auto_delete_days: 30,
  max_disk_gb: 100,
  auto_cleanup: true,
  hotkey_clip: "F8",
  hotkey_marker: "F9",
  auto_record: true,
  close_ui_in_game: false,
  keep_ui_loaded: true,
  start_with_windows: true,
  start_minimized: true,
  show_perf: true,
  theme: (localStorage.getItem("cv-theme") as any) ?? "system",
  auto_update_check: true,
  video: { encoder: "auto", quality: "standard", fps: 60, height: 1080, replay_buffer_secs: 30, record_mic: false, display_capture: false, codec: "h264", rate_control: "bitrate", playable_codecs: [] },
  events: { clip_kinds: ["multikill", "ace"], clip_before_secs: 10, clip_after_secs: 4, tts_enabled: false, tts_kinds: ["kill", "death", "multikill", "clip"], tts_volume: 70 },
  games: { league: { riot_id: "Tester#EUW", ult_key: "R" } },
  disabled_games: [],
  ffmpeg_path: "",
  share_fit_discord: true,
};

const live: LiveStatus =
  q.get("live") === "1"
    ? {
        state: "recording",
        game_id: "league",
        game_name: "League of Legends",
        phase: "in_progress",
        game_time: 874,
        session_id: "live",
        player: { name: "Tester#EUW", character: "Ahri", character_id: "Ahri", mode: "Ranked Solo" },
        stats: { kills: 5, deaths: 1, assists: 7, cs: 112, extra: [] },
        event_count: 17,
        last_event: ev("x", "herald", 860, "Stole Rift Herald", "", true),
        video_offset: 41.2,
        recorder: { connected: true, recording: true, replay_buffer: true, version: "32.0.1", encoder: "nvenc", hardware_encoder: true },
      }
    : {
        state: "idle",
        phase: "waiting",
        event_count: 0,
        recorder: { connected: false, recording: false, replay_buffer: false, hardware_encoder: false },
      };

function session(id: string): GameSession {
  const s = sessions.find((x) => x.id === id) ?? sessions[0];
  const timeline = Array.from({ length: 6 }, (_, i) => ({ t: i * 25, kills: 0, deaths: 0, assists: 0, cs: Math.round(i * 25 * 0.12 * 60 / 60 * 7), gold: 500 + i * 1900 + (i > 3 ? 900 : 0) }));
  return {
    version: 1,
    id: s.id,
    game_id: s.game_id,
    game_name: s.game_name,
    started_at: s.started_at,
    video_file: "sample.mp4",
    video_offset: 20,
    video_duration: 150,
    // The third game is "recorded before v1.8": no scoreboard snapshots.
    scoreboard: s.id === "s2" ? null : MOCK_SB,
    game_duration: 130,
    player: s.player,
    stats: { kills: 5, deaths: 2, assists: 3, cs: 108, gold: 11240, level: 13, vision_score: 14, extra: [] },
    result: "win",
    events: manyEvents ?? detailEvents,
    clips: [
      { file: "clip_1-46.mp4", title: "Clip at 1:46", video_start: 96, video_end: 126, created_at: s.started_at, source: "replay", keep: true },
      { file: "Triple-kill_1-20.mp4", title: "Killed Lux + Triple kill", video_start: 88, video_end: 104, created_at: s.started_at, source: "event", keep: false },
    ],
    timeline,
    perf: { samples: 312, cpu_avg: 0.18, cpu_max: 0.9, ram_avg_mb: 41, ram_max_mb: 47 },
    favorite: s.favorite,
    warnings: [],
    input_file: s.input_bytes ? `${s.id}.input` : null,
    mechanics: s.input_bytes
      ? { version: 1, from: 20, to: 150, focused_secs: 128, clicks: 260, right_clicks: 212, key_presses: 141, apm: 188, apm_per_min: [151, 204, 176], right_click_hz: 1.66, cursor_distance: 214, path_efficiency: 0.83, idle_secs: 9.4, cursor_samples: 31000, apm_bins: MOCK_APM }
      : null,
  };
}

const clips: ClipEntry[] = sessions.slice(0, Math.max(5, Math.floor(sessions.length / 2))).flatMap((s, i) => [
  {
    session_id: s.id,
    game_name: s.game_name,
    character: s.player?.character,
    character_id: s.player?.character_id,
    path: VIDEO,
    file: `clip${i}.mp4`,
    title: i % 2 ? "Triple kill" : `Clip at ${10 + (i % 20)}:2${i % 10}`,
    created_at: s.started_at,
    size_bytes: 3.2e7 + i * 4e6,
    source: i % 2 ? "event" : "replay",
    keep: i === 0,
    thumb_path: q.get("nothumbs") === "1" ? null : `/dev-assets/thumb${(i + 5) % 12}.jpg`,
    duration: 30,
  },
]);

let legacyRemoved = false;

const mockModes: any = {
  game_id: "league",
  game_name: "League of Legends",
  groups: [
    { id: "ranked", label: "Ranked", help: "Solo/Duo and Flex" },
    { id: "normal", label: "Normal", help: "Draft, Quickplay, Swiftplay" },
    { id: "aram", label: "ARAM", help: "Howling Abyss" },
    { id: "arena", label: "Arena", help: "2v2v2v2" },
    { id: "rotating", label: "Rotating & event modes", help: "URF, One for All and other limited-time modes" },
    { id: "other", label: "Other", help: "Custom games, Practice Tool, Co-op vs AI, Tutorial" },
  ],
  modes: {
    unknown_rule: "record",
    catalog_updated_at: new Date(Date.now() - 3600e3).toISOString(),
    entries: {
      q420: { name: "Ranked Solo/Duo", queue_id: 420, group: "ranked", rule: "record", available: true, is_new: false },
      q440: { name: "Ranked Flex", queue_id: 440, group: "ranked", rule: "record", available: true, is_new: false },
      q400: { name: "Normal Draft", queue_id: 400, group: "normal", rule: "record", available: true, is_new: false },
      q490: { name: "Quickplay", queue_id: 490, group: "normal", rule: "record", available: true, is_new: false },
      q480: { name: "Swiftplay", queue_id: 480, group: "normal", rule: "record", available: true, is_new: false },
      q430: { name: "Normal Blind", queue_id: 430, group: "normal", rule: "record", available: false, is_new: false },
      q450: { name: "ARAM", queue_id: 450, group: "aram", rule: "off", available: true, is_new: false },
      q1700: { name: "Arena", queue_id: 1700, group: "arena", rule: "clips_only", available: true, is_new: false },
      q1900: { name: "Pick URF", queue_id: 1900, group: "rotating", rule: "record", available: false, is_new: false },
      q1020: { name: "One for All", queue_id: 1020, group: "rotating", rule: "record", available: false, is_new: false },
      q2400: { name: "ARAM Mayhem", queue_id: 2400, group: "rotating", rule: "record", available: true, is_new: true },
      custom: { name: "Custom games", group: "other", rule: "off", available: true, is_new: false },
      practice: { name: "Practice Tool", group: "other", rule: "off", available: true, is_new: false },
      q870: { name: "Co-op vs. AI Intro", queue_id: 870, group: "other", rule: "record", available: true, is_new: false },
    },
  },
};
let mockUpdate: any = q.get("update") === "1"
  ? { state: "available", current_version: "1.0.0", version: "1.0.1", notes: "- Timeline jumps are faster\n- Light theme polish\n- Fixed: thumbnails for very short games", downloaded: 0, total: null, checked_at: new Date().toISOString() }
  : { state: "up_to_date", current_version: "1.0.0", downloaded: 0, checked_at: new Date().toISOString() };

/** Overlay exports the mock received (UI tests read them: the PNG of every frame). */
const exports: { job: number; id: string; start: number; end: number; title: string; times: number[]; frames: Uint8Array[]; done: boolean; cancelled: boolean }[] = [];
(globalThis as any).__cvExports = exports;

export async function invoke(cmd: string, args: any = {}): Promise<any> {
  // Overlay frames come fast and many: no fake latency.
  if (cmd === "overlay_export_frame") {
    const e = exports.find((x) => x.job === Number(args.headers?.["x-job"]));
    if (!e || e.cancelled) throw new Error("This export was cancelled.");
    e.frames.push(new Uint8Array(args.body));
    return null;
  }
  await new Promise((r) => setTimeout(r, 60));
  switch (cmd) {
    case "overlay_export_begin": {
      const fps = 60;
      const n = Math.max(1, Math.round((args.end - args.start) * fps));
      const times = Array.from({ length: n }, (_, k) => args.start + k / fps);
      const job = exports.length + 1;
      exports.push({ job, id: args.id, start: args.start, end: args.end, title: args.title, times, frames: [], done: false, cancelled: false });
      return { job, width: 960, height: 540, fps, times };
    }
    case "overlay_export_end": {
      const e = exports.find((x) => x.job === args.job);
      if (!e || e.cancelled) throw new Error("This export was cancelled.");
      e.done = true;
      return `C:\\Users\\you\\Videos\\Clairvoyance\\${args.job}\\clips\\${e.title || "Clip"}.mp4`;
    }
    case "overlay_export_cancel": {
      const e = exports.find((x) => x.job === args.job);
      if (e) e.cancelled = true;
      return null;
    }
    case "get_status":
      return live;
    case "app_info":
      return {
        version: "0.2.0",
        legacy_install: q.get("legacy") === "1" && !legacyRemoved,
        log_file: "C:\\Users\\you\\AppData\\Local\\Clairvoyance\\logs\\clairvoyance.log",
        config_file: "C:\\Users\\you\\AppData\\Roaming\\Clairvoyance\\settings.json",
        default_save_dir: "C:\\Users\\you\\Videos\\Clairvoyance",
        save_dir: "C:\\Users\\you\\Videos\\Clairvoyance",
        gpu: { vendor: "NVIDIA", name: "NVIDIA GeForce RTX 5080", encoder: "nvenc" },
        games: [
          {
            id: "league",
            name: "League of Legends",
            short_name: "League",
            supports_events: true,
            config_fields: [
              { key: "riot_id", label: "Riot ID", kind: "text", help: "Your Riot ID, e.g. Name#EUW. Used as a fallback; the app normally detects you automatically." },
              { key: "ult_key", label: "Ult key", kind: "key", help: 'The key you cast your ultimate with. Presses are marked as "Ult pressed" (the game can\'t confirm the cast).' },
              { key: "record_input", label: "Record mouse & keyboard input", kind: "bool", help: "For the replay's input overlay and the Mechanics stats." },
              { key: "input_rate", label: "Cursor sample rate", kind: "select", help: "How often the cursor position is read.", options: [["125", "125 Hz"], ["250", "250 Hz (default)"], ["500", "500 Hz"]] },
            ],
            default_config: { riot_id: "", ult_key: "R", record_input: true, input_rate: "250" },
            input_tracking: true,
            action_categories: [
              { id: "ability", label: "Abilities" },
              { id: "summoner", label: "Summoners" },
              { id: "item", label: "Items" },
              { id: "ward", label: "Ward" },
            ],
          },
        ],
      };
    case "ui_ready":
      return { since_launch_ms: args.pageMs + 400 };
    case "ui_timings":
      return { startup_ms: 812, page_ms: 356 };
    case "update_status":
      return mockUpdate;
    case "update_check":
      await new Promise((r) => setTimeout(r, 900));
      mockUpdate = { ...mockUpdate, state: "available", version: "1.0.1", notes: "- Faster library\n- Fixed a thumbnail bug", checked_at: new Date().toISOString() };
      emit("update-status", mockUpdate);
      return mockUpdate;
    case "update_install": {
      for (let i = 1; i <= 10; i++) {
        await new Promise((r) => setTimeout(r, 200));
        mockUpdate = { ...mockUpdate, state: "downloading", downloaded: i * 2.6e6, total: 2.6e7 };
        emit("update-status", mockUpdate);
      }
      mockUpdate = { ...mockUpdate, state: "installing" };
      emit("update-status", mockUpdate);
      return;
    }
    case "modes_get":
    case "modes_refresh":
      return [structuredClone(mockModes)];
    case "modes_set": {
      const m = mockModes.modes;
      const E = Object.values(m.entries) as any[];
      if (args.what === "mode") Object.assign(m.entries[args.key], { rule: args.rule, is_new: false });
      if (args.what === "group") E.filter((e) => e.group === args.key).forEach((e) => ((e.rule = args.rule), (e.is_new = false)));
      if (args.what === "unknown") m.unknown_rule = args.rule;
      if (args.what === "seen") E.forEach((e) => (e.is_new = false));
      if (args.what === "preset") {
        const on = (g: string) => args.key === "everything" || g === "ranked" || (args.key === "ranked_normal" && g === "normal");
        E.forEach((e) => ((e.rule = on(e.group) ? "record" : "off"), (e.is_new = false)));
        m.unknown_rule = args.key === "everything" ? "record" : "off";
      }
      return [structuredClone(mockModes)];
    }
    case "set_theme":
      settings.theme = args.theme;
      return;
    case "remove_legacy_app":
      await new Promise((r) => setTimeout(r, 800));
      legacyRemoved = true;
      return;
    case "get_settings":
      return structuredClone(settings);
    case "save_settings":
      Object.assign(settings, args.settings);
      return;
    case "finish_first_run":
      settings.first_run_done = true;
      return;
    case "list_sessions":
      return sessions;
    case "get_session":
      return {
        session: session(args.id),
        dir: "C:\\Users\\you\\Videos\\Clairvoyance\\" + args.id,
        video_path: VIDEO,
        thumb_path: null,
        clips: session(args.id).clips.map((c) => ({ ...c, path: VIDEO, exists: true })),
        input_bytes: sessions.find((x) => x.id === args.id)?.input_bytes || null,
      };
    case "list_clips":
      return clips;
    case "video_frame_times": {
      const name = String(args.path ?? "").replace(/\.webm$/, ".frames.json");
      try {
        const r = await fetch(name);
        if (r.ok) return Float64Array.from(await r.json()).buffer;
      } catch {}
      return Float64Array.from({ length: 160 * 30 }, (_, k) => Math.round((k * 1000) / 30) / 1000).buffer;
    }
    case "video_keyframes": {
      const name = String(args.path ?? "").replace(/\.webm$/, ".keyframes.json");
      try {
        const r = await fetch(name);
        if (r.ok) return await r.json();
      } catch {}
      return Array.from({ length: 160 }, (_, k) => k);
    }
    case "input_load":
      await new Promise((r) => setTimeout(r, 30));
      return synthetic(150);
    case "input_actions":
      return syntheticActions(150);
    case "input_stats":
      return { version: 1, from: args.from, to: args.to, focused_secs: args.to - args.from, clicks: 99, right_clicks: 80, key_presses: 40, apm: 205, apm_per_min: [], right_click_hz: 1.9, cursor_distance: 70, path_efficiency: 0.79, idle_secs: 2, cursor_samples: 9000 };
    case "perf_now":
      return { cpu: 0.2, ram_mb: 38 };
    case "storage_info":
      return {
        save_dir: "C:\\Users\\you\\Videos\\Clairvoyance",
        used_bytes: sessions.reduce((a, s) => a + s.size_bytes, 0),
        limit_bytes: settings.max_disk_gb * 1024 ** 3,
        auto_cleanup: settings.auto_cleanup,
        free_bytes: 812e9,
        games: sessions.length,
        protected_bytes: 2.4e9,
        stuck_over_limit: q.get("stuck") === "1",
        recent_cleanups: [
          { at: new Date(Date.now() - 3600e3).toISOString(), id: "old1", title: "League of Legends · Zed · 2 Sep 2026 21:14", action: "delete_game", bytes: 2.3e9, reason: "storage limit" },
          { at: new Date(Date.now() - 3600e3).toISOString(), id: "old2", title: "League of Legends · Lux · 1 Sep 2026 19:02", action: "delete_video", bytes: 1.9e9, reason: "storage limit" },
        ],
        thumbnails_dir: "C:\\Users\\you\\AppData\\Local\\Clairvoyance\\Thumbnails",
      };
    case "cleanup_now":
      await new Promise((r) => setTimeout(r, 700));
      return { thumbs_made: 0, removed: [], freed_bytes: 0, still_over: false, skipped_busy: false };
    case "share_clip": {
      const c = clips.find((c) => c.session_id === args.id && c.file === args.file);
      if (!c) throw "That clip's file is missing.";
      await new Promise((r) => setTimeout(r, 400));
      const fit = settings.share_fit_discord;
      const small = c.size_bytes <= 19_500_000;
      return { path: c.path, bytes: fit && !small ? 18_900_000 : c.size_bytes, width: 1920, height: 1080, fps: fit && !small ? 30 : 60, fitted: fit && !small, copied: true };
    }
    case "set_share_fit_discord":
      settings.share_fit_discord = args.on;
      return;
    case "set_clip_keep": {
      const c = clips.find((c) => c.session_id === args.id && c.file === args.file);
      if (c) c.keep = args.keep;
      emit("library-changed", null);
      return;
    }
    case "set_favorite": {
      const g = sessions.find((s) => s.id === args.id);
      if (g) g.favorite = args.favorite;
      emit("library-changed", null);
      return;
    }
    case "builtin_encoders":
      return { gpu: "NVIDIA GeForce RTX 5080", encoders: ["NVIDIA H.264 Encoder MFT (NVIDIA)"] };
    case "recorder_selftest":
      await new Promise((r) => setTimeout(r, 1500));
      return { path: "C:\\x.mp4", bytes: 7.4e6, frames: 300, dropped: 0, fps: 60, cpu_percent: 0.41 };
    case "perf_test_status":
      return q.get("perf") === "1"
        ? {
            state: "done",
            report_path: "C:\\report.txt",
            report: {
              started_at: "2026-09-30 21:10",
              gpu: "NVIDIA GeForce RTX 5080",
              encoder: "NVIDIA H.264 Encoder MFT (NVIDIA)",
              fps_source: "PresentMon (frame times from Windows)",
              notes: [],
              phases: [
                { name: "Not recording", secs: 60, frames: 14400, fps_avg: 240.1, fps_1_low: 171.2, frametime_p99_ms: 5.6, game_cpu: 9.8, total_cpu: 14.2, app_cpu: 0.05, app_ram_mb: 34, gpu_3d: 41.5, gpu_encode: 0, game_gpu_3d: 39.9 },
                { name: "Built-in recorder", secs: 60, frames: 14250, fps_avg: 237.4, fps_1_low: 168.8, frametime_p99_ms: 5.8, game_cpu: 9.9, total_cpu: 15.1, app_cpu: 0.62, app_ram_mb: 96, gpu_3d: 43.0, gpu_encode: 11.8, game_gpu_3d: 40.1 },
              ],
            },
          }
        : { state: "idle" };
    case "ffmpeg_status":
      // `?ffmpeg=1`: as if ffmpeg were installed (the clip export tests).
      return { available: new URLSearchParams(location.search).has("ffmpeg") };
    // Replay benchmark in the browser preview (`?bench=1`), to check the harness itself.
    case "bench_config":
      return q.get("bench") ? { sessions: [sessions[0].id], runs: 2, early_runs: 1, label: "mock" } : null;
    case "bench_prepare":
      return { [sessions[0].id]: { video: VIDEO, before: { codec: "vp09.00.10.08", width: 1280, height: 720 } } };
    case "bench_log":
      console.log("[bench]", args.line);
      return;
    case "bench_finish":
      (window as any).__benchResult = args.result;
      console.log("[bench] done");
      return;
    default:
      return null;
  }
}
