// Unit tests of the player's controls-row fitting (lib/controlsfit.ts), plain Node:
//   node --experimental-strip-types --test tests/controlsfit.unit.test.ts
import { test } from "node:test";
import assert from "node:assert/strict";
import { collapseLevel, freed, COLLAPSE, ESTIMATE } from "../src/lib/controlsfit.ts";

const GAP = 4;
// Measured widths like in the window (Chromium, 100 %).
const W: Record<string, number> = { vol: 80, rate: 50, set: 34, ovtext: 76, tsub: 150, zoom: 166, evprev: 34, evnext: 34, stepb: 34, stepf: 34, more: 34 };
const ALWAYS = 400; // play, short time, overlay icon, mute, fullscreen, paddings

/** The row's width at level k, as the browser would measure it. */
function used(k: number) {
  let u = ALWAYS;
  for (let i = k; i < COLLAPSE.length; i++) u += freed(COLLAPSE[i], W, GAP);
  return u + (k > 0 ? W.more + GAP : 0);
}

test("the smallest level that fits, from any current level", () => {
  for (let avail = 300; avail <= 1400; avail += 7) {
    let want = COLLAPSE.length;
    for (let k = 0; k <= COLLAPSE.length; k++)
      if (used(k) <= avail) {
        want = k;
        break;
      }
    for (let cur = 0; cur <= COLLAPSE.length; cur++) {
      const got = collapseLevel(avail, used(cur), cur, W, GAP);
      // Going back to a wider layout keeps a few px to spare (no flapping): at most one level more.
      assert.ok(got === want || (got > want && got <= cur && used(want) + 6 > avail), `avail ${avail}, at level ${cur}: ${got} (want ${want})`);
      assert.ok(used(got) <= avail || got === COLLAPSE.length, `level ${got} fits ${avail}`);
    }
  }
});

test("collapse order: volume, speed, settings, overlay text, time details, zoom, event nav, frame steps", () => {
  assert.deepEqual([...COLLAPSE], ["vol", "rate", "set", "ovtext", "tsub", "zoom", "evnav", "step"]);
  // Everything fits at a wide player.
  assert.equal(collapseLevel(1400, used(0), 0, W, GAP), 0);
  // A little too narrow: only the volume slider goes (into the menu, + the menu button).
  assert.equal(collapseLevel(used(0) - 1, used(0), 0, W, GAP), 1);
});

test("unmeasured parts use the estimates", () => {
  for (const id of COLLAPSE) assert.ok(freed(id, {}, GAP) > 0, id);
  assert.equal(freed("vol", {}, GAP), ESTIMATE.vol + GAP);
  assert.equal(freed("evnav", { evprev: 30, evnext: 30 }, 2), 64);
});

test("stable: re-deciding at the chosen level gives the same level", () => {
  for (let avail = 300; avail <= 1400; avail += 3) {
    const k = collapseLevel(avail, used(0), 0, W, GAP);
    assert.equal(collapseLevel(avail, used(k), k, W, GAP), k, `avail ${avail}`);
  }
});
