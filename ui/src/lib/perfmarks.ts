// Tiny in-app responsiveness meter (Settings > Advanced): how long page switches and timeline
// jumps really take in this window. Costs a couple of timestamps per action.

export type MetricKey = "page_switch" | "seek" | "startup" | "replay_frame" | "overlay_on";

const samples: Record<MetricKey, number[]> = { page_switch: [], seek: [], startup: [], replay_frame: [], overlay_on: [] };

/** When the last page switch started (a replay's "time to first frame" counts from it). */
export let navAt = 0;
export function markNav() {
  navAt = performance.now();
}
const KEEP = 50;

export function record(key: MetricKey, ms: number) {
  const a = samples[key];
  a.push(ms);
  if (a.length > KEEP) a.shift();
}

/** Resolves once the next frame after this call has been painted. */
export function nextPaint(): Promise<number> {
  return new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(() => r(performance.now()))));
}

/** Times from now until the next painted frame. */
export function measurePaint(key: MetricKey) {
  const t0 = performance.now();
  nextPaint().then((t) => record(key, t - t0));
}

export function summary() {
  const out: Record<string, { n: number; median: number | null; p90: number | null; max: number | null }> = {};
  for (const [k, v] of Object.entries(samples)) {
    const s = [...v].sort((a, b) => a - b);
    const q = (p: number) => (s.length ? Math.round(s[Math.min(s.length - 1, Math.floor(p * s.length))]) : null);
    out[k] = { n: s.length, median: q(0.5), p90: q(0.9), max: s.length ? Math.round(s[s.length - 1]) : null };
  }
  return out;
}
