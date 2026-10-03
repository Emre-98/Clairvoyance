// Options of the replay input overlay (remembered in localStorage) and their migration between
// versions. No runtime imports, so the migration is unit-tested with plain Node
// (tests/bubbles.unit.test.ts).

/** The one "Trail & bubbles" time (s): the trail's length and how long a bubble stays. */
export const SECS_MIN = 0.25;
export const SECS_MAX = 3;
export const SECS_STEP = 0.05;
export const SECS_DEFAULT = 1;

export interface OverlayOptions {
  trail: boolean;
  /** "Trail & bubbles" (s, 0.25-3): the trail's length, and how long a bubble lives (it goes with
   * the trail piece of its moment). Also used when the trail itself is off. */
  trailSecs: number;
  clicks: boolean;
  dot: boolean;
  keys: boolean;
  heat: boolean;
  /** "game" = whole game, "range" = the selected range (if any). */
  heatRange: "game" | "range";
  /** Ability bubbles (action keys: abilities, summoners, items, ward). */
  bubbles: boolean;
  /** Category id -> shown (missing = shown). */
  bubbleCats: Record<string, boolean>;
}

export const DEFAULT_OPTIONS: OverlayOptions = {
  trail: true,
  trailSecs: SECS_DEFAULT,
  clicks: true,
  dot: true,
  keys: false,
  heat: false,
  heatRange: "game",
  bubbles: true,
  bubbleCats: {},
};
const OPT_KEY = "cv.inputOverlay";

/** Saved-options format: 2 = one "Trail & bubbles" time (v1.7). Before: `trailSecs` (trail,
 * 0.25-3) and `bubbleFade` (bubbles, 0.1-3) separately. */
const OPT_VERSION = 2;

export function clampSecs(v: unknown): number {
  const f = Number(v);
  if (!Number.isFinite(f)) return SECS_DEFAULT;
  return Math.min(SECS_MAX, Math.max(SECS_MIN, Math.round(f / SECS_STEP) / Math.round(1 / SECS_STEP)));
}

/**
 * Migrates saved options to the current format. Old (v1.5-v1.6) options had two times: the
 * trail length is kept when the trail was on (it's what the owner saw as the trail); when only
 * the bubbles were in use (trail off, bubbles on) their fade time becomes the one time; a saved
 * fade alone (trail length never saved) is used too. Then clamped to 0.25-3 s.
 */
export function migrateOptions(raw: Record<string, unknown>): OverlayOptions {
  const r = { ...raw };
  if (r.v !== OPT_VERSION) {
    const trail = Number(r.trailSecs);
    const fade = Number(r.bubbleFade);
    const hasTrail = "trailSecs" in r && Number.isFinite(trail);
    const hasFade = "bubbleFade" in r && Number.isFinite(fade);
    const bubblesOnly = r.trail === false && r.bubbles !== false;
    if (hasFade && (bubblesOnly || !hasTrail)) r.trailSecs = fade;
  }
  delete r.bubbleFade;
  delete r.v;
  const o = { ...DEFAULT_OPTIONS, bubbleCats: {}, ...r } as OverlayOptions;
  o.trailSecs = clampSecs(o.trailSecs);
  if (typeof o.bubbleCats !== "object" || !o.bubbleCats) o.bubbleCats = {};
  return o;
}

export function loadOptions(): OverlayOptions {
  try {
    const raw = localStorage.getItem(OPT_KEY);
    if (raw) {
      const parsed = JSON.parse(raw);
      if (parsed && typeof parsed === "object") return migrateOptions(parsed);
    }
  } catch {}
  return { ...DEFAULT_OPTIONS, bubbleCats: {} };
}

export function saveOptions(o: OverlayOptions) {
  try {
    localStorage.setItem(OPT_KEY, JSON.stringify({ ...o, v: OPT_VERSION }));
  } catch {}
}
