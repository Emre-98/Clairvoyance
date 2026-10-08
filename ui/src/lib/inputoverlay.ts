// Replay input overlay: parses the recorded mouse/keyboard data (binary from `input_load`,
// layout in cv_core::input::stats::ui_payload) and draws it over the video for a time `t`
// (video seconds). Pure functions + one Overlay class; no Svelte here so it can be tested.
import { Bubbles, baseRadius, type ActionsView, type BubbleOptions } from "./bubbles";
import type { OverlayOptions } from "./overlayoptions";
export { DEFAULT_OPTIONS, loadOptions, saveOptions, migrateOptions, clampSecs, type OverlayOptions } from "./overlayoptions";

export interface InputData {
  rate: number;
  /** Cursor samples: time, x, y (0..1 of the game's client area), 1 = a new stroke starts. */
  mt: Float32Array;
  mx: Float32Array;
  my: Float32Array;
  mb: Uint8Array;
  /** Mouse buttons: time, position, code (button | 0x80 when pressed). */
  ct: Float32Array;
  cx: Float32Array;
  cy: Float32Array;
  cc: Uint8Array;
  /** Keys: time, code (vk | 0x8000 when pressed). */
  kt: Float32Array;
  kc: Uint16Array;
  /** Not focused: [start, end] pairs. */
  gaps: Float32Array;
  /** Window changes: time + (ox, oy, sx, sy, fw, fh) = client inside the captured frame. */
  wt: Float32Array;
  wm: Float32Array;
  heat: { w: number; h: number; v: Float32Array } | null;
  /** Key presses with their release, for the keys strip. */
  keys: KeyBar[];
  /** Mouse button presses with their release, for the keys strip's mouse lane. */
  buttons: KeyBar[];
}

export interface KeyBar {
  vk: number;
  t0: number;
  t1: number;
  label: string;
  lane: number;
  /** Mouse button (1 left, 2 right, 3 middle, 4/5 side), 0 for a key. */
  btn?: number;
}

/** Keys strip label of a mouse button. */
export function btnName(btn: number): string {
  return ({ 1: "LMB", 2: "RMB", 3: "MMB", 4: "M4", 5: "M5" } as Record<number, string>)[btn] ?? `M${btn}`;
}

/** The keys strip lane of the mouse buttons (under the three key lanes). */
const MOUSE_LANE = 3;

export function vkName(vk: number): string {
  if ((vk >= 0x41 && vk <= 0x5a) || (vk >= 0x30 && vk <= 0x39)) return String.fromCharCode(vk);
  if (vk >= 0x70 && vk <= 0x87) return `F${vk - 0x6f}`;
  if (vk >= 0x60 && vk <= 0x69) return `Num${vk - 0x60}`;
  const m: Record<number, string> = {
    0x08: "Bksp", 0x09: "Tab", 0x0d: "Enter", 0x10: "Shift", 0xa0: "Shift", 0xa1: "Shift", 0x11: "Ctrl", 0xa2: "Ctrl", 0xa3: "Ctrl",
    0x12: "Alt", 0xa4: "Alt", 0xa5: "Alt", 0x14: "Caps", 0x1b: "Esc", 0x20: "Space", 0x25: "←", 0x26: "↑", 0x27: "→", 0x28: "↓", 0xc0: "`",
  };
  return m[vk] ?? `#${vk}`;
}

/** Lane 0: abilities (Q W E R), 1: summoners + items (D F 1-7 B), 2: everything else. */
function laneOf(vk: number): number {
  if ([0x51, 0x57, 0x45, 0x52].includes(vk)) return 0;
  if ([0x44, 0x46, 0x42].includes(vk) || (vk >= 0x31 && vk <= 0x37)) return 1;
  return 2;
}

export function parse(buf: ArrayBuffer): InputData {
  const dv = new DataView(buf);
  const magic = String.fromCharCode(dv.getUint8(0), dv.getUint8(1), dv.getUint8(2), dv.getUint8(3));
  if (magic !== "CVIV") throw new Error("not input overlay data");
  const u = (i: number) => dv.getUint32(i, true);
  if (u(4) !== 2) throw new Error("input overlay data: unknown version");
  const [nm, nc, nk, ng, nw, hw, hh, rate, nb] = [u(8), u(12), u(16), u(20), u(24), u(28), u(32), u(36), u(40)];
  let o = 44;
  const pad = (n: number) => (n + 3) & ~3;
  const f32 = (n: number) => {
    const a = new Float32Array(buf, o, n);
    o += n * 4;
    return a;
  };
  const u8 = (n: number) => {
    const a = new Uint8Array(buf, o, n);
    o += pad(n);
    return a;
  };
  const mt = f32(nm);
  // Positions arrive as i16 (1/16384 of the client size): half the bytes of f32.
  const q = (n: number) => {
    const a = new Int16Array(buf, o, n);
    o += n * 2;
    const f = new Float32Array(n);
    for (let i = 0; i < n; i++) f[i] = a[i] * (1 / 16384);
    return f;
  };
  const mx = q(nm), my = q(nm);
  o = (o + 3) & ~3;
  const mb = new Uint8Array(nm);
  const br = new Uint32Array(buf, o, nb);
  o += nb * 4;
  for (const i of br) if (i < nm) mb[i] = 1;
  const ct = f32(nc), cx = f32(nc), cy = f32(nc), cc = u8(nc);
  const kt = f32(nk);
  const kc = new Uint16Array(buf, o, nk);
  o += pad(nk * 2);
  const gaps = f32(ng * 2);
  const wraw = f32(nw * 7);
  const wt = new Float32Array(nw);
  const wm = new Float32Array(nw * 6);
  for (let i = 0; i < nw; i++) {
    wt[i] = wraw[i * 7];
    for (let j = 0; j < 6; j++) wm[i * 6 + j] = wraw[i * 7 + 1 + j];
  }
  const heat = hw * hh > 0 ? { w: hw, h: hh, v: f32(hw * hh) } : null;
  // Key bars: each press with its release (or a short default when the release wasn't recorded).
  const keys: KeyBar[] = [];
  const open = new Map<number, KeyBar>();
  for (let i = 0; i < nk; i++) {
    const vk = kc[i] & 0xff;
    const down = (kc[i] & 0x8000) !== 0;
    if (down) {
      const b = { vk, t0: kt[i], t1: kt[i] + 0.12, label: vkName(vk), lane: laneOf(vk) };
      const prev = open.get(vk);
      if (prev) prev.t1 = Math.min(prev.t1, b.t0);
      open.set(vk, b);
      keys.push(b);
    } else {
      const b = open.get(vk);
      if (b) {
        b.t1 = Math.min(Math.max(kt[i], b.t0 + 0.05), b.t0 + 3);
        open.delete(vk);
      }
    }
  }
  // Mouse button bars, the same way (left / right clicks in the strip's mouse lane).
  const buttons: KeyBar[] = [];
  const held = new Map<number, KeyBar>();
  for (let i = 0; i < nc; i++) {
    const btn = cc[i] & 0x7f;
    if (cc[i] & 0x80) {
      const b = { vk: 0, btn, t0: ct[i], t1: ct[i] + 0.12, label: btnName(btn), lane: MOUSE_LANE };
      const prev = held.get(btn);
      if (prev) prev.t1 = Math.min(prev.t1, b.t0);
      held.set(btn, b);
      buttons.push(b);
    } else {
      const b = held.get(btn);
      if (b) {
        b.t1 = Math.min(Math.max(ct[i], b.t0 + 0.05), b.t0 + 3);
        held.delete(btn);
      }
    }
  }
  return { rate, mt, mx, my, mb, ct, cx, cy, cc, kt, kc, gaps, wt, wm, heat, keys, buttons };
}

/** Index of the first element >= v (sorted array). */
export function lowerBound(a: ArrayLike<number>, v: number): number {
  let lo = 0;
  let hi = a.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (a[mid] < v) lo = mid + 1;
    else hi = mid;
  }
  return lo;
}

/** Index of the first element > v. */
export function upperBound(a: ArrayLike<number>, v: number): number {
  let lo = 0;
  let hi = a.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (a[mid] <= v) lo = mid + 1;
    else hi = mid;
  }
  return lo;
}

export function inGap(d: InputData, t: number): boolean {
  for (let i = 0; i < d.gaps.length; i += 2) if (t >= d.gaps[i] && t < d.gaps[i + 1]) return true;
  return false;
}

/**
 * Where the video content is drawn inside the element: object-fit "contain" (whole video, bars)
 * or "cover" (fills the element, the overflow is cropped; the rect then extends past it).
 */
export function videoRect(boxW: number, boxH: number, vw: number, vh: number, fit: "contain" | "cover" = "contain") {
  if (!vw || !vh) return { x: 0, y: 0, w: boxW, h: boxH };
  const s = fit === "cover" ? Math.max(boxW / vw, boxH / vh) : Math.min(boxW / vw, boxH / vh);
  const w = vw * s;
  const h = vh * s;
  return { x: (boxW - w) / 2, y: (boxH - h) / 2, w, h };
}

/**
 * Maps client-area coordinates (0..1) at time t to pixels on the canvas: client -> captured
 * frame (window record) -> the recorder's letterbox inside the video -> the video's place in
 * the element.
 */
export function mapper(d: InputData, t: number, rect: { x: number; y: number; w: number; h: number }, vw: number, vh: number) {
  let ox = 0, oy = 0, sx = 1, sy = 1, fw = vw || 16, fh = vh || 9;
  const i = upperBound(d.wt, t) - 1;
  const k = i >= 0 ? i : d.wt.length ? 0 : -1;
  if (k >= 0) {
    ox = d.wm[k * 6];
    oy = d.wm[k * 6 + 1];
    sx = d.wm[k * 6 + 2];
    sy = d.wm[k * 6 + 3];
    fw = d.wm[k * 6 + 4];
    fh = d.wm[k * 6 + 5];
  }
  // The recorder letterboxes the captured frame into the output size.
  const W = vw || fw;
  const H = vh || fh;
  const s = Math.min(W / fw, H / fh);
  const lx = (1 - (fw * s) / W) / 2;
  const ly = (1 - (fh * s) / H) / 2;
  const cw = (fw * s) / W;
  const ch = (fh * s) / H;
  const ax = rect.x + rect.w * (lx + cw * ox);
  const ay = rect.y + rect.h * (ly + ch * oy);
  const bx = rect.w * cw * sx;
  const by = rect.h * ch * sy;
  return { x: (nx: number) => ax + bx * nx, y: (ny: number) => ay + by * ny, w: bx, h: by, ox: ax, oy: ay };
}

type Mapper = ReturnType<typeof mapper>;

/** Cursor dwell heatmap for [a, b] (same rule as the Rust one: dwell per sample, max 0.5 s). */
export function heatmapFor(d: InputData, a: number, b: number, w = 96, h = 54): Float32Array {
  const v = new Float32Array(w * h);
  const start = lowerBound(d.mt, a);
  for (let i = start; i < d.mt.length && d.mt[i] <= b; i++) {
    const nextT = i + 1 < d.mt.length && !d.mb[i + 1] ? d.mt[i + 1] : d.mt[i] + 0.05;
    const dwell = Math.min(Math.max(Math.min(nextT, b) - d.mt[i], 0), 0.5);
    const x = d.mx[i];
    const y = d.my[i];
    if (dwell <= 0 || x < 0 || x >= 1 || y < 0 || y >= 1 || inGap(d, d.mt[i])) continue;
    v[Math.min(h - 1, Math.floor(y * h)) * w + Math.min(w - 1, Math.floor(x * w))] += dwell;
  }
  let max = 0;
  for (const x of v) max = Math.max(max, x);
  if (max > 0) for (let i = 0; i < v.length; i++) v[i] /= max;
  return v;
}

/** One-hue sequential ramp (warm amber), light -> dark, with alpha growing with the value. */
/** Two box-blur passes (≈ gaussian) so the cells read as a smooth field, then re-normalized. */
export function blurHeat(src: Float32Array, w: number, h: number, r = 1): Float32Array {
  let a = Float32Array.from(src);
  const tmp = new Float32Array(a.length);
  for (let pass = 0; pass < 2; pass++) {
    for (let y = 0; y < h; y++)
      for (let x = 0; x < w; x++) {
        let s = 0, n = 0;
        for (let k = -r; k <= r; k++) {
          const xx = x + k;
          if (xx >= 0 && xx < w) {
            s += a[y * w + xx];
            n++;
          }
        }
        tmp[y * w + x] = s / n;
      }
    for (let y = 0; y < h; y++)
      for (let x = 0; x < w; x++) {
        let s = 0, n = 0;
        for (let k = -r; k <= r; k++) {
          const yy = y + k;
          if (yy >= 0 && yy < h) {
            s += tmp[yy * w + x];
            n++;
          }
        }
        a[y * w + x] = s / n;
      }
  }
  let max = 0;
  for (const x of a) max = Math.max(max, x);
  if (max > 0) a = a.map((x) => x / max);
  return a;
}

function heatImage(raw: Float32Array, w: number, h: number): HTMLCanvasElement {
  const v = blurHeat(raw, w, h);
  const c = document.createElement("canvas");
  c.width = w;
  c.height = h;
  const g = c.getContext("2d")!;
  const img = g.createImageData(w, h);
  for (let i = 0; i < v.length; i++) {
    const x = Math.sqrt(v[i]); // spread the low end
    if (x <= 0.06) continue;
    // #ffd8a8 (light) -> #e8590c (dark)
    img.data[i * 4] = 255 + (232 - 255) * x;
    img.data[i * 4 + 1] = 216 + (89 - 216) * x;
    img.data[i * 4 + 2] = 168 + (12 - 168) * x;
    img.data[i * 4 + 3] = 40 + 150 * x;
  }
  g.putImageData(img, 0, 0);
  return c;
}

const BTN_COLOR: Record<number, string> = { 1: "#4dabf7", 2: "#ff6b6b", 3: "#ffd43b", 4: "#b197fc", 5: "#b197fc" };
const TRAIL_BUCKETS = 8;
/** The rocket trail's points are at least this far apart (CSS px). */
const ROCKET_STEP = 3;

type Rgba = [number, number, number, number];
/** Pilot light along the trail, tail (0) -> tip (1). */
const SLOW_STOPS: [number, Rgba][] = [
  [0, [30, 58, 138, 0.12]],
  [0.5, [59, 130, 246, 0.7]],
  [0.85, [56, 189, 248, 1]],
  [1, [224, 242, 254, 1]],
];
/** Flame along the trail: dark embers, deep red, red, orange, yellow, white-hot tip. */
const FAST_STOPS: [number, Rgba][] = [
  [0, [150, 32, 32, 0.15]],
  [0.3, [201, 42, 42, 0.8]],
  [0.55, [240, 62, 62, 1]],
  [0.75, [253, 126, 20, 1]],
  [0.9, [255, 212, 59, 1]],
  [1, [255, 251, 230, 1]],
];
const GLOW_SLOW: Rgba = [56, 189, 248, 1];
const GLOW_FAST: Rgba = [240, 72, 50, 1];

function mix(a: Rgba, b: Rgba, k: number): Rgba {
  return [a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k, a[2] + (b[2] - a[2]) * k, a[3] + (b[3] - a[3]) * k];
}

export function ramp(stops: [number, Rgba][], f: number): Rgba {
  if (f <= stops[0][0]) return stops[0][1];
  for (let i = 1; i < stops.length; i++) {
    const [p, c] = stops[i];
    if (f <= p) {
      const [p0, c0] = stops[i - 1];
      return mix(c0, c, (f - p0) / (p - p0));
    }
  }
  return stops[stops.length - 1][1];
}

function rgba(c: Rgba, alpha: number): string {
  return `rgba(${Math.round(c[0])},${Math.round(c[1])},${Math.round(c[2])},${Math.min(1, c[3] * alpha).toFixed(3)})`;
}

/** Cursor speed (screen heights per second, 16:9) at which the trail starts to heat up / is a
 * full flame. */
export const HEAT_COOL = 0.34;
export const HEAT_HOT = 1.65;
/** How fast a flame cools back to the pilot light (s, exponential). */
export const HEAT_COOLDOWN = 0.18;

/**
 * Heat of each cursor sample, 0 (slow, pilot light) .. 1 (flick, full flame): the cursor speed
 * over the last 50 ms (not across stroke breaks), eased between HEAT_COOL and HEAT_HOT, cooling
 * down gradually after a flick (HEAT_COOLDOWN).
 */
export function trailHeat(mt: ArrayLike<number>, mx: ArrayLike<number>, my: ArrayLike<number>, mb: ArrayLike<number>, win = 0.05): Float32Array {
  const n = mt.length;
  const out = new Float32Array(n);
  let j = 0; // first sample of the window
  let start = 0; // first sample of the current stroke
  // Running path length from the stroke start, so the window length is a difference.
  const len = new Float64Array(n);
  for (let i = 0; i < n; i++) {
    if (mb[i] || i === 0) {
      start = i;
      len[i] = 0;
    } else {
      const dx = (mx[i] - mx[i - 1]) * (16 / 9);
      const dy = my[i] - my[i - 1];
      len[i] = len[i - 1] + Math.hypot(dx, dy);
    }
    if (j < start) j = start;
    while (j < i && mt[i] - mt[j] > win) j++;
    const dt = Math.max(mt[i] - mt[j], 1 / 120);
    const v = (len[i] - len[j]) / dt;
    const k = Math.min(1, Math.max(0, (v - HEAT_COOL) / (HEAT_HOT - HEAT_COOL)));
    let hv = k * k * (3 - 2 * k);
    // After a flick the flame cools down gradually (no hard red -> blue switch).
    if (i > start) hv = Math.max(hv, out[i - 1] * Math.exp(-(mt[i] - mt[i - 1]) / HEAT_COOLDOWN));
    out[i] = hv;
  }
  return out;
}
const STRIP_BEFORE = 2.5;
const STRIP_AFTER = 0.5;

/** Draws the overlay; keeps the heatmap images it made (computed once per range). */
export class Overlay {
  private heatCache = new Map<string, HTMLCanvasElement>();
  /** Ability bubbles (loaded next to the recording; null until they arrive / none). */
  bubbles: Bubbles | null = null;
  /** Grows when something drawn changes without the time changing (bubbles arrived). */
  version = 0;
  constructor(public d: InputData) {}

  setBubbles(v: ActionsView | null) {
    this.bubbles = v && v.presses.t.length ? new Bubbles(v) : null;
    // Every bubble goes with the trail piece of its moment.
    this.bubbles?.setTrail(this.d.mt, this.d.mb, this.d.rate);
    this.version++;
  }

  /**
   * `fit`: how the player places the video (CSS object-fit). `insetBottom`: CSS px at the bottom
   * covered by the player's own controls (fullscreen panel): the keys strip stays above them.
   */
  draw(g: CanvasRenderingContext2D, cssW: number, cssH: number, dpr: number, t: number, vw: number, vh: number, o: OverlayOptions, heatRange: [number, number] | null, showUnconfirmed = false, fit: "contain" | "cover" = "contain", insetBottom = 0) {
    const d = this.d;
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    g.clearRect(0, 0, cssW, cssH);
    const rect = videoRect(cssW, cssH, vw, vh, fit);
    const m = mapper(d, t, rect, vw, vh);
    // The visible part of the video (with "cover" the rect is larger than the element).
    const vis = {
      x: Math.max(0, rect.x),
      y: Math.max(0, rect.y),
      w: Math.min(cssW, rect.x + rect.w) - Math.max(0, rect.x),
      h: Math.min(cssH, rect.y + rect.h) - Math.max(0, rect.y),
    };
    g.save();
    g.beginPath();
    g.rect(vis.x, vis.y, vis.w, vis.h);
    g.clip();

    if (o.heat) {
      const range: [number, number] | null = o.heatRange === "range" ? heatRange : null;
      const key = range ? `${range[0].toFixed(2)}-${range[1].toFixed(2)}` : "game";
      let img = this.heatCache.get(key);
      if (!img) {
        const hw = d.heat?.w ?? 96;
        const hh = d.heat?.h ?? 54;
        const v = range || !d.heat ? heatmapFor(d, range ? range[0] : 0, range ? range[1] : Infinity, hw, hh) : d.heat.v;
        img = heatImage(v, hw, hh);
        this.heatCache.set(key, img);
      }
      g.imageSmoothingEnabled = true;
      g.imageSmoothingQuality = "high";
      g.drawImage(img, m.ox, m.oy, m.w, m.h);
    }

    const lw = Math.max(1.5, Math.min(rect.w, rect.h) / 360);
    const hi = upperBound(d.mt, t) - 1;
    const gap = inGap(d, t);

    if (o.trail && hi > 0) {
      if (o.trailStyle === "classic") this.drawClassicTrail(g, m, t, o.trailSecs, hi, lw);
      else this.drawRocketTrail(g, m, t, o.trailSecs, hi, lw);
    }

    if (o.clicks) {
      const s = lowerBound(d.ct, t - 0.5);
      const e = upperBound(d.ct, t);
      for (let i = s; i < e; i++) {
        if (!(d.cc[i] & 0x80)) continue;
        const p = (t - d.ct[i]) / 0.5;
        const btn = d.cc[i] & 0x7f;
        const x = m.x(d.cx[i]);
        const y = m.y(d.cy[i]);
        const r = lw * (3 + 14 * p);
        g.globalAlpha = 1 - p;
        g.beginPath();
        g.arc(x, y, r, 0, Math.PI * 2);
        g.lineWidth = lw * 1.6;
        g.strokeStyle = "rgba(0,0,0,0.5)";
        g.stroke();
        g.lineWidth = lw * 1.1;
        g.strokeStyle = BTN_COLOR[btn] ?? "#fff";
        g.stroke();
        if (p < 0.25) {
          g.beginPath();
          g.arc(x, y, lw * 2.2, 0, Math.PI * 2);
          g.fillStyle = BTN_COLOR[btn] ?? "#fff";
          g.fill();
        }
        g.globalAlpha = 1;
      }
    }

    if (o.dot && hi >= 0 && !gap) {
      const x = m.x(d.mx[hi]);
      const y = m.y(d.my[hi]);
      g.beginPath();
      g.arc(x, y, lw * 2.4, 0, Math.PI * 2);
      g.fillStyle = "#fff";
      g.fill();
      g.lineWidth = lw * 0.9;
      g.strokeStyle = "rgba(0,0,0,0.7)";
      g.stroke();
    }
    if (o.bubbles && this.bubbles) {
      // One time for the trail and the bubbles (also with the trail switched off: the bubbles
      // keep the time the trail would have).
      const bo: BubbleOptions = { on: true, fade: o.trailSecs, cats: o.bubbleCats ?? {}, unconfirmed: showUnconfirmed };
      const base = baseRadius(rect.h);
      const p = this.bubbles.v.presses;
      // Each anchor with the window/letterbox mapping of its own moment (it never follows the
      // cursor or a later window move).
      const anchor = (i: number): [number, number] => {
        const mm = mapper(d, p.t[i], rect, vw, vh);
        return [mm.x(p.x[i]), mm.y(p.y[i])];
      };
      const cats = Object.keys(bo.cats).filter((k) => bo.cats[k] === false).sort().join(",");
      const key = `${bo.fade}|${rect.x.toFixed(1)},${rect.y.toFixed(1)},${rect.w.toFixed(1)},${rect.h.toFixed(1)}|${vw}x${vh}|${cats}|${bo.unconfirmed}`;
      this.bubbles.draw(g, key, bo, base, t, anchor);
    }
    g.restore();

    if (o.keys) {
      const bottom = Math.min(vis.y + vis.h, cssH - insetBottom);
      this.drawKeys(g, { x: vis.x, y: vis.y, w: vis.w, h: Math.max(0, bottom - vis.y) }, t, fit === "contain" ? rect.h : vis.h);
    }
  }

  /** The v1.4 trail: pale yellow, older segments fainter. */
  private drawClassicTrail(g: CanvasRenderingContext2D, m: Mapper, t: number, secs: number, hi: number, lw: number) {
    const d = this.d;
    const t0 = t - secs;
    const lo = Math.max(1, lowerBound(d.mt, t0));
    g.lineCap = "round";
    g.lineJoin = "round";
    // Fade: older segments in fainter buckets (one stroke per bucket keeps it fast).
    for (let b = 0; b < TRAIL_BUCKETS; b++) {
      const a0 = t0 + (secs * b) / TRAIL_BUCKETS;
      const a1 = t0 + (secs * (b + 1)) / TRAIL_BUCKETS;
      const s = Math.max(lo, lowerBound(d.mt, a0));
      const e = Math.min(hi, upperBound(d.mt, a1) - 1);
      if (e < s) continue;
      g.beginPath();
      for (let i = s; i <= e; i++) {
        if (d.mb[i]) continue; // a new stroke starts here (focus came back, window moved)
        g.moveTo(m.x(d.mx[i - 1]), m.y(d.my[i - 1]));
        g.lineTo(m.x(d.mx[i]), m.y(d.my[i]));
      }
      const f = (b + 1) / TRAIL_BUCKETS;
      g.strokeStyle = `rgba(0,0,0,${0.35 * f})`;
      g.lineWidth = lw * (1 + f) + 2;
      g.stroke();
      g.strokeStyle = `rgba(255,236,153,${0.95 * f})`;
      g.lineWidth = lw * (1 + f);
      g.stroke();
    }
  }

  /** How hot each cursor sample is (0 slow .. 1 fast flick), computed once. */
  private speedHeat: Float32Array | null = null;
  private sampleHeat(): Float32Array {
    if (!this.speedHeat) this.speedHeat = trailHeat(this.d.mt, this.d.mx, this.d.my, this.d.mb);
    return this.speedHeat;
  }

  /**
   * The rocket exhaust trail: a thin streak that tapers to a hairline at its tail, colored by
   * its age and by how fast the cursor moved there. Slow moves burn like a blue pilot light;
   * flicks ignite into a red-orange flame with a white-hot core that burns out faster than the
   * rest of the trail, so a flick reads as a quick streak. The cursor path is thinned to points
   * a few px apart and drawn as a smooth curve through them (quadratic pieces between the
   * midpoints), each piece with its own width and color.
   */
  private drawRocketTrail(g: CanvasRenderingContext2D, m: Mapper, t: number, secs: number, hi: number, lw: number) {
    const d = this.d;
    const heat = this.sampleHeat();
    const t0 = t - secs;
    const lo = Math.max(1, lowerBound(d.mt, t0));
    if (hi < lo) return;
    // Points at least ROCKET_STEP px apart (the newest one always), split at stroke breaks.
    const px: number[] = [], py: number[] = [], pt: number[] = [], ph: number[] = [], brk: number[] = [];
    let lx = NaN, ly = NaN;
    for (let i = lo - 1; i <= hi; i++) {
      const x = m.x(d.mx[i]);
      const y = m.y(d.my[i]);
      const cut = i >= lo && d.mb[i] === 1;
      if (!cut && i !== hi && Math.hypot(x - lx, y - ly) < ROCKET_STEP) continue;
      px.push(x);
      py.push(y);
      pt.push(d.mt[i]);
      ph.push(heat[i]);
      brk.push(cut || i === lo - 1 ? 1 : 0);
      lx = x;
      ly = y;
    }
    const n = px.length;
    // Smooth out the mouse's pixel steps and jitter: each inner point moves toward its
    // neighbors (1-2-1, three times); stroke ends and the newest point (the cursor) stay put.
    for (let pass = 0; pass < 3; pass++) {
      const sx = px.slice(), sy = py.slice();
      for (let k = 1; k < n - 1; k++) {
        if (brk[k] || brk[k + 1]) continue;
        px[k] = (sx[k - 1] + 2 * sx[k] + sx[k + 1]) / 4;
        py[k] = (sy[k - 1] + 2 * sy[k] + sy[k + 1]) / 4;
      }
    }
    g.lineCap = "butt";
    g.lineJoin = "round";
    // Piece k runs from the midpoint before point k to the midpoint after it (the ends of a
    // stroke from the point itself), curving through point k: neighbors share their ends and
    // tangents, so the streak is smooth and the pieces meet without overlaps.
    const piece = (k: number) => {
      const first = brk[k] === 1 || k === 0;
      const last = k === n - 1 || brk[k + 1] === 1;
      g.beginPath();
      if (first) g.moveTo(px[k], py[k]);
      else g.moveTo((px[k - 1] + px[k]) / 2, (py[k - 1] + py[k]) / 2);
      if (last) g.lineTo(px[k], py[k]);
      else g.quadraticCurveTo(px[k], py[k], (px[k] + px[k + 1]) / 2, (py[k] + py[k + 1]) / 2);
    };
    for (let pass = 0; pass < 3; pass++) {
      for (let k = 0; k < n; k++) {
        const h = ph[k];
        const age = t - pt[k];
        // Fades over the whole trail time, the same for pilot light and flame (a flame that
        // left earlier would leave a gap between older and newer blue on long trails).
        const f = 1 - age / secs;
        if (f <= 0) continue;
        // A flame's colors cool on a faster clock (white-hot -> red -> embers), so a flick
        // still reads as a quick streak.
        const fh = Math.max(0, 1 - age / (secs * (1 - 0.5 * h)));
        // Full width and opacity over the newest 40% of its life, then tapering to a hairline
        // and fading out completely. The same for pilot light and flame (only the colors differ), so where
        // a cooling flame meets the pilot light there is no step in width or opacity, even when
        // the cursor slows down and a lot of time is packed into a few pixels.
        const u = Math.min(1, f / 0.6);
        const tp = u * u * (3 - 2 * u);
        const w = lw * (0.25 + 1.05 * tp);
        piece(k);
        if (pass === 0) {
          // A narrow halo: sky blue around the pilot light, red-orange around a flame.
          g.strokeStyle = rgba(mix(GLOW_SLOW, GLOW_FAST, h), (0.16 + 0.12 * h) * tp * tp);
          g.lineWidth = w * 2.6 + 1;
          g.stroke();
        } else if (pass === 1) {
          const c = mix(ramp(SLOW_STOPS, f), ramp(FAST_STOPS, fh * fh * fh), h);
          c[3] = 1;
          // Fades out completely at the tail, so its end flows away instead of dropping off.
          g.strokeStyle = rgba(c, tp);
          g.lineWidth = w;
          g.stroke();
        } else {
          // The white-hot core right at the cursor.
          const fc = f + (fh - f) * h;
          const k0 = fc > 0.8 ? (fc - 0.8) / 0.2 : 0;
          if (k0 <= 0) continue;
          g.strokeStyle = rgba(mix([224, 242, 254, 1], [255, 251, 230, 1], h), k0 * (0.6 + 0.4 * h));
          g.lineWidth = Math.max(0.75, w * 0.4);
          g.stroke();
        }
      }
    }
  }

  /** A strip of the keys and mouse buttons pressed from 2.5 s before to 0.5 s after `t`, one
   * lane per key group plus one for the mouse. */
  private drawKeys(g: CanvasRenderingContext2D, rect: { x: number; y: number; w: number; h: number }, t: number, sizeH = rect.h) {
    const W = Math.max(220, Math.min(rect.w * 0.36, 520));
    const laneH = Math.max(15, Math.min(22, sizeH / 30));
    const H = laneH * (MOUSE_LANE + 1) + 10;
    const x0 = rect.x + 12;
    const y0 = rect.y + rect.h - H - 12;
    const span = STRIP_BEFORE + STRIP_AFTER;
    const px = (tt: number) => x0 + ((tt - (t - STRIP_BEFORE)) / span) * W;
    g.save();
    g.fillStyle = "rgba(10,12,16,0.62)";
    roundRect(g, x0 - 6, y0 - 5, W + 12, H + 10, 8);
    g.fill();
    g.beginPath();
    g.rect(x0, y0, W, H);
    g.clip();
    g.font = `600 ${Math.round(laneH * 0.62)}px system-ui, sans-serif`;
    g.textBaseline = "middle";
    for (const bars of [this.d.keys, this.d.buttons]) {
      // Bars are sorted by press time: find the ones overlapping the window.
      const from = t - STRIP_BEFORE - 3;
      let lo = 0;
      let hi = bars.length;
      while (lo < hi) {
        const mid = (lo + hi) >> 1;
        if (bars[mid].t0 < from) lo = mid + 1;
        else hi = mid;
      }
      for (let i = lo; i < bars.length && bars[i].t0 <= t + STRIP_AFTER; i++) {
        const k = bars[i];
        if (k.t1 < t - STRIP_BEFORE) continue;
        const a = px(k.t0);
        const w = Math.max(px(k.t1) - a, g.measureText(k.label).width + 10);
        const y = y0 + 5 + k.lane * laneH;
        const held = k.t0 <= t && t <= k.t1;
        const future = k.t0 > t;
        // A held mouse button takes its click ring's color (left blue, right red).
        const heldFill = k.btn ? (BTN_COLOR[k.btn] ?? "#fff") : "rgba(255,236,153,0.95)";
        g.fillStyle = held ? heldFill : future ? "rgba(255,255,255,0.18)" : "rgba(255,255,255,0.32)";
        roundRect(g, a, y, w, laneH - 3, 4);
        g.fill();
        if (k.btn && !held) {
          // Not held: a thin edge in the button's color tells left from right at a glance.
          g.fillStyle = BTN_COLOR[k.btn] ?? "#fff";
          g.fillRect(a, y + 2, 2, laneH - 7);
        }
        g.fillStyle = held ? "#1b1b1b" : "rgba(255,255,255,0.92)";
        g.fillText(k.label, a + 5, y + (laneH - 3) / 2 + 0.5);
      }
    }
    // Now.
    const nx = px(t);
    g.fillStyle = "rgba(255,255,255,0.85)";
    g.fillRect(nx - 1, y0, 2, H);
    g.restore();
  }
}

function roundRect(g: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, r: number) {
  g.beginPath();
  g.moveTo(x + r, y);
  g.arcTo(x + w, y, x + w, y + h, r);
  g.arcTo(x + w, y + h, x, y + h, r);
  g.arcTo(x, y + h, x, y, r);
  g.arcTo(x, y, x + w, y, r);
  g.closePath();
}

/** A synthetic recording (browser preview and UI tests): a circle path, clicks every second. */
export function synthetic(dur = 120, rate = 250): ArrayBuffer {
  const n = Math.floor(dur * rate);
  const clicks: [number, number, number, number][] = [];
  const keys: [number, number][] = [];
  const pos = (t: number) => [0.5 + 0.3 * Math.cos(t), 0.5 + 0.3 * Math.sin(t)];
  for (let s = 1; s < dur; s++) {
    const [x, y] = pos(s);
    clicks.push([s, x, y, (s % 3 === 0 ? 1 : 2) | 0x80]);
    clicks.push([s + 0.06, x, y, s % 3 === 0 ? 1 : 2]);
    const vk = [0x51, 0x57, 0x45, 0x52, 0x44, 0x46, 0x31][s % 7];
    keys.push([s + 0.3, vk | 0x8000]);
    keys.push([s + 0.42, vk]);
  }
  const pad = (k: number) => (k + 3) & ~3;
  const size = 44 + n * 4 + pad(n * 4) + 4 + clicks.length * 12 + pad(clicks.length) + keys.length * 4 + pad(keys.length * 2) + 0 + 7 * 4;
  const buf = new ArrayBuffer(size);
  const dv = new DataView(buf);
  [0x43, 0x56, 0x49, 0x56].forEach((c, i) => dv.setUint8(i, c));
  [2, n, clicks.length, keys.length, 0, 1, 0, 0, rate, 1].forEach((v, i) => dv.setUint32(4 + i * 4, v, true));
  let o = 44;
  const f = (v: number) => {
    dv.setFloat32(o, v, true);
    o += 4;
  };
  for (let i = 0; i < n; i++) f(i / rate);
  for (let i = 0; i < n; i++) dv.setInt16(o + i * 2, Math.round(pos(i / rate)[0] * 16384), true);
  o += n * 2;
  for (let i = 0; i < n; i++) dv.setInt16(o + i * 2, Math.round(pos(i / rate)[1] * 16384), true);
  o += n * 2;
  o = (o + 3) & ~3;
  dv.setUint32(o, 0, true); // one stroke break: the first sample
  o += 4;
  for (const c of clicks) f(c[0]);
  for (const c of clicks) f(c[1]);
  for (const c of clicks) f(c[2]);
  clicks.forEach((c, i) => dv.setUint8(o + i, c[3]));
  o += pad(clicks.length);
  for (const k of keys) f(k[0]);
  keys.forEach((k, i) => dv.setUint16(o + i * 2, k[1], true));
  o += pad(keys.length * 2);
  // One window record: client == frame, 1920x1080.
  [0, 0, 0, 1, 1, 1920, 1080].forEach(f);
  return buf;
}
