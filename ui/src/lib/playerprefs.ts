// Replay player preferences, remembered between replays (this PC only).

export interface PlayerPrefs {
  /** Fullscreen: the controls panel is slid down (true fullscreen, nothing over the video). */
  panelDown: boolean;
  /** Fullscreen on a screen with another aspect than the video: "fit" (whole video) or "fill" (crop). */
  fit: "fit" | "fill";
  /** Fullscreen panel opacity, 0.4 .. 1. */
  panelOpacity: number;
}

export const DEFAULT_PREFS: PlayerPrefs = { panelDown: false, fit: "fit", panelOpacity: 0.75 };
const KEY = "cv.player";

export function loadPrefs(): PlayerPrefs {
  try {
    const raw = localStorage.getItem(KEY);
    if (raw) {
      const p = { ...DEFAULT_PREFS, ...JSON.parse(raw) } as PlayerPrefs;
      p.panelDown = !!p.panelDown;
      p.fit = p.fit === "fill" ? "fill" : "fit";
      const o = Number(p.panelOpacity);
      p.panelOpacity = Number.isFinite(o) ? Math.min(1, Math.max(0.4, Math.round(o * 100) / 100)) : DEFAULT_PREFS.panelOpacity;
      return p;
    }
  } catch {}
  return { ...DEFAULT_PREFS };
}

export function savePrefs(p: PlayerPrefs) {
  try {
    localStorage.setItem(KEY, JSON.stringify(p));
  } catch {}
}
