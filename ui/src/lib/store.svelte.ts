import { api, on, onEngine } from "./api";
import { initTheme } from "./theme";
import type { AppInfo, LiveStatus, SessionSummary, SessionView, Settings } from "./types";

export type Route =
  | { page: "home" }
  | { page: "games" }
  | { page: "clips" }
  | { page: "settings"; section?: string }
  | { page: "game"; id: string; t?: number };

export interface Toast {
  id: number;
  text: string;
  level: "info" | "ok" | "warn" | "error";
}

export const app = $state({
  route: { page: "home" } as Route,
  status: null as LiveStatus | null,
  sessions: [] as SessionSummary[],
  sessionsLoaded: false,
  info: null as AppInfo | null,
  settings: null as Settings | null,
  toasts: [] as Toast[],
  /** Bumped whenever something on disk changed. */
  libraryVersion: 0,
});

// Game pages open instantly: their data is fetched on hover and kept until something changes.
const sessionCache = new Map<string, SessionView>();
const inflight = new Map<string, Promise<SessionView>>();
export function cachedSession(id: string): SessionView | null {
  return sessionCache.get(id) ?? null;
}
export function fetchSession(id: string): Promise<SessionView> {
  const running = inflight.get(id);
  if (running) return running;
  const p = api
    .getSession(id)
    .then((v) => {
      sessionCache.set(id, v);
      return v;
    })
    .finally(() => inflight.delete(id));
  inflight.set(id, p);
  return p;
}
export function prefetchSession(id: string) {
  if (!sessionCache.has(id)) fetchSession(id).catch(() => {});
}

let toastId = 0;
export function toast(text: string, level: Toast["level"] = "info", ms = 4500) {
  const id = ++toastId;
  app.toasts.push({ id, text, level });
  setTimeout(() => {
    app.toasts = app.toasts.filter((t) => t.id !== id);
  }, ms);
}

export function go(route: Route) {
  app.route = route;
}

export async function refreshSessions() {
  try {
    app.sessions = await api.listSessions();
  } catch (e) {
    toast(`Couldn't load your games: ${e}`, "error");
  } finally {
    app.sessionsLoaded = true;
  }
}

export async function reloadSettings() {
  app.settings = await api.getSettings();
  app.info = await api.appInfo();
}

export async function saveSettings(s: Settings) {
  await api.saveSettings($state.snapshot(s) as Settings);
  await reloadSettings();
}

let booted = false;
export async function boot() {
  if (booted) return;
  booted = true;
  await Promise.all([reloadSettings(), api.status().then((s) => {
      app.status = s;
    }), refreshSessions()]);
  initTheme(app.settings?.theme);
  await onEngine((e) => {
    if (e.type === "status") {
      const { type: _t, ...s } = e;
      app.status = s as LiveStatus;
    } else if (e.type === "notice") {
      toast(e.text, e.level === "error" ? "error" : e.level === "warn" ? "warn" : "info", 7000);
    } else if (e.type === "game_ended") {
      toast("Game saved. Open it from your library.", "ok");
    }
  });
  await on("library-changed", () => {
    sessionCache.clear();
    app.libraryVersion++;
    refreshSessions();
  });
  // The window was hidden (UI paused to save resources) and is shown again: catch up.
  await on("resync", (s: LiveStatus | null) => {
    if (s) app.status = s;
    sessionCache.clear();
    app.libraryVersion++;
    refreshSessions();
  });
}
