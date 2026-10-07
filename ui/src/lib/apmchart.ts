// The faint APM chart behind the timeline's markers (bins of video time from 0).

/** The chart's top: the 95th percentile with headroom (one burst doesn't flatten the rest). */
export function apmScale(bins: (number | null)[]): number {
  const v = bins.filter((x): x is number => x != null && x > 0).sort((a, b) => a - b);
  if (!v.length) return 0;
  const p95 = v[Math.min(v.length - 1, Math.floor(v.length * 0.95))];
  return Math.max(30, p95 * 1.15);
}

/** APM of the bin holding video time `t` (null outside the recording or without focus). */
export function apmAt(bins: (number | null)[], bin: number, t: number): number | null {
  if (!(t >= 0)) return null;
  const i = Math.floor(t / bin);
  return i < bins.length ? (bins[i] ?? null) : null;
}
