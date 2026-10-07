// Unit test of the player's codec report (lib/codecs.ts), plain Node:
//   node --experimental-strip-types --test tests/codecs.unit.test.ts
import { test } from "node:test";
import assert from "node:assert/strict";
import { CODEC_TYPES, playableCodecs, sameList } from "../src/lib/codecs.ts";

test("codecs: only the ones the player can play are reported", () => {
  const chromiumLinux = (t: string) => (t.includes("hvc1") ? "" : "probably");
  assert.deepEqual(playableCodecs(chromiumLinux), ["h264", "av1"]);
  assert.deepEqual(playableCodecs(() => "maybe"), ["h264", "hevc", "av1"]);
  assert.deepEqual(playableCodecs(() => ""), []);
  assert.ok(CODEC_TYPES.hevc.includes("hvc1") && CODEC_TYPES.av1.includes("av01"), "the sample entries the recorder writes");
  assert.ok(sameList(["h264"], ["h264"]) && !sameList(undefined, []) && !sameList(["h264"], ["h264", "av1"]));
});
