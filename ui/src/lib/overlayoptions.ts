// Options of the replay input overlay (remembered in localStorage) and their migration between
// versions. No runtime imports, so the migration is unit-tested with plain Node
// (tests/bubbles.unit.test.ts).

/** The one "Trail & bubbles" time (s): the trail's length and how long a bubble stays. */
export const SECS_MIN = 0.25;
export const SECS_MAX = 3;
export const SECS_STEP = 0.05;
export const SECS_DEFAULT = 1;

/** "rocket" (default): colored by cursor speed (blue pilot light when slow, orange -> red flame
 * on flicks); "plasma", "toxic", "sunset", "frost": the same, in other colors (palettes in
 * lib/inputoverlay.ts); "classic": the pale yellow trail of v1.4-v1.9. */
export type TrailStyle = "rocket" | "plasma" | "toxic" | "sunset" | "frost" | "classic";

/** The trail styles in menu order: name, menu swatch (slow -> flick colors) and tooltip. */
export const TRAIL_STYLES: { id: TrailStyle; label: string; swatch: string; title: string }[] = [
  { id: "rocket", label: "Rocket", swatch: "linear-gradient(90deg,#3b82f6,#38bdf8 30%,#e83838 60%,#fc781c 80%,#ffde5a)", title: "Blue when the cursor moves slowly, an orange and red flame on fast flicks" },
  { id: "plasma", label: "Plasma", swatch: "linear-gradient(90deg,#14b8a6,#2dd4bf 30%,#8b5cf6 60%,#d946ef 80%,#f472b6)", title: "Teal when slow, violet and pink on fast flicks" },
  { id: "toxic", label: "Toxic", swatch: "linear-gradient(90deg,#22c55e,#4ade80 30%,#84cc16 60%,#a3e635 80%,#facc15)", title: "Green when slow, lime and yellow on fast flicks" },
  { id: "sunset", label: "Sunset", swatch: "linear-gradient(90deg,#7e22ce,#c084fc 30%,#db2777 60%,#f43f5e 75%,#fb923c)", title: "Purple when slow, pink and coral on fast flicks" },
  { id: "frost", label: "Frost", swatch: "linear-gradient(90deg,#64748b,#94a3b8 30%,#0284c7 60%,#38bdf8 80%,#f0f9ff)", title: "Silver when slow, icy blue on fast flicks" },
  { id: "classic", label: "Classic", swatch: "#ffeb99", title: "One pale yellow line" },
];

export interface OverlayOptions {
  trail: boolean;
  trailStyle: TrailStyle;
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
  trailStyle: "rocket",
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
  if (!TRAIL_STYLES.some((s) => s.id === o.trailStyle)) o.trailStyle = DEFAULT_OPTIONS.trailStyle;
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
