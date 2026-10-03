// Unit tests of the ability bubbles' layout and timing (lib/bubbles.ts), plain Node:
//   node --experimental-strip-types --test tests/bubbles.unit.test.ts
import { test } from "node:test";
import assert from "node:assert/strict";
import { alive, aliveRange, animation, draw, geometry, layout, pressEnds, sampleFrameOf, syntheticActions, trailEnds, ALPHA_END, RECAST_SCALE, SAMPLE_ACTIONS, type BubbleOptions, type PressArrays } from "../src/lib/bubbles.ts";
import { migrateOptions, clampSecs, DEFAULT_OPTIONS } from "../src/lib/overlayoptions.ts";

const Q = 0, W = 1, R = 3;
const opts = (o: Partial<BubbleOptions> = {}): BubbleOptions => ({ on: true, fade: 1, cats: {}, unconfirmed: false, ...o });
const BASE = 14;

/** Presses at (t, x px, y px, action, state); anchors are given in px directly. */
function presses(list: [number, number, number, number, number?][]): { p: PressArrays; anchor: (i: number) => [number, number] } {
  const p: PressArrays = { t: [], show: [], x: [], y: [], action: [], state: [], hint: [], hints: [] };
  for (const [t, x, y, a, st] of list) {
    p.t.push(t);
    p.show.push(t);
    p.x.push(x);
    p.y.push(y);
    p.action.push(a);
    p.state.push(st ?? 0);
    p.hint.push(0);
  }
  return { p, anchor: (i) => [p.x[i], p.y[i]] };
}

test("pop in, hold, fade out to the trail's faintest level; visible exactly for its time", () => {
  for (const total of [0.25, 1, 3]) {
    assert.equal(animation(-0.001, total), null, "not before its frame");
    const a0 = animation(0, total)!;
    assert.ok(a0 && a0.scale < 1 && a0.alpha === 1, "pops in from its frame");
    assert.ok(animation(Math.min(0.1, total * 0.3), total)!.scale === 1, "pop done within 100 ms");
    const last = animation(total - 0.001, total)!.alpha;
    assert.ok(last >= ALPHA_END && last < ALPHA_END + 0.05, `as faint as the trail's oldest part at the end (${last})`);
    assert.equal(animation(total, total), null, "gone after its time");
  }
  assert.equal(animation(0.5, 1)!.alpha, 1, "holds before fading");
});

test("alive range: from its frame until its trail piece leaves the trail", () => {
  const show = [1, 2, 2.5, 3, 10];
  const e = pressEnds(show);
  assert.deepEqual(aliveRange(show, e.max, 2.9, 1), [1, 3]);
  assert.deepEqual(aliveRange(show, e.max, 3, 1), [1, 4], "the one at 2 s is still in the range at exactly 3 s (end >= t - secs, like the trail)");
  assert.equal(alive(show, e, 1, 3, 1), true);
  assert.equal(alive(show, e, 1, 3.0001, 1), false);
  assert.deepEqual(aliveRange(show, e.max, 0.5, 3), [0, 0]);
});

// The trail's own rule (lib/inputoverlay.ts Overlay.draw): at time t the segment ending at
// sample k is drawn when lowerBound(mt, t - secs) <= k (= mt[k] >= t - secs), mt[k] <= t, k >= 1
// and k doesn't start a new stroke.
function lowerBound(a: ArrayLike<number>, v: number) {
  let lo = 0, hi = a.length;
  while (lo < hi) {
    const m = (lo + hi) >> 1;
    if (a[m] < v) lo = m + 1;
    else hi = m;
  }
  return lo;
}
function segmentDrawn(mt: Float32Array, mb: Uint8Array, k: number, t: number, secs: number) {
  const t0 = t - secs;
  const lo = Math.max(1, lowerBound(mt, t0));
  const hi = lowerBound(mt, t + 1e-12) - 1;
  return !mb[k] && k >= lo && k <= hi;
}

/** A cursor moving at 250 Hz with timer jitter, a 2 s rest (no samples), a stroke break. */
function cursor(dur = 30, rate = 250) {
  const mt: number[] = [];
  const mb: number[] = [];
  let t = 0, seed = 7;
  const rnd = () => ((seed = (seed * 1103515245 + 12345) >>> 0) / 2 ** 32);
  while (t < dur) {
    if (t > 10 && t < 12) {
      t = 12;
      continue;
    }
    mt.push(t);
    mb.push(mt.length === 1 || Math.abs(t - 20) < 0.003 ? 1 : 0);
    t += (1 / rate) * (0.8 + 0.4 * rnd());
  }
  return { mt: Float32Array.from(mt), mb: Uint8Array.from(mb), rate };
}

test("trail ends: the press's trail sample (moving), its own time (at rest / new stroke)", () => {
  const { mt, mb, rate } = cursor();
  const k = 1000;
  const mid = (mt[k - 1] + mt[k]) / 2;
  const at = trailEnds([mid, mt[k], 11, mt[lowerBound(mt, 12)] - 0.5 / rate], mt, mb, rate);
  assert.equal(at.end[0], mt[k], "between two samples: the segment ending at the next one");
  assert.equal(at.end[1], mt[k], "exactly on a sample: that sample (the end of its segment)");
  assert.equal(at.end[2], 11, "cursor at rest: the press time");
  assert.equal(at.end[3], mt[lowerBound(mt, 12)], "moving off after the rest, within the last period: that segment");
  const brk = lowerBound(mt, 19.999);
  assert.equal(mb[brk], 1);
  assert.equal(trailEnds([(mt[brk - 1] + mt[brk]) / 2], mt, mb, rate).end[0], (mt[brk - 1] + mt[brk]) / 2, "before a stroke break: the press time");
  assert.deepEqual([...trailEnds([5], new Float32Array(), new Uint8Array(), 250).end], [5], "no samples: the press time");
});

test("bubble and its trail piece end on the same frame (0.25 s and 3 s, 60 fps and 30 fps)", () => {
  const { mt, mb, rate } = cursor();
  // Presses spread over the moving parts, off the frame grid.
  const t: number[] = [];
  for (let x = 0.5; x < 29; x += 0.137) if (!(x > 9.9 && x < 12.1) && Math.abs(x - 20) > 0.05) t.push(Math.round(x * 1e4) / 1e4);
  const ends = trailEnds(t, mt, mb, rate);
  for (const fps of [60, 30]) {
    const frame = (i: number) => Math.round((i * 1000) / fps) / 1000;
    for (const secs of [0.25, 3]) {
      let checked = 0, mismatches = 0;
      for (let i = 0; i < t.length; i++) {
        const k = lowerBound(mt, t[i] + 1e-12); // the sample ending the segment under the press
        if (mb[k] || ends.end[i] !== mt[k]) continue;
        // Frame of the key-down (bubble's first frame) and the last frame the trail piece is drawn.
        const f0 = Math.floor(t[i] * fps + 1e-9);
        let lastTrail = -1, lastBubble = -1;
        for (let f = f0; frame(f) <= t[i] + secs + 0.2; f++) {
          const ft = frame(f);
          const show = frame(f0);
          if (ft >= mt[k] && segmentDrawn(mt, mb, k, ft, secs)) lastTrail = f;
          if (alive([show], { end: Float64Array.of(ends.end[i]), max: Float64Array.of(ends.end[i]) }, 0, ft, secs)) lastBubble = f;
        }
        checked++;
        if (lastTrail !== lastBubble) mismatches++;
      }
      assert.ok(checked > 150, `${checked} presses`);
      assert.equal(mismatches, 0, `${fps} fps, ${secs} s: ${mismatches} of ${checked} presses end on another frame than their trail piece`);
    }
  }
});

test("draw: the bubble is drawn exactly while its trail piece is", () => {
  const { mt, mb, rate } = cursor();
  const { p, anchor } = presses([[5.0021, 200, 150, Q]]);
  const ends = trailEnds(p.t, mt, mb, rate);
  const k = lowerBound(mt, 5.0021 + 1e-12);
  const g: any = new Proxy({}, { get: (_t, key) => (key === "measureText" ? () => ({ width: 10 }) : () => {}), set: () => true });
  for (const secs of [0.25, 3]) {
    const L = layout(p, SAMPLE_ACTIONS, opts({ fade: secs }), BASE, anchor, ends);
    const lastOn = mt[k] + secs;
    assert.equal(draw(g, p, SAMPLE_ACTIONS, L, opts({ fade: secs }), BASE, lastOn - 0.001, ends), 1);
    assert.equal(segmentDrawn(mt, mb, k, lastOn - 0.001, secs), true);
    assert.equal(draw(g, p, SAMPLE_ACTIONS, L, opts({ fade: secs }), BASE, lastOn + 0.001, ends), 0);
    assert.equal(segmentDrawn(mt, mb, k, lastOn + 0.001, secs), false);
  }
});

test("options: one Trail & bubbles time, old saved values migrated", () => {
  assert.equal(DEFAULT_OPTIONS.trailSecs, 1);
  assert.equal("bubbleFade" in DEFAULT_OPTIONS, false);
  // v1.5/v1.6 saves always had both times.
  assert.equal(migrateOptions({ trail: true, trailSecs: 2.5, bubbles: true, bubbleFade: 0.4 }).trailSecs, 2.5, "trail on: its length is kept");
  assert.equal(migrateOptions({ trail: false, trailSecs: 2.5, bubbles: true, bubbleFade: 0.4 }).trailSecs, 0.4, "only bubbles in use: their fade time");
  assert.equal(migrateOptions({ trail: false, trailSecs: 2.5, bubbles: false, bubbleFade: 0.4 }).trailSecs, 2.5, "neither in use: the trail length");
  assert.equal(migrateOptions({ bubbleFade: 3 }).trailSecs, 3, "a fade time alone");
  assert.equal(migrateOptions({ trail: false, bubbles: true, trailSecs: 1, bubbleFade: 0.1 }).trailSecs, 0.25, "clamped to 0.25 s");
  assert.equal(migrateOptions({ trailSecs: 9 }).trailSecs, 3);
  assert.equal(migrateOptions({ trailSecs: "x" }).trailSecs, 1);
  assert.equal("bubbleFade" in migrateOptions({ trailSecs: 1, bubbleFade: 2 }), false, "the old field is dropped");
  // Already the new format: the fade field (if any) is ignored.
  assert.equal(migrateOptions({ v: 2, trail: false, trailSecs: 1.5, bubbleFade: 0.4 }).trailSecs, 1.5);
  assert.equal("v" in migrateOptions({ v: 2, trailSecs: 1 }), false);
  assert.equal(clampSecs(0.35), 0.35);
  assert.equal(clampSecs(1.03), 1.05);
  // Other options untouched.
  const o = migrateOptions({ trail: true, trailSecs: 1.25, clicks: false, keys: true, heat: true, heatRange: "range", bubbleCats: { ward: false }, bubbleFade: 1 });
  assert.deepEqual(o, { ...DEFAULT_OPTIONS, trailSecs: 1.25, clicks: false, keys: true, heat: true, heatRange: "range", bubbleCats: { ward: false } });
});

test("same action pressed repeatedly: bubbles overlap at their exact positions, no offsets", () => {
  const { p, anchor } = presses(Array.from({ length: 12 }, (_, k) => [10 + k * 0.08, 200 + k * 3, 150, Q] as [number, number, number, number]));
  const L = layout(p, SAMPLE_ACTIONS, opts({ fade: 3 }), BASE, anchor);
  for (let i = 0; i < 12; i++) {
    assert.equal(L.dx[i], 0);
    assert.equal(L.ax[i], 200 + i * 3);
    assert.equal(L.ay[i], 150);
  }
});

test("another action covering a letter: only the body moves sideways, the anchor stays", () => {
  const { p, anchor } = presses([
    [10, 200, 150, Q],
    [10.05, 204, 152, W],
  ]);
  const L = layout(p, SAMPLE_ACTIONS, opts(), BASE, anchor);
  assert.equal(L.dx[0], 0, "the older bubble stays");
  assert.notEqual(L.dx[1], 0, "the newer one is nudged");
  assert.ok(Math.abs(L.dx[1]) <= geometry(BASE, 1).r * 2.6, `a few px: ${L.dx[1]}`);
  assert.equal(L.ax[1], 204, "anchor x exact");
  assert.equal(L.ay[1], 152, "anchor y exact");
  // Q's letter is no longer under W's body.
  const gq = geometry(BASE, 1), gw = geometry(BASE, 1);
  assert.ok(Math.abs(L.ax[1] + L.dx[1] - L.ax[0]) >= gw.r + gq.letter - 1e-6);
  // It moved away from Q (to the right: W is right of Q).
  assert.ok(L.dx[1] > 0);
  // Far apart, or after Q is gone: no nudge.
  const far = presses([
    [10, 200, 150, Q],
    [10.05, 300, 150, W],
  ]);
  assert.equal(layout(far.p, SAMPLE_ACTIONS, opts(), BASE, far.anchor).dx[1], 0);
  const later = presses([
    [10, 200, 150, Q],
    [11.2, 204, 152, W],
  ]);
  assert.equal(layout(later.p, SAMPLE_ACTIONS, opts({ fade: 1 }), BASE, later.anchor).dx[1], 0, "Q faded out before W appeared");
  assert.notEqual(layout(later.p, SAMPLE_ACTIONS, opts({ fade: 3 }), BASE, later.anchor).dx[1], 0, "with a 3 s fade Q is still there");
});

test("W in the middle of Q spam stays readable", () => {
  const list: [number, number, number, number][] = [];
  for (let k = 0; k < 38; k++) list.push([40 + k * 0.08, 300 + k * 4, 200 + (k % 3), Q]);
  list.push([41.53, 300 + 19 * 4 + 1, 201, W]);
  list.sort((a, b) => a[0] - b[0]);
  const { p, anchor } = presses(list);
  const L = layout(p, SAMPLE_ACTIONS, opts({ fade: 3 }), BASE, anchor);
  const w = p.action.indexOf(W);
  const g = geometry(BASE, 1);
  const wx = L.ax[w] + L.dx[w];
  let nudged = 0;
  for (let i = w + 1; i < p.t.length; i++) {
    // Every later Q body is clear of W's letter...
    assert.ok(Math.abs(L.ax[i] + L.dx[i] - wx) >= g.r + g.letter - 1e-6, `Q ${i} covers W`);
    if (L.dx[i]) nudged++;
    // ...with its anchor exact.
    assert.equal(L.ax[i], p.x[i]);
  }
  assert.ok(nudged > 0);
  // The Qs before W never move (they're older; W is the one that gives way).
  for (let i = 0; i < w; i++) assert.equal(L.dx[i], 0);
  assert.ok(L.ms < 20, `layout ${L.ms} ms`);
});

test("filters: hidden categories and hidden unconfirmed presses neither draw nor push others", () => {
  const { p, anchor } = presses([
    [10, 200, 150, R, 2],
    [10.05, 204, 150, Q],
  ]);
  let L = layout(p, SAMPLE_ACTIONS, opts({ unconfirmed: false }), BASE, anchor);
  assert.equal(L.on[0], 0);
  assert.equal(L.dx[1], 0);
  L = layout(p, SAMPLE_ACTIONS, opts({ unconfirmed: true }), BASE, anchor);
  assert.equal(L.on[0], 1);
  assert.notEqual(L.dx[1], 0);
  L = layout(p, SAMPLE_ACTIONS, opts({ unconfirmed: true, cats: { ability: false } }), BASE, anchor);
  assert.equal(L.on[0] + L.on[1], 0);
});

test("sample data: frames and a long game's layout stay fast", () => {
  assert.equal(sampleFrameOf(0.0668), 0.033, "frame 2 starts at 67 ms (WebM ms timestamps)");
  assert.equal(sampleFrameOf(0.067), 0.067);
  assert.equal(sampleFrameOf(41.53), 41.5);
  const v = syntheticActions(2400);
  const n = v.presses.t.length;
  const L = layout(v.presses, v.actions, opts({ fade: 3, unconfirmed: true }), BASE, (i) => [v.presses.x[i] * 1600, v.presses.y[i] * 900]);
  assert.ok(n > 2500);
  assert.ok(L.ms < 50, `${n} presses laid out in ${L.ms.toFixed(1)} ms`);
  console.log(`layout of ${n} presses (40 min game, 3 s fade): ${L.ms.toFixed(2)} ms`);
});

test("ult recasts: a smaller, outlined R bubble at the exact spot, shown with the filter off", () => {
  // Two R presses: the first is the ult (confirmed), the second a recast of it (state 3).
  const { p, anchor } = presses([
    [10, 300, 200, R, 1],
    [10.5, 500, 200, R, 3],
  ]);
  const L = layout(p, SAMPLE_ACTIONS, opts(), BASE, anchor);
  assert.deepEqual([...L.on], [1, 1], "a recast isn't an unconfirmed press: shown without the filter");
  const arcs: { x: number; r: number; style: string }[] = [];
  let fill = "";
  let stroke = "";
  const g: any = new Proxy(
    {},
    {
      get: (_t, k) => {
        if (k === "arc") return (x: number, _y: number, r: number) => arcs.push({ x, r, style: "" });
        if (k === "fill") return () => arcs.length && (arcs[arcs.length - 1].style = "fill:" + fill);
        if (k === "stroke") return () => arcs.length && !arcs[arcs.length - 1].style && (arcs[arcs.length - 1].style = "stroke:" + stroke);
        if (k === "measureText") return () => ({ width: 10 });
        return () => {};
      },
      set: (_t, k, v) => {
        if (k === "fillStyle") fill = String(v);
        if (k === "strokeStyle") stroke = String(v);
        return true;
      },
    },
  );
  draw(g, p, SAMPLE_ACTIONS, L, opts({ fade: 3 }), BASE, 11);
  // Bodies: the biggest circle drawn at each press's x.
  const body = (x: number) => Math.max(...arcs.filter((a) => Math.abs(a.x - x) < 1e-6).map((a) => a.r));
  const ratio = body(500) / body(300);
  assert.ok(Math.abs(ratio - RECAST_SCALE) < 0.02, `recast body ${ratio.toFixed(2)}x the ult's`);
  const solid = arcs.find((a) => Math.abs(a.x - 300) < 1e-6 && a.r === body(300))!;
  const outlined = arcs.find((a) => Math.abs(a.x - 500) < 1e-6 && a.r === body(500))!;
  assert.match(solid.style, /fill:#|fill:rgb\(1|fill:hsl/i, "the ult: filled with R's colour");
  assert.match(outlined.style, /fill:rgba\(15,17,22/, "the recast: dark inside, outlined in R's colour");
});
