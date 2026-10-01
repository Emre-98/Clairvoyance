// Replay input overlay: parses the recorded mouse/keyboard data (binary from `input_load`,
// layout in cv_core::input::stats::ui_payload) and draws it over the video for a time `t`
// (video seconds). Pure functions + one Overlay class; no Svelte here so it can be tested.

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
}

export interface KeyBar {
  vk: number;
  t0: number;
  t1: number;
  label: string;
  lane: number;
}

export interface OverlayOptions {
  trail: boolean;
  trailSecs: number;
  clicks: boolean;
  dot: boolean;
  keys: boolean;
  heat: boolean;
  /** "game" = whole game, "range" = the selected range (if any). */
  heatRange: "game" | "range";
}

export const DEFAULT_OPTIONS: OverlayOptions = { trail: true, trailSecs: 1, clicks: true, dot: true, keys: false, heat: false, heatRange: "game" };
const OPT_KEY = "cv.inputOverlay";

export function loadOptions(): OverlayOptions {
  try {
    const raw = localStorage.getItem(OPT_KEY);
    if (raw) return { ...DEFAULT_OPTIONS, ...JSON.parse(raw) };
  } catch {}
  return { ...DEFAULT_OPTIONS };
}

export function saveOptions(o: OverlayOptions) {
  try {
    localStorage.setItem(OPT_KEY, JSON.stringify(o));
  } catch {}
}

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
  return { rate, mt, mx, my, mb, ct, cx, cy, cc, kt, kc, gaps, wt, wm, heat, keys };
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

/** Where the video content is drawn inside the element (object-fit: contain). */
export function videoRect(boxW: number, boxH: number, vw: number, vh: number) {
  if (!vw || !vh) return { x: 0, y: 0, w: boxW, h: boxH };
  const s = Math.min(boxW / vw, boxH / vh);
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
const STRIP_BEFORE = 2.5;
const STRIP_AFTER = 0.5;

/** Draws the overlay; keeps the heatmap images it made (computed once per range). */
export class Overlay {
  private heatCache = new Map<string, HTMLCanvasElement>();
  constructor(public d: InputData) {}

  draw(g: CanvasRenderingContext2D, cssW: number, cssH: number, dpr: number, t: number, vw: number, vh: number, o: OverlayOptions, heatRange: [number, number] | null) {
    const d = this.d;
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    g.clearRect(0, 0, cssW, cssH);
    const rect = videoRect(cssW, cssH, vw, vh);
    const m = mapper(d, t, rect, vw, vh);
    g.save();
    g.beginPath();
    g.rect(rect.x, rect.y, rect.w, rect.h);
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
      const t0 = t - o.trailSecs;
      const lo = Math.max(1, lowerBound(d.mt, t0));
      g.lineCap = "round";
      g.lineJoin = "round";
      // Fade: older segments in fainter buckets (one stroke per bucket keeps it fast).
      for (let b = 0; b < TRAIL_BUCKETS; b++) {
        const a0 = t0 + (o.trailSecs * b) / TRAIL_BUCKETS;
        const a1 = t0 + (o.trailSecs * (b + 1)) / TRAIL_BUCKETS;
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
    g.restore();

    if (o.keys) this.drawKeys(g, rect, t);
  }

  /** A strip of the keys pressed from 2.5 s before to 0.5 s after `t`, one lane per key group. */
  private drawKeys(g: CanvasRenderingContext2D, rect: { x: number; y: number; w: number; h: number }, t: number) {
    const keys = this.d.keys;
    const W = Math.max(220, Math.min(rect.w * 0.36, 520));
    const laneH = Math.max(15, Math.min(22, rect.h / 30));
    const H = laneH * 3 + 10;
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
    // Keys are sorted by press time: find the ones overlapping the window.
    const from = t - STRIP_BEFORE - 3;
    let s = 0;
    let lo = 0;
    let hi = keys.length;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if (keys[mid].t0 < from) lo = mid + 1;
      else hi = mid;
    }
    s = lo;
    for (let i = s; i < keys.length && keys[i].t0 <= t + STRIP_AFTER; i++) {
      const k = keys[i];
      if (k.t1 < t - STRIP_BEFORE) continue;
      const a = px(k.t0);
      const w = Math.max(px(k.t1) - a, g.measureText(k.label).width + 10);
      const y = y0 + 5 + k.lane * laneH;
      const held = k.t0 <= t && t <= k.t1;
      const future = k.t0 > t;
      g.fillStyle = held ? "rgba(255,236,153,0.95)" : future ? "rgba(255,255,255,0.18)" : "rgba(255,255,255,0.32)";
      roundRect(g, a, y, w, laneH - 3, 4);
      g.fill();
      g.fillStyle = held ? "#1b1b1b" : "rgba(255,255,255,0.92)";
      g.fillText(k.label, a + 5, y + (laneH - 3) / 2 + 0.5);
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
