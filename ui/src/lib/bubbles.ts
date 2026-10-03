// Ability bubbles on the replay's input overlay: when an action key (League: Q W E R, D F, item
// slots, ward) was pressed, a small bubble pops up exactly where the cursor was at that moment,
// on exactly that video frame, and fades out together with the piece of the cursor trail drawn
// at that moment (v1.7: one "Trail & bubbles" time for both).
//
// The list (time, frame, exact position, action, state) is computed once by the backend
// (`input_actions`, cv_core::input::actions) when the overlay is first switched on. This file
// only lays the bubbles out on screen (once per player size / fade time / filter, cached) and
// draws the ones alive at a time. Pure TypeScript with no imports at runtime, so the layout rules
// are unit-tested with plain Node (tests/bubbles.test.ts).

export interface ActionKey {
  id: string;
  label: string;
  icon?: string | null;
  category: string;
  size: number;
  color: string;
}

export interface PressArrays {
  t: number[];
  show: number[];
  x: number[];
  y: number[];
  action: number[];
  /** 0 normal, 1 confirmed (solid), 2 unconfirmed (faded), 3 recast of the same ult (smaller, outlined). */
  state: number[];
  /** Index into `hints` + 1 (0 = none). */
  hint: number[];
  hints: string[];
}

export interface ActionsView {
  actions: ActionKey[];
  categories: { id: string; label: string }[];
  saved_binds: boolean;
  frame_exact: boolean;
  presses: PressArrays;
}

export interface BubbleOptions {
  /** The "Ability bubbles" group. */
  on: boolean;
  /** The trail length (s, 0.25-3): a bubble lives until the trail piece of its moment is gone. */
  fade: number;
  /** Category id -> shown (missing = shown). */
  cats: Record<string, boolean>;
  /** The timeline's "Unconfirmed presses" filter: faded ult presses shown. */
  unconfirmed: boolean;
}

/** The faintest the bubble gets before it goes, together with its trail piece: the same as the
 * trail's oldest part (lib/inputoverlay.ts, 8 buckets: 1/8). */
export const ALPHA_END = 1 / 8;

/** Recast bubbles (later presses of the same ult) are this much smaller. */
export const RECAST_SCALE = 0.72;

/** Animation of a bubble `age` seconds after its frame, with total visible time `total`:
 * pop in (scale, ~100 ms), hold, fade out to the trail's faintest level (`ALPHA_END`), then it
 * is gone (with its trail piece). null = not visible. */
export function animation(age: number, total: number): { scale: number; alpha: number } | null {
  if (age < 0 || age >= total) return null;
  return anim(age, total);
}

function anim(age: number, total: number): { scale: number; alpha: number } {
  const pop = Math.min(0.1, total * 0.3);
  const out = Math.min(Math.max(0.06, total * 0.45), total - pop);
  let scale = 1;
  if (age < pop) {
    // ease-out-back from 0.55 to 1 (a little overshoot = "pop").
    const p = Math.max(0, age) / pop;
    const c = 1.9;
    const e = 1 + (c + 1) * Math.pow(p - 1, 3) + c * Math.pow(p - 1, 2);
    scale = 0.55 + 0.45 * e;
  }
  const alpha = age > total - out ? ALPHA_END + (1 - ALPHA_END) * Math.min(1, Math.max(0, (total - age) / out)) : 1;
  return { scale, alpha };
}

/**
 * When each bubble's piece of the cursor trail is drawn from: `end[i]` is the time of the trail
 * sample whose segment carries the press position, so the bubble is alive at `t` exactly while
 * `end[i] >= t - secs`, the same test the trail uses for that segment (`mt[k] >= t0`).
 * `max` is the running maximum (for the binary search of the first alive bubble).
 */
export interface Ends {
  end: Float64Array;
  max: Float64Array;
}

/**
 * Ends from the cursor samples (lib/inputoverlay.ts InputData: times `mt`, stroke breaks `mb`,
 * the sample rate). Same rule as the press position (cv_core::input::actions::cursor_at): the
 * cursor moving, the press lies on the segment ending at the first sample after it, and that
 * segment's sample time is the end. The cursor at rest (no sample within ~1 period after the
 * press, a stroke break, no sample at all): no trail piece moves under it, so the trail's own
 * clock decides: the press time (it leaves the trail's time window when its moment does).
 */
export function trailEnds(t: ArrayLike<number>, mt: ArrayLike<number>, mb: ArrayLike<number>, rate: number): Ends {
  const n = t.length;
  const end = new Float64Array(n);
  const period = 1 / Math.max(1, rate);
  for (let i = 0; i < n; i++) {
    const tp = t[i];
    let e = tp;
    const k = ub(mt, tp);
    if (k > 0 && k < mt.length && !mb[k] && mt[k] > mt[k - 1]) {
      const start = mt[k] - mt[k - 1] > 1.5 * period ? mt[k] - period : mt[k - 1];
      if (tp > start) e = mt[k];
    }
    end[i] = e;
  }
  return withMax(end);
}

/** Without cursor samples: each bubble's moment is its press time. */
export function pressEnds(t: ArrayLike<number>): Ends {
  return withMax(Float64Array.from(t));
}

function withMax(end: Float64Array): Ends {
  const max = new Float64Array(end.length);
  let m = -Infinity;
  for (let i = 0; i < end.length; i++) max[i] = m = Math.max(m, end[i]);
  return { end, max };
}

/** Index of the first element >= v. */
function lb(a: ArrayLike<number>, v: number): number {
  let lo = 0, hi = a.length;
  while (lo < hi) {
    const m = (lo + hi) >> 1;
    if (a[m] < v) lo = m + 1;
    else hi = m;
  }
  return lo;
}

/** Base bubble radius (CSS px) for a video drawn `h` px high. */
export function baseRadius(h: number): number {
  return Math.max(8, Math.min(17, h * 0.026));
}

export interface Geometry {
  /** Body radius. */
  r: number;
  /** Body centre above the anchor (the tail's length + r). */
  lift: number;
  /** Half size of the letter area (what another bubble mustn't cover). */
  letter: number;
}

export function geometry(base: number, size: number): Geometry {
  const r = base * size;
  return { r, lift: r + Math.max(5, base * 0.45), letter: r * 0.62 };
}

/** Index of the first element > v. */
function ub(a: ArrayLike<number>, v: number): number {
  let lo = 0, hi = a.length;
  while (lo < hi) {
    const m = (lo + hi) >> 1;
    if (a[m] <= v) lo = m + 1;
    else hi = m;
  }
  return lo;
}

export interface Layout {
  /** Anchor (exact cursor position) per press, CSS px. */
  ax: Float32Array;
  ay: Float32Array;
  /** Sideways nudge of the body (the anchor never moves). */
  dx: Float32Array;
  /** Drawn at all with the current filters. */
  on: Uint8Array;
  ms: number;
}

/**
 * Where every bubble goes. Anchors sit exactly on the cursor position. Bubbles of the same
 * action simply overlap at their own positions (Q Q Q Q: no offsets). A bubble that would cover
 * the letter of a bubble of another action still on screen when it appears (newest is drawn on
 * top) moves its body sideways just enough to clear it; its anchor and tail stay on the exact
 * position. Decided when a bubble appears, so it never jumps while it's visible.
 */
export function layout(p: PressArrays, actions: ActionKey[], o: BubbleOptions, base: number, anchor: (i: number) => [number, number], ends: Ends = pressEnds(p.t)): Layout {
  const t0 = performance.now();
  const n = p.t.length;
  const L: Layout = { ax: new Float32Array(n), ay: new Float32Array(n), dx: new Float32Array(n), on: new Uint8Array(n), ms: 0 };
  const geo = actions.map((a) => geometry(base, a.size || 1));
  for (let i = 0; i < n; i++) {
    const a = actions[p.action[i]];
    L.on[i] = a && o.cats[a.category] !== false && (p.state[i] !== 2 || o.unconfirmed) ? 1 : 0;
    if (!L.on[i]) continue;
    const [x, y] = anchor(i);
    L.ax[i] = x;
    L.ay[i] = y;
  }
  const others: number[] = [];
  for (let i = 0; i < n; i++) {
    if (!L.on[i]) continue;
    const gi = geo[p.action[i]];
    const cy = L.ay[i] - gi.lift;
    // Bubbles of other actions alive when this one appears (their trail piece still drawn).
    others.length = 0;
    const birth0 = p.show[i] - o.fade;
    for (let j = i - 1; j >= 0 && ends.max[j] >= birth0; j--) {
      if (!L.on[j] || p.action[j] === p.action[i] || ends.end[j] < birth0) continue;
      const gj = geo[p.action[j]];
      const reachY = gi.r + gj.letter;
      if (Math.abs(cy - (L.ay[j] - gj.lift)) < reachY) others.push(j);
    }
    if (!others.length) continue;
    const hits = (dx: number) => {
      let k = 0;
      const cx = L.ax[i] + dx;
      for (const j of others) if (Math.abs(cx - (L.ax[j] + L.dx[j])) < gi.r + geo[p.action[j]].letter) k++;
      return k;
    };
    if (!hits(0)) continue;
    // Prefer the side away from the most recent bubble it would cover.
    const j0 = others[0];
    const away = L.ax[i] >= L.ax[j0] + L.dx[j0] ? 1 : -1;
    const max = gi.r * 2.6;
    let best = 0, bestHits = Infinity;
    for (let step = 1; step <= max; step += 1) {
      for (const s of [away, -away]) {
        const h = hits(s * step);
        if (h < bestHits) {
          bestHits = h;
          best = s * step;
        }
        if (h === 0) break;
      }
      if (bestHits === 0) break;
    }
    L.dx[i] = best;
  }
  L.ms = performance.now() - t0;
  return L;
}

/** Index range [from, to) that holds every press alive at `t`: shown at or before `t`, and its
 * trail piece (`ends`) still in the trail (>= t - secs). Inside it, check `end[i]` per press. */
export function aliveRange(show: ArrayLike<number>, endMax: ArrayLike<number>, t: number, secs: number): [number, number] {
  return [lb(endMax, t - secs), ub(show, t)];
}

/** Is press `i` drawn at `t`: on or after its frame, and its trail piece not yet gone. */
export function alive(show: ArrayLike<number>, ends: Ends, i: number, t: number, secs: number): boolean {
  return show[i] <= t && ends.end[i] >= t - secs;
}

/** Draws the bubbles alive at `t`: first every tail and anchor dot (a layer under all bodies, so a
 * slanted tail never crosses another bubble's letter), then the bodies oldest first (the newest
 * is on top). */
export function draw(g: CanvasRenderingContext2D, p: PressArrays, actions: ActionKey[], L: Layout, o: BubbleOptions, base: number, t: number, ends: Ends = pressEnds(p.t)): number {
  const t0 = t - o.fade;
  const [from, to] = aliveRange(p.show, ends.max, t, o.fade);
  let drawn = 0;
  for (let pass = 0; pass < 2; pass++) {
    for (let i = from; i < to; i++) {
      // Exactly the trail's test for the segment under the press (mt[k] >= t - secs).
      if (!L.on[i] || ends.end[i] < t0) continue;
      const an = anim(t - p.show[i], ends.end[i] + o.fade - p.show[i]);
      const a = actions[p.action[i]];
      const faded = p.state[i] === 2;
      // A recast of the same ult (command, second part): smaller and outlined.
      const recast = p.state[i] === 3;
      const geo = geometry(base, (a.size || 1) * (recast ? RECAST_SCALE : 1));
      const cx = L.ax[i] + L.dx[i];
      const cy = L.ay[i] - geo.lift * (0.4 + 0.6 * an.scale);
      g.globalAlpha = an.alpha * (faded ? 0.6 : 1);
      if (pass === 0) drawTail(g, a, L.ax[i], L.ay[i], cx, cy, faded);
      else {
        drawBody(g, a, cx, cy, geo.r * an.scale, faded, p.hint[i] ? p.hints[p.hint[i] - 1] : null, recast);
        drawn++;
      }
    }
  }
  g.globalAlpha = 1;
  return drawn;
}

/** Tail from the exact position to the body (slanted when the body was nudged) and the anchor dot. */
function drawTail(g: CanvasRenderingContext2D, a: ActionKey, ax: number, ay: number, cx: number, cy: number, faded: boolean) {
  g.lineCap = "round";
  g.beginPath();
  g.moveTo(ax, ay);
  g.lineTo(cx, cy);
  g.strokeStyle = "rgba(0,0,0,0.6)";
  g.lineWidth = 3.4;
  g.stroke();
  g.strokeStyle = faded ? "rgba(255,255,255,0.75)" : a.color;
  g.lineWidth = 1.6;
  g.stroke();
  g.beginPath();
  g.arc(ax, ay, 2.6, 0, Math.PI * 2);
  g.fillStyle = "rgba(0,0,0,0.75)";
  g.fill();
  g.beginPath();
  g.arc(ax, ay, 1.7, 0, Math.PI * 2);
  g.fillStyle = "#fff";
  g.fill();
}

/** Body: solid colour with a dark outline (bright frames) and a light inner ring (dark frames);
 * unconfirmed ult presses are outlined only. Then the label (the action) or icon, and the hint. */
function drawBody(g: CanvasRenderingContext2D, a: ActionKey, cx: number, cy: number, r: number, faded: boolean, hint: string | null, recast = false) {
  if (recast) {
    // Outlined: a dark disc with the action's colour as a thick ring.
    g.beginPath();
    g.arc(cx, cy, r, 0, Math.PI * 2);
    g.fillStyle = "rgba(15,17,22,0.72)";
    g.fill();
    g.lineWidth = 2;
    g.strokeStyle = "rgba(0,0,0,0.7)";
    g.stroke();
    g.beginPath();
    g.arc(cx, cy, Math.max(1, r - 1.6), 0, Math.PI * 2);
    g.lineWidth = 2.2;
    g.strokeStyle = a.color;
    g.stroke();
    label(g, a, cx, cy, r);
    return;
  }
  g.beginPath();
  g.arc(cx, cy, r, 0, Math.PI * 2);
  g.fillStyle = faded ? "rgba(15,17,22,0.55)" : a.color;
  g.fill();
  g.lineWidth = 2;
  g.strokeStyle = "rgba(0,0,0,0.7)";
  g.stroke();
  g.beginPath();
  g.arc(cx, cy, Math.max(1, r - 2), 0, Math.PI * 2);
  g.lineWidth = faded ? 1.6 : 1;
  g.strokeStyle = faded ? a.color : "rgba(255,255,255,0.55)";
  if (faded) g.setLineDash([3, 2.5]);
  g.stroke();
  if (faded) g.setLineDash([]);
  label(g, a, cx, cy, r);
  if (hint) {
    const fs = Math.max(9, Math.round(r * 0.62));
    g.font = `700 ${fs}px system-ui, "Segoe UI", sans-serif`;
    const w = g.measureText(hint).width + 6;
    const hx = cx + r * 0.62;
    const hy = cy + r * 0.45;
    g.fillStyle = "rgba(10,12,16,0.88)";
    g.beginPath();
    g.roundRect(hx - 1, hy - fs / 2 - 2, w, fs + 4, 4);
    g.fill();
    g.fillStyle = "#fff";
    g.textAlign = "left";
    g.textBaseline = "middle";
    g.fillText(hint, hx + 2, hy + 0.5);
  }
}

/** The action's label (or the ward icon) in the middle of a bubble. */
function label(g: CanvasRenderingContext2D, a: ActionKey, cx: number, cy: number, r: number) {
  if (a.icon === "ward") wardIcon(g, cx, cy, r);
  else {
    const fs = Math.round(r * (a.label.length > 1 ? 0.9 : 1.15));
    g.font = `800 ${fs}px system-ui, "Segoe UI", sans-serif`;
    g.textAlign = "center";
    g.textBaseline = "middle";
    g.lineJoin = "round";
    g.lineWidth = Math.max(2, fs * 0.2);
    g.strokeStyle = "rgba(0,0,0,0.75)";
    g.strokeText(a.label, cx, cy + fs * 0.05);
    g.fillStyle = "#fff";
    g.fillText(a.label, cx, cy + fs * 0.05);
  }
}

/** A ward: an upright eye-shaped lens with a pupil. */
function wardIcon(g: CanvasRenderingContext2D, cx: number, cy: number, r: number) {
  const w = r * 0.55, h = r * 0.78;
  g.beginPath();
  g.moveTo(cx, cy - h);
  g.quadraticCurveTo(cx + w * 1.5, cy, cx, cy + h);
  g.quadraticCurveTo(cx - w * 1.5, cy, cx, cy - h);
  g.closePath();
  g.fillStyle = "#fff";
  g.fill();
  g.lineWidth = 1.5;
  g.strokeStyle = "rgba(0,0,0,0.75)";
  g.stroke();
  g.beginPath();
  g.arc(cx, cy, r * 0.22, 0, Math.PI * 2);
  g.fillStyle = "#1c1917";
  g.fill();
}

/** Bubbles of one replay: the backend's list + the cached layout. */
export class Bubbles {
  private cacheKey = "";
  private cached: Layout | null = null;
  /** Last layout time and bubbles drawn (tests, benchmark). */
  stats = { layoutMs: 0, drawn: 0, base: 0 };
  v: ActionsView;
  /** Each bubble's trail piece (see `trailEnds`); the press times until the samples are set. */
  ends: Ends;
  constructor(v: ActionsView) {
    this.v = v;
    this.ends = pressEnds(v.presses.t);
  }

  /** Ties every bubble to the trail drawn from these cursor samples. */
  setTrail(mt: ArrayLike<number>, mb: ArrayLike<number>, rate: number) {
    this.ends = trailEnds(this.v.presses.t, mt, mb, rate);
    this.cacheKey = "";
  }

  get count() {
    return this.v.presses.t.length;
  }

  /** The current layout (tests and the benchmark read the anchors and nudges). */
  get lastLayout(): Layout | null {
    return this.cached;
  }

  layoutFor(key: string, o: BubbleOptions, base: number, anchor: (i: number) => [number, number]): Layout {
    if (key !== this.cacheKey || !this.cached) {
      this.cached = layout(this.v.presses, this.v.actions, o, base, anchor, this.ends);
      this.cacheKey = key;
      this.stats.layoutMs = this.cached.ms;
    }
    return this.cached;
  }

  draw(g: CanvasRenderingContext2D, key: string, o: BubbleOptions, base: number, t: number, anchor: (i: number) => [number, number]) {
    if (!o.on || !this.count) return;
    const L = this.layoutFor(key, o, base, anchor);
    this.stats.base = base;
    this.stats.drawn = draw(g, this.v.presses, this.v.actions, L, o, base, t, this.ends);
  }
}

/** League's action keys as the backend sends them (browser preview and UI tests). */
export const SAMPLE_ACTIONS: ActionKey[] = [
  { id: "spell1", label: "Q", category: "ability", size: 1, color: "#3b82f6" },
  { id: "spell2", label: "W", category: "ability", size: 1, color: "#22c55e" },
  { id: "spell3", label: "E", category: "ability", size: 1, color: "#f59e0b" },
  { id: "spell4", label: "R", category: "ability", size: 1.18, color: "#a855f7" },
  { id: "summoner1", label: "D", category: "summoner", size: 1.18, color: "#f43f5e" },
  { id: "summoner2", label: "F", category: "summoner", size: 1.18, color: "#14b8a6" },
  { id: "item1", label: "1", category: "item", size: 0.84, color: "#64748b" },
  { id: "ward", label: "Ward", icon: "ward", category: "ward", size: 0.84, color: "#eab308" },
];

/** Frame start times of the UI tests' sample video: 30 fps, WebM timestamps in whole ms. */
export function sampleFrameOf(t: number): number {
  const k = Math.floor(t * 30 + 1e-9);
  // The frame's WebM timestamp (ms-rounded) may be a hair after k/30: then it's frame k-1.
  const at = (i: number) => Math.round((i * 1000) / 30) / 1000;
  if (at(k + 1) <= t) return at(k + 1);
  return at(k) <= t ? at(k) : at(k - 1);
}

/**
 * A synthetic bubble list matching `synthetic()` in inputoverlay.ts (cursor on a circle, one key
 * per second at s + 0.3 cycling Q W E R D F 1), plus a ward every 10 s, E on a rebound key
 * (hint "A"), every other R unconfirmed, and a Q spam burst (every 80 ms from 40.0 to 43.0 s)
 * with a W in the middle (41.53 s).
 */
export function syntheticActions(dur = 150): ActionsView {
  const pos = (t: number) => [0.5 + 0.3 * Math.cos(t), 0.5 + 0.3 * Math.sin(t)];
  const list: { t: number; a: number; st: number; hint: number }[] = [];
  let r = 0;
  for (let s = 1; s < dur; s++) {
    const a = s % 7;
    list.push({ t: s + 0.3, a, st: a === 3 ? (r++ % 2 ? 2 : 1) : 0, hint: a === 2 ? 1 : 0 });
    if (s % 10 === 5) list.push({ t: s + 0.65, a: 7, st: 0, hint: 0 });
  }
  for (let k = 0; k <= 37; k++) list.push({ t: Math.round((40 + k * 0.08) * 1000) / 1000, a: 0, st: 0, hint: 0 });
  list.push({ t: 41.53, a: 1, st: 0, hint: 0 });
  list.sort((x, y) => x.t - y.t);
  const p: PressArrays = { t: [], show: [], x: [], y: [], action: [], state: [], hint: [], hints: ["A"] };
  for (const e of list) {
    const [x, y] = pos(e.t);
    p.t.push(e.t);
    p.show.push(sampleFrameOf(e.t));
    p.x.push(x);
    p.y.push(y);
    p.action.push(e.a);
    p.state.push(e.st);
    p.hint.push(e.hint);
  }
  return {
    actions: SAMPLE_ACTIONS,
    categories: [
      { id: "ability", label: "Abilities" },
      { id: "summoner", label: "Summoners" },
      { id: "item", label: "Items" },
      { id: "ward", label: "Ward" },
    ],
    saved_binds: true,
    frame_exact: true,
    presses: p,
  };
}
