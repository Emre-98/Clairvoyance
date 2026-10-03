// Which player controls fit in the controls row at a given width (Player.svelte). Pure, no
// imports, so it's unit-tested with plain Node (tests/controlsfit.unit.test.ts).
//
// When the player gets narrower the controls never overlap: they're collapsed one step at a time
// in this order (the first ones are needed least): the volume slider, the speed, the player
// settings (into the "More controls" menu), the "Input overlay" text (icon only), the second half
// of the time label, the timeline zoom controls, the previous / next event buttons and the frame
// step buttons (into the menu; their shortcuts P / N and , / . keep working).
export const COLLAPSE = ["vol", "rate", "set", "ovtext", "tsub", "zoom", "evnav", "step"] as const;
export type CollapseId = (typeof COLLAPSE)[number];

/** Widths (CSS px) used until the real ones are measured. */
export const ESTIMATE: Record<string, number> = {
  vol: 80,
  rate: 52,
  set: 34,
  ovtext: 84,
  tsub: 170,
  zoom: 160,
  evprev: 34,
  evnext: 34,
  stepb: 34,
  stepf: 34,
  more: 34,
};
/** The gap between the overlay chip's icon and its text. */
const CHIP_GAP = 7;
/** Going back to a wider layout needs this much room to spare (no flapping at the boundary). */
const HYSTERESIS = 6;

/** Width freed in the row by collapsing `id`. */
export function freed(id: CollapseId, w: Record<string, number>, gap: number): number {
  const m = (k: string) => (w[k] > 0 ? w[k] : ESTIMATE[k]);
  switch (id) {
    case "evnav":
      return m("evprev") + m("evnext") + 2 * gap;
    case "step":
      return m("stepb") + m("stepf") + 2 * gap;
    case "tsub":
      return m("tsub");
    case "ovtext":
      return m("ovtext") + CHIP_GAP;
    default:
      return m(id) + gap;
  }
}

/**
 * The collapse level (how many of COLLAPSE are collapsed) for `avail` px, given the row's width
 * `used` now at level `level` and the measured widths `w`: the smallest level that fits.
 */
export function collapseLevel(avail: number, used: number, level: number, w: Record<string, number>, gap: number): number {
  const n = COLLAPSE.length;
  const more = (w.more > 0 ? w.more : ESTIMATE.more) + gap;
  // What's always there (play, the short time, the overlay icon, mute, fullscreen, paddings).
  let base = used;
  for (let i = Math.min(level, n); i < n; i++) base -= freed(COLLAPSE[i], w, gap);
  if (level > 0) base -= more;
  let rest = 0;
  const need: number[] = new Array(n + 1);
  for (let k = n; k >= 0; k--) {
    need[k] = base + rest + (k > 0 ? more : 0);
    if (k > 0) rest += freed(COLLAPSE[k - 1], w, gap);
  }
  for (let k = 0; k <= n; k++) {
    const slack = k < level ? HYSTERESIS : 0;
    if (need[k] + slack <= avail) return k;
  }
  return n;
}
