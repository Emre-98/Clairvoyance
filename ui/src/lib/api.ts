import type {
  AppInfo,
  ClipEntry,
  EngineEvent,
  LiveStatus,
  RecorderStatus,
  SessionSummary,
  SessionView,
  Settings,
  StorageInfo,
  PerfTestStatus,
  CleanupReport,
  UpdateStatus,
  GameModesView,
  SelfTestResult,
} from "./types";

type Invoke = <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;
type Listen = (event: string, cb: (payload: any) => void) => Promise<() => void>;

export const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

let invokeImpl: Invoke;
let listenImpl: Listen;
let fileSrcImpl: (path: string) => string = (p) => p;

async function init() {
  if (isTauri) {
    const core = await import("@tauri-apps/api/core");
    const ev = await import("@tauri-apps/api/event");
    invokeImpl = core.invoke as Invoke;
    listenImpl = async (name, cb) => ev.listen(name, (e) => cb(e.payload));
    fileSrcImpl = (p) => core.convertFileSrc(p);
  } else if (import.meta.env.DEV || import.meta.env.VITE_MOCK) {
    // Browser preview with fake data (npm run dev).
    const mock = await import("./mock");
    invokeImpl = mock.invoke as Invoke;
    listenImpl = mock.listen;
    fileSrcImpl = mock.fileSrc;
  } else {
    invokeImpl = async () => {
      throw new Error("Not running inside the app");
    };
    listenImpl = async () => () => {};
  }
}

const ready = init();

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  await ready;
  return invokeImpl<T>(cmd, args);
}

export async function on(event: string, cb: (payload: any) => void): Promise<() => void> {
  await ready;
  return listenImpl(event, cb);
}

export function onEngine(cb: (e: EngineEvent) => void) {
  return on("engine", cb);
}

export function fileSrc(path: string | null | undefined): string {
  if (!path) return "";
  return fileSrcImpl(path);
}

export const api = {
  status: () => call<LiveStatus>("get_status"),
  appInfo: () => call<AppInfo>("app_info"),
  getSettings: () => call<Settings>("get_settings"),
  saveSettings: (settings: Settings) => call<void>("save_settings", { settings }),
  listSessions: () => call<SessionSummary[]>("list_sessions"),
  getSession: (id: string) => call<SessionView>("get_session", { id }),
  setFavorite: (id: string, favorite: boolean) => call<void>("set_favorite", { id, favorite }),
  deleteSession: (id: string) => call<void>("delete_session", { id }),
  listClips: () => call<ClipEntry[]>("list_clips"),
  deleteClip: (id: string, file: string) => call<void>("delete_clip", { id, file }),
  exportClip: (id: string, start: number, end: number, title: string, precise: boolean) =>
    call<string>("export_clip", { id, start, end, title, precise }),
  saveClipNow: () => call<void>("save_clip_now"),
  addMarkerNow: () => call<void>("add_marker_now"),
  stopSession: () => call<void>("stop_session"),
  reveal: (path: string) => call<void>("reveal_path", { path }),
  openPath: (path: string) => call<void>("open_path", { path }),
  openUrl: (url: string) => call<void>("open_url", { url }),
  recorderStatus: () => call<RecorderStatus>("recorder_status"),
  ffmpegStatus: () => call<{ available: boolean; path?: string | null }>("ffmpeg_status"),
  ffmpegDownload: () => call<string>("ffmpeg_download"),
  perfNow: () => call<{ cpu: number; ram_mb: number } | null>("perf_now"),
  storageInfo: () => call<StorageInfo>("storage_info"),
  cleanupNow: () => call<CleanupReport>("cleanup_now"),
  setClipKeep: (id: string, file: string, keep: boolean) => call<void>("set_clip_keep", { id, file, keep }),
  finishFirstRun: () => call<void>("finish_first_run"),
  simulateGame: (speed: number, length: number, queue: number | null = null) => call<void>("simulate_game", { speed, length, queue }),
  videoInfo: (path: string) => call<{ layout: string; fragments: number; keyframe_interval_avg: number; keyframe_interval_max: number; duration_secs: number; codec: string | null }>("video_info", { path }),
  videoKeyframes: (path: string) => call<number[]>("video_keyframes", { path }),
  testReport: (ui: unknown) => call<string>("test_report", { ui }),
  quit: () => call<void>("quit_app"),
  removeLegacyApp: () => call<void>("remove_legacy_app"),
  playerOpen: (path: string | null) => call<void>("player_open", { path }),
  benchConfig: () => call<any | null>("bench_config"),
  benchPrepare: (sessions: string[], finalize: boolean) => call<Record<string, any>>("bench_prepare", { sessions, finalize }),
  benchFinish: (result: unknown) => call<void>("bench_finish", { result }),
  benchLog: (line: string) => call<void>("bench_log", { line }),
  uiReady: (pageMs: number) => call<{ since_launch_ms: number | null }>("ui_ready", { pageMs }),
  modesGet: () => call<GameModesView[]>("modes_get"),
  modesRefresh: () => call<GameModesView[]>("modes_refresh"),
  modesSet: (game: string, what: "mode" | "group" | "unknown" | "preset" | "seen", key?: string | null, rule?: string | null) =>
    call<GameModesView[]>("modes_set", { game, what, key: key ?? null, rule: rule ?? null }),
  updateStatus: () => call<UpdateStatus>("update_status"),
  updateCheck: () => call<UpdateStatus>("update_check"),
  updateInstall: () => call<void>("update_install"),
  uiTimings: () => call<{ startup_ms: number | null; page_ms: number | null }>("ui_timings"),
  setTheme: (theme: string, darkNow: boolean) => call<void>("set_theme", { theme, darkNow }),
  builtinEncoders: () => call<{ gpu: string; encoders: string[] }>("builtin_encoders"),
  recorderSelftest: (secs: number) => call<SelfTestResult>("recorder_selftest", { secs }),
  perfTestStart: (phaseSecs: number) => call<void>("perf_test_start", { phaseSecs }),
  perfTestStatus: () => call<PerfTestStatus>("perf_test_status"),
  perfTestCancel: () => call<void>("perf_test_cancel"),
};

export async function pickFolder(): Promise<string | null> {
  if (!isTauri) return null;
  const { open } = await import("@tauri-apps/plugin-dialog");
  const r = await open({ directory: true, multiple: false });
  return typeof r === "string" ? r : null;
}

export async function pickFile(filters?: { name: string; extensions: string[] }[]): Promise<string | null> {
  if (!isTauri) return null;
  const { open } = await import("@tauri-apps/plugin-dialog");
  const r = await open({ directory: false, multiple: false, filters });
  return typeof r === "string" ? r : null;
}

export async function confirmDialog(message: string, title = "Clairvoyance"): Promise<boolean> {
  if (!isTauri) return window.confirm(message);
  const { ask } = await import("@tauri-apps/plugin-dialog");
  return ask(message, { title, kind: "warning" });
}

export async function windowAction(action: "minimize" | "maximize" | "close") {
  if (!isTauri) return;
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  const w = getCurrentWindow();
  if (action === "minimize") await w.minimize();
  else if (action === "maximize") await w.toggleMaximize();
  else await w.close();
}
