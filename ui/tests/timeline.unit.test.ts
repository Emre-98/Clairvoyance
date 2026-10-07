// Unit tests of the zoomable timeline's mapping, zoom, follow, ruler, lanes and frame stepping
// (lib/timelineview.ts), plain Node:
//   node --experimental-strip-types --test tests/timeline.unit.test.ts
import { test } from "node:test";
import assert from "node:assert/strict";
import {
  clampView,
  follow,
  frameDur,
  frameIndexAt,
  frameNearest,
  frameSeekTime,
  fullView,
  isZoomed,
  lanes,
  minSpan,
  pan,
  preciseClock,
  ruler,
  spanAt,
  tToX,
  xToT,
  zoomAround,
  zoomLevel,
} from "../src/lib/timelineview.ts";

const near = (a: number, b: number, eps = 1e-9, msg = "") => assert.ok(Math.abs(a - b) <= eps, `${msg} ${a} != ${b}`);

/** 60 fps with WebM's whole-millisecond timestamps. */
const fps60 = (n: number) => Float64Array.from({ length: n }, (_, k) => Math.round((k * 1000) / 60) / 1000);
/** Variable frame rate: 16-17 ms frames with a 50 ms hitch every 100 frames. */
const vfr = (n: number) => {
  const a = new Float64Array(n);
  for (let k = 1; k < n; k++) a[k] = a[k - 1] + (k % 100 === 0 ? 0.05 : k % 3 ? 0.0167 : 0.0166);
  return a;
};

test("time <-> pixel mapping is exact and invertible at every zoom", () => {
  const dur = 1980;
  for (const v of [fullView(dur), { start: 100, span: 10 }, { start: 1234.5, span: 0.25 }]) {
    for (const w of [300, 1517, 3840]) {
      near(tToX(v.start, v, w), 0);
      near(tToX(v.start + v.span, v, w), w, 1e-6);
      for (const x of [0, 1, w / 3, w - 1]) near(tToX(xToT(x, v, w), v, w), x, 1e-6, "round trip");
    }
  }
  // Whole game: 0 at the left edge, the end at the right edge.
  near(tToX(990, fullView(dur), 1000), 500);
});

test("zoom keeps the anchor under the cursor and stays inside the video", () => {
  const dur = 600;
  const frames = fps60(dur * 60);
  const min = minSpan(dur, frames);
  near(min, 20 * frameDur(frames), 1e-9, "shortest view = 20 frames");
  let v = fullView(dur);
  const w = 1200;
  const anchor = 123.4;
  const x0 = tToX(anchor, v, w);
  for (let i = 0; i < 40; i++) {
    v = zoomAround(v, 1.3, anchor, dur, min);
    assert.ok(v.start >= 0 && v.start + v.span <= dur + 1e-9, "inside the video");
    if (v.span > min * 1.0001) near(tToX(anchor, v, w), x0, 1e-6, "anchor stays put");
  }
  near(v.span, min, 1e-12, "stops at the shortest span");
  assert.ok(isZoomed(v, dur));
  // Zooming out past the whole game clamps to it.
  for (let i = 0; i < 80; i++) v = zoomAround(v, 0.5, anchor, dur, min);
  assert.deepEqual(v, fullView(dur));
  assert.ok(!isZoomed(v, dur));
  // Anchors near the ends stay under the cursor and the view never leaves the video.
  v = zoomAround(fullView(dur), 50, 1, dur, min);
  near(tToX(1, v, w), tToX(1, fullView(dur), w), 1e-6);
  assert.ok(v.start >= 0);
  v = zoomAround({ start: 0, span: 30 }, 0.5, 29, 40, min);
  near(v.start + v.span, 40, 1e-9, "clamped at the end when the anchor's place would leave the video");
});

test("slider level <-> span is logarithmic and round-trips", () => {
  const dur = 2000;
  const min = 1 / 3;
  near(spanAt(0, dur, min), dur);
  near(spanAt(1, dur, min), min, 1e-9);
  for (const l of [0, 0.1, 0.5, 0.77, 1]) near(zoomLevel({ start: 0, span: spanAt(l, dur, min) }, dur, min), l, 1e-9);
  // Each 10 % of the slider is the same zoom factor.
  const f1 = spanAt(0.2, dur, min) / spanAt(0.3, dur, min);
  const f2 = spanAt(0.7, dur, min) / spanAt(0.8, dur, min);
  near(f1, f2, 1e-9);
});

test("the playhead stays in view while playing; pans don't fight it", () => {
  const dur = 100;
  const v = { start: 10, span: 5 };
  assert.equal(follow(v, 12, dur, 0.3), v, "inside: unchanged");
  const p = follow(v, 14.8, dur, 0.3); // past 92 %
  near(p.span, 5);
  assert.ok(p.start < 14.8 && 14.8 - p.start < 0.5, "pages so it sits near the left");
  const b = follow(v, 3, dur, 0.3);
  assert.ok(b.start <= 3 && 3 <= b.start + b.span, "jumped backwards into view");
  near(follow({ start: 96, span: 4 }, 99.9, dur, 0.3).start, 96, 1e-9, "at the end the view can't go further");
  const pn = pan(v, 50, dur, 0.3);
  near(pn.start, 60);
  near(pan(v, 500, dur, 0.3).start, 95);
  near(pan(v, -500, dur, 0.3).start, 0);
  assert.deepEqual(clampView({ start: -5, span: 1e9 }, dur, 0.3), { start: 0, span: dur });
});

test("frame index at a time, frame seek targets, nearest frame (constant and variable rate)", () => {
  for (const f of [fps60(6000), vfr(6000)]) {
    for (const i of [0, 1, 59, 60, 61, 599, 1234, 5998]) {
      assert.equal(frameIndexAt(f, f[i]), i, `start of frame ${i}`);
      assert.equal(frameIndexAt(f, f[i + 1] - 1e-4), i, `end of frame ${i}`);
      const s = frameSeekTime(f, i);
      assert.ok(s > f[i] && s < f[i + 1], "seek target inside the frame");
      assert.equal(frameIndexAt(f, s), i);
      assert.equal(frameNearest(f, f[i] + 2e-7), i, "presented time maps back");
      assert.equal(frameNearest(f, f[i] - 2e-7), i);
    }
    // Stepping from a seek target moves exactly one frame.
    let i = 100;
    for (let k = 0; k < 300; k++) {
      const next = frameIndexAt(f, frameSeekTime(f, i)) + 1;
      assert.equal(next, i + 1);
      i = next;
    }
    const last = f.length - 1;
    assert.ok(frameSeekTime(f, last) > f[last], "last frame has a seek target past its start");
    assert.equal(frameIndexAt(f, -1), 0);
    assert.equal(frameIndexAt(f, 1e9), last);
  }
});

test("ruler: labels on round game-clock times, at least 64 px apart; frame ticks when frames are 3 px+", () => {
  const frames = fps60(600 * 60);
  const w = 1200;
  // Whole 10 min game.
  let r = ruler(fullView(600), w, frames, 20);
  assert.ok(r.major.length >= 5 && r.major.length <= 19, `${r.major.length} labels`);
  for (let i = 1; i < r.major.length; i++) assert.ok(tToX(r.major[i].t, fullView(600), w) - tToX(r.major[i - 1].t, fullView(600), w) >= 63.9);
  assert.equal(r.frameTo, -1, "no frame ticks for the whole game");
  for (const m of r.major) near(((m.t - 20) % 30 + 30) % 30, 0, 1e-6, "on the game clock (offset 20 s)");
  // 1 s across 1200 px: 60 frames, 20 px each: every frame ticked and numbered.
  const v = { start: 100.0, span: 1 };
  r = ruler(v, w, frames, 20);
  assert.equal(r.frameFrom, frameIndexAt(frames, 100));
  assert.ok(r.frameTo - r.frameFrom >= 60 && r.frameTo - r.frameFrom <= 62);
  assert.ok(r.frameLabelEvery >= 1 && r.frameLabelEvery <= 5);
  assert.ok(r.major.every((m) => /^\d+:\d\d\.\d$/.test(m.label)), r.major.map((m) => m.label).join(" "));
  // Loading screen (before the game clock): negative labels.
  r = ruler({ start: 0, span: 60 }, w, frames, 20);
  assert.ok(r.major.some((m) => m.label.startsWith("-")));
});

test("marker lanes: overlapping markers stack, then spread out when zoomed in", () => {
  const dur = 1800;
  const times = [100, 100.4, 100.8, 101.2, 500, 900];
  const w = 1000;
  const at = (v: { start: number; span: number }) => lanes(Float64Array.from(times, (t) => tToX(t, v, w)), w, 18, 3);
  const full = at(fullView(dur));
  assert.deepEqual([...full.lane], [0, 1, 2, 0, 0, 0], "3 lanes, the 4th overlaps in the least recent lane");
  assert.equal(full.count, 3);
  const zoomed = at({ start: 95, span: 20 });
  assert.deepEqual([...zoomed.lane].slice(0, 4), [0, 0, 0, 0], "spread out on one lane");
  assert.equal(zoomed.lane[4], -1, "out of view: not drawn");
  // 600 markers lay out quickly.
  const many = Float64Array.from({ length: 600 }, (_, i) => ((i * 7919) % 1490) * 0.67);
  many.sort();
  const t0 = performance.now();
  for (let k = 0; k < 100; k++) lanes(many, w, 18, 3);
  assert.ok((performance.now() - t0) / 100 < 0.5, "under 0.5 ms for 600 markers");
});

test("precise clock", () => {
  assert.equal(preciseClock(0), "0:00.000");
  assert.equal(preciseClock(83.4567), "1:23.457");
  assert.equal(preciseClock(-20), "-0:20.000");
  assert.equal(preciseClock(3723.004), "1:02:03.004");
});

// ---------- APM chart behind the markers (lib/apmchart.ts) ----------
import { apmAt, apmScale } from "../src/lib/apmchart.ts";

test("APM chart: value under the cursor by 10 s bin, nothing outside or unfocused", () => {
  const bins = [null, 120, 180, null, 90];
  assert.equal(apmAt(bins, 10, -1), null);
  assert.equal(apmAt(bins, 10, 5), null, "unfocused bin");
  assert.equal(apmAt(bins, 10, 10), 120);
  assert.equal(apmAt(bins, 10, 29.99), 180);
  assert.equal(apmAt(bins, 10, 49), 90);
  assert.equal(apmAt(bins, 10, 50), null, "past the end");
  assert.equal(apmAt(bins, 10, NaN), null);
});

test("APM chart: one burst doesn't flatten the rest (95th percentile scale)", () => {
  const bins = Array.from({ length: 100 }, (_, i) => (i === 50 ? 900 : 150));
  const top = apmScale(bins);
  assert.ok(top < 200 && top >= 150, `scale ${top}`);
  assert.equal(apmScale([null, null]), 0, "nothing to draw");
  assert.equal(apmScale([5]), 30, "a floor so a quiet game isn't a full-height wall");
});
