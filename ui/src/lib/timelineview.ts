// Timeline zoom, the time <-> pixel mapping, the ruler and frame-exact stepping. Pure functions
// with no imports at runtime, so they can be unit-tested in Node (tests/timeline.unit.test.ts).

/** The part of the video the timeline shows: `start` .. `start + span` (video seconds). */
export interface View {
  start: number;
  span: number;
}

/** Shortest view: this many frames across the whole timeline (one frame ~ 1/20 of its width). */
export const MIN_FRAMES = 20;

export function fullView(duration: number): View {
  return { start: 0, span: Math.max(duration, 1e-3) };
}

/** Average frame duration (s) from the frame times (1/60 when unknown). */
export function frameDur(frames: ArrayLike<number> | null | undefined): number {
  if (!frames || frames.length < 2) return 1 / 60;
  const n = frames.length - 1;
  const d = (frames[n] - frames[0]) / n;
  return d > 0 && isFinite(d) ? d : 1 / 60;
}

export function minSpan(duration: number, frames: ArrayLike<number> | null | undefined): number {
  return Math.min(Math.max(duration, 1e-3), MIN_FRAMES * frameDur(frames));
}

/** Keeps the view inside the video and between the shortest span and the whole video. */
export function clampView(v: View, duration: number, min: number): View {
  const dur = Math.max(duration, 1e-3);
  const span = Math.min(dur, Math.max(Math.min(min, dur), v.span));
  const start = Math.min(Math.max(0, v.start), dur - span);
  return { start: Math.max(0, start), span };
}

export const isZoomed = (v: View, duration: number) => v.span < Math.max(duration, 1e-3) * 0.999;

export function tToX(t: number, v: View, width: number): number {
  return ((t - v.start) / v.span) * width;
}

export function xToT(x: number, v: View, width: number): number {
  return v.start + (x / Math.max(width, 1)) * v.span;
}

/** Zooms by `factor` (> 1 = in) keeping `anchor` (a time) at the same pixel. */
export function zoomAround(v: View, factor: number, anchor: number, duration: number, min: number): View {
  const span = v.span / factor;
  const c = clampView({ start: 0, span }, duration, min).span;
  const frac = v.span > 0 ? (anchor - v.start) / v.span : 0.5;
  return clampView({ start: anchor - frac * c, span: c }, duration, min);
}

/** Slider position 0 (whole video) .. 1 (shortest span), logarithmic. */
export function zoomLevel(v: View, duration: number, min: number): number {
  const dur = Math.max(duration, 1e-3);
  if (min >= dur) return 0;
  return Math.min(1, Math.max(0, Math.log(dur / v.span) / Math.log(dur / min)));
}

export function spanAt(level: number, duration: number, min: number): number {
  const dur = Math.max(duration, 1e-3);
  if (min >= dur) return dur;
  return dur * Math.pow(min / dur, Math.min(1, Math.max(0, level)));
}

/**
 * Keeps the playhead in view while playing / stepping: when `t` leaves the view (or reaches its
 * last 8 %), the view pages so `t` sits at 8 % from its left edge. Otherwise unchanged.
 */
export function follow(v: View, t: number, duration: number, min: number): View {
  if (t >= v.start && t <= v.start + v.span * 0.92) return v;
  return clampView({ start: t - v.span * 0.08, span: v.span }, duration, min);
}

export function pan(v: View, dt: number, duration: number, min: number): View {
  return clampView({ start: v.start + dt, span: v.span }, duration, min);
}

// ---------- frames ----------

/** Index of the frame on screen at `t`: the last frame starting at or before `t` (0 before the first). */
export function frameIndexAt(frames: ArrayLike<number>, t: number): number {
  let lo = 0;
  let hi = frames.length - 1;
  if (hi < 0) return 0;
  // A hair of tolerance: a seek to a frame's own start time must count as that frame.
  const x = t + 1e-6;
  if (frames[0] > x) return 0;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (frames[mid] <= x) lo = mid;
    else hi = mid - 1;
  }
  return lo;
}

/** Index of the frame whose start is closest to `t` (a presented frame's own timestamp). */
export function frameNearest(frames: ArrayLike<number>, t: number): number {
  const i = frameIndexAt(frames, t);
  if (i + 1 < frames.length && Math.abs(frames[i + 1] - t) < Math.abs(frames[i] - t)) return i + 1;
  return i;
}

/**
 * The time to seek to for frame `i`: the middle of its interval, so rounding in the demuxer or
 * decoder can never land on the frame before or after it.
 */
export function frameSeekTime(frames: ArrayLike<number>, i: number): number {
  const n = frames.length;
  if (!n) return 0;
  const k = Math.min(n - 1, Math.max(0, i));
  const next = k + 1 < n ? frames[k + 1] : frames[k] + frameDur(frames);
  return (frames[k] + next) / 2;
}

/** "m:ss.mmm" (or "h:mm:ss.mmm"), negative times with a minus. */
export function preciseClock(t: number): string {
  const neg = t < 0;
  let ms = Math.round(Math.abs(t) * 1000);
  const h = Math.floor(ms / 3600000);
  ms -= h * 3600000;
  const m = Math.floor(ms / 60000);
  ms -= m * 60000;
  const s = Math.floor(ms / 1000);
  ms -= s * 1000;
  const p2 = (x: number) => String(x).padStart(2, "0");
  const body = h ? `${h}:${p2(m)}:${p2(s)}` : `${m}:${p2(s)}`;
  return `${neg ? "-" : ""}${body}.${String(ms).padStart(3, "0")}`;
}

// ---------- ruler ----------

const STEPS = [0.05, 0.1, 0.2, 0.5, 1, 2, 5, 10, 15, 30, 60, 120, 300, 600, 900, 1800];

export interface Ruler {
  /** Labelled ticks: time + label. */
  major: { t: number; label: string }[];
  /** Small ticks between labels (times). */
  minor: number[];
  /** One tick per frame (indices into the frame list), when frames are at least 3 px apart. */
  frameFrom: number;
  frameTo: number;
  /** Frames are wide enough to number (every `frameLabelEvery` frames). */
  frameLabelEvery: number;
}

function label(t: number, step: number): string {
  const neg = t < -1e-9;
  const a = Math.abs(t);
  const m = Math.floor(a / 60 + 1e-9);
  const s = a - m * 60;
  const dec = step < 0.1 ? 2 : step < 1 ? 1 : 0;
  let ss = s.toFixed(dec);
  if (Number(ss) >= 60) ss = (0).toFixed(dec);
  const [i, f] = ss.split(".");
  return `${neg ? "-" : ""}${m}:${i.padStart(2, "0")}${f ? "." + f : ""}`;
}

/**
 * Ticks for the visible part. `origin` is subtracted for the labels (the game clock: video
 * time - offset). Labels are at least `minLabelPx` apart.
 */
export function ruler(v: View, width: number, frames: ArrayLike<number> | null, origin = 0, minLabelPx = 64): Ruler {
  const pps = width / Math.max(v.span, 1e-9);
  const step = STEPS.find((s) => s * pps >= minLabelPx) ?? STEPS[STEPS.length - 1];
  const sub = step >= 60 ? step / 4 : step >= 10 ? step / 5 : step / (step === 0.2 || step === 2 || step === 0.05 ? 4 : 5);
  const major: { t: number; label: string }[] = [];
  const minor: number[] = [];
  // Ticks sit on round game-clock values.
  const g0 = v.start - origin;
  const g1 = v.start + v.span - origin;
  const first = Math.ceil(g0 / sub - 1e-9) * sub;
  for (let g = first, n = 0; g <= g1 + 1e-9 && n < 4000; g += sub, n++) {
    const r = Math.round(g / sub) * sub;
    const onMajor = Math.abs(r / step - Math.round(r / step)) < 1e-6;
    if (onMajor) major.push({ t: r + origin, label: label(r, step) });
    else minor.push(r + origin);
  }
  let frameFrom = 0;
  let frameTo = -1;
  let frameLabelEvery = 0;
  if (frames && frames.length > 1) {
    const fpx = frameDur(frames) * pps;
    if (fpx >= 3) {
      frameFrom = frameIndexAt(frames, v.start);
      frameTo = Math.min(frames.length - 1, frameIndexAt(frames, v.start + v.span) + 1);
      if (fpx >= 9) frameLabelEvery = [1, 2, 5, 10].find((k) => k * fpx >= 42) ?? 10;
    }
  }
  return { major, minor, frameFrom, frameTo, frameLabelEvery };
}

// ---------- markers ----------

/**
 * Lanes for markers at their x positions (sorted ascending): a marker goes to the first lane
 * whose last marker is at least `gap` px to its left; with every lane taken, to the lane whose
 * last marker is furthest left (they overlap there). Markers outside [-gap, width + gap] get
 * lane -1 (not drawn).
 */
export function lanes(xs: ArrayLike<number>, width: number, gap: number, maxLanes: number, out?: Int8Array): { lane: Int8Array; count: number } {
  const lane = out && out.length >= xs.length ? out : new Int8Array(xs.length);
  const last = new Float64Array(maxLanes).fill(-1e12);
  let count = 1;
  for (let i = 0; i < xs.length; i++) {
    const x = xs[i];
    if (x < -gap || x > width + gap) {
      lane[i] = -1;
      continue;
    }
    let l = -1;
    for (let k = 0; k < maxLanes; k++)
      if (x - last[k] >= gap) {
        l = k;
        break;
      }
    if (l < 0) {
      l = 0;
      for (let k = 1; k < maxLanes; k++) if (last[k] < last[l]) l = k;
    }
    last[l] = x;
    lane[i] = l;
    if (l + 1 > count) count = l + 1;
  }
  return { lane, count };
}
