// Fake backend for `npm run dev` in a normal browser (UI work and screenshots only).
// Never included in the production build.
import type { ClipEntry, GameEvent, GameSession, LiveStatus, SessionSummary, Settings } from "./types";

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
  ev("3", "objective", 22, "Took Voidgrub"),
  ev("4", "death", 31, "Killed by Vi", "Assists: Zed"),
  ev("5", "dragon", 44, "Helped take Infernal Drake", "Assists: Ahri"),
  ev("6", "assist", 52, "Assist on Caitlyn", "Killed by Jinx"),
  ev("7", "tower", 61, "Destroyed a tower"),
  ev("8", "herald", 68, "Stole Rift Herald", "", true),
  ev("u2", "ult_pressed", 77, "Ult pressed", "Key press. The game can't confirm the ult was cast."),
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
    video_path: "/dev-assets/sample.webm",
    thumb_path: null,
    duration: 1500 + i * 97,
    favorite: i === 2,
    size_bytes: 2.1e9 + i * 1.3e8,
    event_count: 18 + i,
    clip_count: i % 4,
  };
}

const sessions: SessionSummary[] = Array.from({ length: 9 }, (_, i) => makeSummary(i));
sessions[0].duration = 130;

const settings: Settings = {
  first_run_done: q.get("setup") !== "1",
  save_dir: "",
  auto_delete_days: 30,
  max_disk_gb: 200,
  hotkey_clip: "F8",
  hotkey_marker: "F9",
  auto_record: true,
  close_ui_in_game: false,
  keep_ui_loaded: true,
  start_with_windows: true,
  start_minimized: true,
  show_perf: true,
  video: { encoder: "auto", quality: "standard", fps: 60, height: 1080, replay_buffer_secs: 30, record_mic: false, display_capture: false },
  events: { clip_kinds: ["multikill", "ace"], clip_before_secs: 10, clip_after_secs: 4, tts_enabled: false, tts_kinds: ["kill", "death", "multikill", "clip"], tts_volume: 70 },
  games: { league: { riot_id: "Tester#EUW", ult_key: "R" } },
  disabled_games: [],
  ffmpeg_path: "",
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
    game_duration: 130,
    player: s.player,
    stats: { kills: 5, deaths: 2, assists: 3, cs: 108, gold: 11240, level: 13, vision_score: 14, extra: [] },
    result: "win",
    events: detailEvents,
    clips: [
      { file: "clip_1-46.mp4", title: "Clip at 1:46", video_start: 96, video_end: 126, created_at: s.started_at, source: "replay" },
      { file: "Triple-kill_1-20.mp4", title: "Killed Lux + Triple kill", video_start: 88, video_end: 104, created_at: s.started_at, source: "event" },
    ],
    timeline,
    perf: { samples: 312, cpu_avg: 0.18, cpu_max: 0.9, ram_avg_mb: 41, ram_max_mb: 47 },
    favorite: s.favorite,
    warnings: [],
  };
}

const clips: ClipEntry[] = sessions.slice(0, 5).flatMap((s, i) => [
  { session_id: s.id, game_name: s.game_name, character: s.player?.character, character_id: s.player?.character_id, path: "/dev-assets/sample.webm", title: i % 2 ? "Triple kill" : `Clip at ${10 + i}:2${i}`, created_at: s.started_at, size_bytes: 3.2e7 + i * 4e6, source: i % 2 ? "event" : "replay" },
]);

let legacyRemoved = false;

export async function invoke(cmd: string, args: any = {}): Promise<any> {
  await new Promise((r) => setTimeout(r, 60));
  switch (cmd) {
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
            ],
            default_config: { riot_id: "", ult_key: "R" },
          },
        ],
      };
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
        video_path: "/dev-assets/sample.webm",
        thumb_path: null,
        clips: session(args.id).clips.map((c) => ({ ...c, path: "/dev-assets/sample.webm", exists: true })),
      };
    case "list_clips":
      return clips;
    case "perf_now":
      return { cpu: 0.2, ram_mb: 38 };
    case "storage_info":
      return { save_dir: "C:\\Users\\you\\Videos\\Clairvoyance", used_bytes: 23.4e9, free_bytes: 812e9, games: sessions.length };
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
      return { available: false };
    case "set_favorite": {
      const s = sessions.find((x) => x.id === args.id);
      if (s) s.favorite = args.favorite;
      emit("library-changed", null);
      return;
    }
    default:
      return null;
  }
}
