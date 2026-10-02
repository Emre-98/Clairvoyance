// Unit tests of the ability bubbles' layout and timing (lib/bubbles.ts), plain Node:
//   node --experimental-strip-types --test tests/bubbles.unit.test.ts
import { test } from "node:test";
import assert from "node:assert/strict";
import { aliveRange, animation, draw, geometry, layout, sampleFrameOf, syntheticActions, RECAST_SCALE, SAMPLE_ACTIONS, type BubbleOptions, type PressArrays } from "../src/lib/bubbles.ts";

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

test("pop in, hold, fade out; visible exactly for the fade time", () => {
  for (const fade of [0.1, 1, 3]) {
    assert.equal(animation(-0.001, fade), null, "not before its frame");
    const a0 = animation(0, fade)!;
    assert.ok(a0 && a0.scale < 1 && a0.alpha === 1, "pops in from its frame");
    assert.ok(animation(Math.min(0.1, fade * 0.3), fade)!.scale === 1, "pop done within 100 ms");
    assert.ok(animation(fade - 0.001, fade)!.alpha < 0.05, "almost gone at the end");
    assert.equal(animation(fade, fade), null, "gone after the fade time");
  }
  assert.equal(animation(0.5, 1)!.alpha, 1, "holds before fading");
});

test("alive range follows the frame time and the fade time", () => {
  const show = [1, 2, 2.5, 3, 10];
  assert.deepEqual(aliveRange(show, 2.9, 1), [1, 3]);
  assert.deepEqual(aliveRange(show, 3, 1), [2, 4], "a bubble is gone exactly fade s after its frame; the new one shows on its frame");
  assert.deepEqual(aliveRange(show, 0.5, 3), [0, 0]);
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
