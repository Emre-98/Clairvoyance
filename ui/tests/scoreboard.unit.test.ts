// Unit tests of the time-synced scoreboard (lib/scoreboard.ts), plain Node:
//   node --experimental-strip-types --test tests/scoreboard.unit.test.ts
import { test } from "node:test";
import assert from "node:assert/strict";
import { build, frameAt, stateAt, shortName } from "../src/lib/scoreboard.ts";
import type { Scoreboard } from "../src/lib/types.ts";

const sb: Scoreboard = {
  version: "16.20.1",
  players: [
    { name: "Me#EUW", character: "Ahri", character_id: "Ahri", team: "ORDER", me: true },
    { name: "Enemy#NA1", character: "Zed", character_id: "Zed", team: "CHAOS" },
  ],
  names: { "3031": "Infinity Edge" },
  frames: [
    { t: 10, d: [{ i: 0, lv: 1, it: [1056], sp: ["SummonerFlash", "SummonerDot"] }, { i: 1, lv: 1 }] },
    { t: 120, d: [{ i: 0, lv: 3, cs: 14, k: 1 }] },
    { t: 300, d: [{ i: 1, d: 1, it: [3031] }] },
  ],
};

test("scoreboard: the state at any playback time, changes carried forward", () => {
  const tl = build(sb);
  assert.equal(frameAt(tl, 0), 0, "before the first read: the first read");
  assert.deepEqual(stateAt(tl, 5)![0], { level: 1, kills: 0, deaths: 0, assists: 0, cs: 0, items: [1056], spells: ["SummonerFlash", "SummonerDot"] });
  assert.equal(stateAt(tl, 119.9)![0].level, 1);
  assert.equal(stateAt(tl, 120)![0].level, 3);
  assert.deepEqual(stateAt(tl, 200)![0].items, [1056], "unchanged items carried forward");
  assert.equal(stateAt(tl, 200)![1].deaths, 0);
  assert.deepEqual(stateAt(tl, 1e9)![1], { level: 1, kills: 0, deaths: 1, assists: 0, cs: 0, items: [3031], spells: [] });
  // Scrubbing back: earlier frames aren't changed by later ones.
  assert.equal(stateAt(tl, 150)![1].items.length, 0);
});

test("scoreboard: empty and names", () => {
  assert.equal(stateAt(build({ ...sb, frames: [] }), 10), null);
  assert.equal(shortName("Faker#KR1"), "Faker");
  assert.equal(shortName("NoTag"), "NoTag");
});

test("scoreboard: lookups stay fast on a long game", () => {
  const frames = Array.from({ length: 400 }, (_, k) => ({ t: k * 5, d: Array.from({ length: 10 }, (_, i) => ({ i, cs: k * (i + 1) })) }));
  const big = build({ ...sb, players: Array.from({ length: 10 }, (_, i) => ({ name: `P${i}`, character: "A", character_id: "A", team: i < 5 ? "ORDER" : "CHAOS" })), frames });
  const t0 = performance.now();
  for (let n = 0; n < 100000; n++) stateAt(big, (n * 7.3) % 2000);
  assert.ok(performance.now() - t0 < 200, "100k lookups");
  assert.equal(stateAt(big, 1000)![9].cs, 200 * 10);
});
