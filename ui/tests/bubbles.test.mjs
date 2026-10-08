// Pixel test of the ability bubbles in Chromium against the mock backend
// (`VITE_MOCK=1 npx vite`) and the sample video (python3 tests/make-sample.py): 4:3 video with
// the 16:9 game letterboxed inside, pillarboxed by the 16:9 player, 30 fps with WebM's whole-ms
// frame times. The mock's bubbles are lib/bubbles.ts `syntheticActions` (cursor on a circle:
// (0.5 + 0.3 cos t, 0.5 + 0.3 sin t) of the game area).
//
//   node tests/bubbles.test.mjs [http://localhost:5173] [outdir]
import { chromium } from "playwright-core";

const BASE = process.argv[2] ?? "http://localhost:5173";
const OUT = process.argv[3] ?? "/tmp";
const exe = process.env.CHROME ?? "/opt/pw-browsers/chromium-1194/chrome-linux/chrome";
const results = [];
const check = (name, ok, detail = "") => {
  results.push({ name, ok, detail });
  console.log(`${ok ? "PASS" : "FAIL"} ${name}${detail ? " — " + detail : ""}`);
};
const pos = (t) => [0.5 + 0.3 * Math.cos(t), 0.5 + 0.3 * Math.sin(t)];
/** Frame start (s) of the sample video's frame containing t: 30 fps, ms timestamps. */
const frameOf = (t) => {
  const at = (i) => Math.round((i * 1000) / 30) / 1000;
  const k = Math.floor(t * 30 + 1e-9);
  if (at(k + 1) <= t) return at(k + 1);
  return at(k) <= t ? at(k) : at(k - 1);
};
const COLORS = { ward: [20, 184, 166], Q: [255, 140, 26], W: [31, 191, 63], R: [255, 43, 43] };

const browser = await chromium.launch({ executablePath: exe, args: ["--autoplay-policy=no-user-gesture-required"] });
const page = await browser.newPage({ viewport: { width: 1500, height: 950 } });
page.on("pageerror", (e) => console.log("pageerror", e.message));
await page.goto(BASE);
// Only the bubbles on the canvas: no trail, clicks, dot, keys or heatmap.
// The classic trail: its oldest piece stays visible to its end (Rocket fades it to nothing), so it
// shows where the trail ends for the timing checks below.
const only = { trail: false, trailStyle: "classic", clicks: false, dot: false, keys: false, heat: false, bubbles: true, trailSecs: 1, bubbleCats: {}, v: 2 };
await page.evaluate((o) => localStorage.setItem("cv.inputOverlay", JSON.stringify(o)), only);
await page.reload();
await page.waitForSelector("button.card.game");
await page.locator("button.card.game").first().click();
await page.waitForFunction(() => document.querySelector(".vhost video")?.readyState >= 2, null, { timeout: 15000 });

async function seek(t) {
  await page.evaluate(async (t) => {
    const v = document.querySelector(".vhost video");
    v.pause();
    await new Promise((r) => {
      v.addEventListener("seeked", r, { once: true });
      v.currentTime = t;
    });
    for (let i = 0; i < 3; i++) await new Promise((r) => requestAnimationFrame(r));
  }, t);
  await page.waitForTimeout(60);
}
/** Where a game-area position (0..1) is on screen (CSS px of .screen): the letterbox in the
 * video, then object-fit: contain in the player. Computed here, independently of the app. */
async function expectedPx(nx, ny) {
  return page.evaluate(([nx, ny]) => {
    const v = document.querySelector(".vhost video");
    const scr = document.querySelector(".screen").getBoundingClientRect();
    const s = Math.min(scr.width / v.videoWidth, scr.height / v.videoHeight);
    const ox = (scr.width - v.videoWidth * s) / 2;
    const oy = (scr.height - v.videoHeight * s) / 2;
    const f = Math.min(v.videoWidth / 1920, v.videoHeight / 1080);
    const cw = 1920 * f, ch = 1080 * f;
    const cx = (v.videoWidth - cw) / 2, cy = (v.videoHeight - ch) / 2;
    return { x: ox + (cx + nx * cw) * s, y: oy + (cy + ny * ch) * s };
  }, [nx, ny]);
}
/** Pixels of the overlay canvas matching a colour (±tol) or white, inside a box (CSS px). */
async function scan(kind, box = null, tol = 40) {
  return page.evaluate(
    ({ kind, box, tol, COLORS }) => {
      const c = document.querySelector('[data-testid="input-overlay"]');
      if (!c) return { n: 0 };
      const scr = document.querySelector(".screen").getBoundingClientRect();
      const k = c.width / scr.width;
      const d = c.getContext("2d").getImageData(0, 0, c.width, c.height).data;
      const [x0, y0, x1, y1] = box ? box.map((v) => Math.round(v * k)) : [0, 0, c.width, c.height];
      let n = 0, sx = 0, sy = 0, any = 0;
      for (let y = Math.max(0, y0); y < Math.min(c.height, y1); y++)
        for (let x = Math.max(0, x0); x < Math.min(c.width, x1); x++) {
          const i = (y * c.width + x) * 4;
          if (d[i + 3] > 0) any++;
          let ok;
          if (kind === "any") ok = d[i + 3] > 0;
          else if (kind === "white") ok = d[i] > 235 && d[i + 1] > 235 && d[i + 2] > 235 && d[i + 3] > 200;
          else if (kind === "Rfaint") ok = d[i + 3] > 40 && d[i] > 110 && d[i + 1] < 90 && d[i + 2] < 90 && d[i] - d[i + 1] > 70;
          else {
            const [r, g, b] = COLORS[kind];
            ok = d[i + 3] > 200 && Math.abs(d[i] - r) < tol && Math.abs(d[i + 1] - g) < tol && Math.abs(d[i + 2] - b) < tol;
          }
          if (ok) {
            n++;
            sx += x + 0.5;
            sy += y + 0.5;
          }
        }
      return { n, any, x: n ? sx / n / k : null, y: n ? sy / n / k : null };
    },
    { kind, box, tol, COLORS },
  );
}

// 1. Switch on: the bubbles load next to the recording.
await seek(15.0);
const t0 = Date.now();
await page.keyboard.press("i");
await page.waitForFunction(() => (window.__cvBubbleCount ?? 0) > 0, null, { timeout: 5000 });
const loadMs = await page.evaluate(() => [window.__cvOverlayLoadMs, window.__cvBubblesLoadMs, window.__cvBubbleCount]);
check("bubbles loaded with the overlay", loadMs[2] > 100, `${loadMs[2]} presses; overlay on ${loadMs[0]?.toFixed(0)} ms, bubbles ${loadMs[1]?.toFixed(0)} ms after the switch (mock: 60 ms fake delay); ${Date.now() - t0} ms wall`);

// 2. Exact moment and exact position: the ward pressed at 15.65 s (between frames: its frame
// starts at 15.633 s) shows on that frame, not one before; its anchor is on the cursor position
// at 15.65 s, not the frame's.
const tw = 15.65;
const fw = frameOf(tw);
await seek(fw - 0.002);
const e = await expectedPx(...pos(tw));
// Around the anchor (the bubble flashes white on its first frame, so any painted pixel counts).
const near = [e.x - 30, e.y - 60, e.x + 30, e.y + 4];
const before = await scan("any", near);
const wardBefore = await scan("ward", near);
await seek(fw + 0.002);
const onFrame = await scan("any", near);
check("bubble appears on the frame of its key-down, not before", wardBefore.n === 0 && onFrame.n > before.n + 1000, `frame ${fw.toFixed(3)} s for key-down ${tw} s: ${wardBefore.n} ward px near it on the frame before (${before.n} painted, earlier bubbles), ${onFrame.n} painted px on its frame`);
const dot = await scan("white", [e.x - 5, e.y - 5, e.x + 5, e.y + 5]);
const err = dot.n ? Math.hypot(dot.x - e.x, dot.y - e.y) : Infinity;
check("anchor exactly on the cursor position at the key-down", dot.n >= 3 && err <= 1, `expected (${e.x.toFixed(2)}, ${e.y.toFixed(2)}), anchor dot (${dot.x?.toFixed(2)}, ${dot.y?.toFixed(2)}) = ${err.toFixed(2)} px (${dot.n} px)`);
// The frame's own cursor would be 17 ms earlier on the path: the anchor isn't there.
const ef = await expectedPx(...pos(fw));
check("not snapped to the frame's cursor or a sample", Math.hypot(ef.x - e.x, ef.y - e.y) > 1 && err <= 1, `frame-time position is ${Math.hypot(ef.x - e.x, ef.y - e.y).toFixed(2)} px away`);
// Later in its life the anchor hasn't moved (doesn't follow the cursor).
await seek(tw + 0.45);
const dot2 = await scan("white", [e.x - 5, e.y - 5, e.x + 5, e.y + 5]);
const ec = await expectedPx(...pos(tw + 0.45));
check("anchor stays put while the cursor moves on", dot2.n >= 3 && Math.hypot(dot2.x - e.x, dot2.y - e.y) <= 1, `${Math.hypot(dot2.x - e.x, dot2.y - e.y).toFixed(2)} px from the key-down position 0.45 s later (the cursor is ${Math.hypot(ec.x - e.x, ec.y - e.y).toFixed(0)} px away by then)`);
await page.locator(".screen").screenshot({ path: `${OUT}/bubble-ward.png` });

// More key-downs off the frame grid: Q spam at 40.08 s (frame 40.067) and W at 41.53 (41.500).
for (const [t, n0, n1] of [
  [40.08, 2, 3],
  [41.53, null, null],
]) {
  const f = frameOf(t);
  await seek(f - 0.002);
  const a = await page.evaluate(() => window.__cvBubbleStats.drawn);
  await seek(f + 0.002);
  const b = await page.evaluate(() => window.__cvBubbleStats.drawn);
  check(`key-down at ${t} s shows on frame ${f.toFixed(3)}`, b === a + (t === 41.53 ? 2 : 1) && (n0 == null || (a === n0 && b === n1)), `${a} bubbles on the frame before, ${b} on its frame`);
}

// 3. One "Trail & bubbles" time (v1.7), 0.25 s and 3 s, changed live with the slider (no
// reload): the ward bubble and the piece of the cursor trail drawn at its key-down end on the
// same frame: on every frame with the bubble the trail is painted under its anchor, and on the
// first frame without it the trail's end has moved past the key-down point (a point 3 px behind
// it on the path, more than the trail's half width + its round cap, is no longer painted).
// Trail and bubbles are drawn separately (trail only / bubbles only) so the trail pixels under
// the anchor aren't hidden by the bubble's anchor dot; the bubble's timing doesn't depend on
// whether the trail is shown.
await page.locator('[data-testid="overlay-options"]').click();
const slider = page.getByRole("slider", { name: "Trail and bubble time in seconds" });
const region = { x0: e.x - 40, y0: e.y - 60, x1: e.x + 40, y1: e.y + 8 };
const box = [region.x0, region.y0, region.x1, region.y1];
const under = [e.x - 1, e.y - 1, e.x + 1, e.y + 1];
// 3 px behind the key-down point on the cursor's path (earlier positions).
const eb = await (async () => {
  const p1 = await expectedPx(...pos(tw - 0.01));
  const d = Math.hypot(p1.x - e.x, p1.y - e.y);
  return expectedPx(...pos(tw - (0.01 * 3) / d));
})();
const behind = [eb.x - 1, eb.y - 1, eb.x + 1, eb.y + 1];
const trailBox = page.getByRole("checkbox", { name: "Cursor trail" });
const bubbleBox = page.getByRole("checkbox", { name: "Ability bubbles" });
/** Frame k's start in the sample video (30 fps, whole-ms WebM timestamps). */
const frameAt = (k) => Math.round((k * 1000) / 30) / 1000;
for (const secs of [0.25, 3]) {
  await slider.fill(String(secs));
  const saved = await page.evaluate(() => JSON.parse(localStorage.getItem("cv.inputOverlay")));
  // Frames from just before the expected end (the trail's sample after the key-down + secs).
  const kEnd = Math.floor((tw + secs) * 30);
  const rows = [];
  let lastBubble = null, bubbleNoTrail = 0, goneTrailBehind = null, goneTrailAt = null;
  const fr = [];
  for (let k = kEnd - 3; k <= kEnd + 4; k++) {
    const ft = frameAt(k) + 0.0005;
    await trailBox.check();
    await bubbleBox.uncheck();
    await seek(ft);
    const tr = await scan("any", under);
    const tb = await scan("any", behind);
    await trailBox.uncheck();
    await bubbleBox.check();
    await seek(ft);
    const bu = await scan("ward", box, 255);
    fr.push({ k, tr: tr.any, tb: tb.any, bu: bu.any });
    rows.push(`${frameAt(k).toFixed(3)}: trail at/behind ${tr.any}/${tb.any}, bubble ${bu.any}`);
  }
  for (const f of fr) {
    if (f.bu > 10) {
      lastBubble = f.k;
      if (f.tr === 0) bubbleNoTrail++;
    }
  }
  const after = fr.find((f) => lastBubble != null && f.k === lastBubble + 1);
  check(
    `trail & bubbles ${secs} s: the bubble ends on the same frame as its trail piece`,
    saved.trailSecs === secs && !("bubbleFade" in saved) && lastBubble != null && bubbleNoTrail === 0 && after && after.bu === 0 && after.tb === 0 && fr[0].bu > 10,
    `last frame with the bubble ${lastBubble != null ? frameAt(lastBubble).toFixed(3) : "-"} s (key-down ${tw} s + ${secs} s), frames with the bubble but no trail under it: ${bubbleNoTrail}, next frame: trail behind the point ${after?.tb} px; ${rows.join(", ")}; saved ${JSON.stringify({ trailSecs: saved.trailSecs })}`,
  );
  // And it was there for the whole time before: on its frame and half-way.
  await seek(fw + 0.002);
  const first = await scan("ward", box, 255);
  await seek(fw + secs / 2);
  const half = await scan("ward", box, 255);
  check(`trail & bubbles ${secs} s: visible from its frame on`, first.any > 10 && half.any > 10, `${first.any} px on its frame, ${half.any} px ${secs / 2} s later`);
}
// Combined, for the report: trail + bubbles at 1 s, mid-life.
await trailBox.check();
await slider.fill("1");
await seek(fw + 0.5);
await page.locator(".screen").screenshot({ path: `${OUT}/bubble-trail.png` });
await trailBox.uncheck();
// The spam checks below use 3 s (30+ bubbles at once).
await slider.fill("3");
await page.locator(".ovpop .x").click();

// 4. Q spam with a W in the middle, fade 3 s: 30+ bubbles at once, Qs overlap at their own
// positions (only a different action's letter makes a body move), W stays readable.
await seek(42.97);
const spam = await page.evaluate(() => {
  const B = window.__cvBubbles;
  const L = B.lastLayout;
  const p = B.v.presses;
  const A = B.v.actions;
  const base = B.stats.base;
  const geo = (i) => {
    const r = base * (A[p.action[i]].size || 1);
    return { r, lift: r + Math.max(5, base * 0.45), letter: r * 0.62 };
  };
  const t = document.querySelector(".vhost video").currentTime;
  const alive = [];
  for (let i = 0; i < p.t.length; i++) if (p.show[i] <= t && p.show[i] > t - 3 && L.on[i]) alive.push(i);
  const qs = alive.filter((i) => p.action[i] === 0);
  // A Q may only move when, at its birth, a bubble of another action was close enough for its
  // body to cover that bubble's letter; otherwise it stays exactly on its anchor.
  let wrongMoves = 0, overlappingQs = 0, exactQs = 0;
  for (const i of qs) {
    const g = geo(i);
    const near = alive.some((j) => j < i && p.action[j] !== 0 && p.show[j] > p.show[i] - 3 && Math.abs(L.ax[i] - L.ax[j] - L.dx[j]) < g.r + geo(j).letter && Math.abs(L.ay[i] - g.lift - (L.ay[j] - geo(j).lift)) < g.r + geo(j).letter);
    if (L.dx[i] !== 0 && !near) wrongMoves++;
    if (L.dx[i] === 0) exactQs++;
    if (qs.some((j) => j < i && Math.hypot(L.ax[i] - L.ax[j], L.ay[i] - L.ay[j]) < g.r)) overlappingQs++;
  }
  const w = alive.find((i) => p.action[i] === 1 && Math.abs(p.t[i] - 41.53) < 1e-6);
  const gw = geo(w);
  return { drawn: window.__cvBubbleStats.drawn, qs: qs.length, wrongMoves, exactQs, overlappingQs, wAnchor: [L.ax[w], L.ay[w]], wBody: [L.ax[w] + L.dx[w], L.ay[w] - gw.lift], letter: gw.letter };
});
check("30+ bubbles on screen in the spam", spam.drawn >= 30, `${spam.drawn} drawn, ${spam.qs} of them Q`);
check("Q spam: same-action bubbles overlap at their own positions", spam.wrongMoves === 0 && spam.overlappingQs >= 20, `${spam.overlappingQs} Qs overlap an earlier Q, ${spam.exactQs}/${spam.qs} exactly on their anchor, ${spam.wrongMoves} moved without another action's letter under them`);
const ew = await expectedPx(...pos(41.53));
check("W's anchor exact in the spam", Math.hypot(spam.wAnchor[0] - ew.x, spam.wAnchor[1] - ew.y) <= 0.5, `anchor (${spam.wAnchor[0].toFixed(2)}, ${spam.wAnchor[1].toFixed(2)}) vs expected (${ew.x.toFixed(2)}, ${ew.y.toFixed(2)})`);
// W's letter: once it settled after its punch-in (0.4 s) and after the spam (17 more Qs drawn on
// top since, the last one settled too: a new bubble's punch-in may cover a neighbour's letter
// edge for ~0.1 s): the same white letter, no Q colour on it.
const lb = [spam.wBody[0] - spam.letter, spam.wBody[1] - spam.letter, spam.wBody[0] + spam.letter, spam.wBody[1] + spam.letter];
await seek(41.93);
const lFirst = await scan("white", lb);
const bFirst = await scan("Q", lb);
await seek(43.15);
const lLate = await scan("white", lb);
const bLate = await scan("Q", lb);
await page.locator(".screen").screenshot({ path: `${OUT}/bubble-spam.png` });
check("W in the middle of Q spam stays readable", lFirst.n > 10 && Math.abs(lLate.n - lFirst.n) <= 2 && bLate.n === 0, `letter area: ${lFirst.n} white px once it settled, ${lLate.n} after 17 more Qs; Q-colour px on it ${bFirst.n} → ${bLate.n}`);

// 5. Draw time while playing through the spam (fade 3 s, 30+ bubbles) and the layout time.
const draw = await page.evaluate(async () => {
  const v = document.querySelector(".vhost video");
  v.currentTime = 41.8;
  await new Promise((r) => v.addEventListener("seeked", r, { once: true }));
  const n0 = (window.__cvOverlayDrawMs ?? []).length;
  await v.play();
  let maxDrawn = 0;
  const w0 = performance.now();
  while (performance.now() - w0 < 2000) {
    await new Promise((r) => requestAnimationFrame(r));
    maxDrawn = Math.max(maxDrawn, window.__cvBubbleStats?.drawn ?? 0);
  }
  v.pause();
  const all = [...(window.__cvOverlayDrawMs ?? [])];
  return { d: all.slice(-Math.min(all.length, 120)).sort((a, b) => a - b), maxDrawn, layoutMs: window.__cvBubbleStats.layoutMs, n0 };
});
const p = (q) => draw.d[Math.min(draw.d.length - 1, Math.floor(q * draw.d.length))];
check("draw < 2 ms per frame with 30+ bubbles", draw.d.length > 30 && draw.maxDrawn >= 30 && p(0.95) < 2, `median ${p(0.5)?.toFixed(3)} ms, p95 ${p(0.95)?.toFixed(3)} ms, max ${draw.d[draw.d.length - 1]?.toFixed(3)} ms over ${draw.d.length} frames, up to ${draw.maxDrawn} bubbles; layout ${draw.layoutMs.toFixed(2)} ms (once)`);

// 6. Ult tie-in: R at 31.3 s confirmed (solid), R at 38.3 s "no cast" (hidden until the
// timeline's "Unconfirmed presses" filter is on, then outlined).
await page.evaluate((o) => localStorage.setItem("cv.inputOverlay", JSON.stringify(o)), only);
await page.locator('[data-testid="overlay-options"]').click();
await slider.fill("1");
await page.locator(".ovpop .x").click();
await seek(31.5);
const rUsed = await scan("R");
await seek(38.5);
const rHidden = await scan("Rfaint");
await page.getByRole("button", { name: /Unconfirmed presses/ }).click();
await seek(38.51);
const rShown = await scan("Rfaint");
const rShownSolid = await scan("R");
await page.locator(".screen").screenshot({ path: `${OUT}/bubble-unconfirmed.png` });
check("ult used: solid R bubble", rUsed.n > 80, `${rUsed.n} red px`);
check("ult pressed, no cast: hidden with the filter off, outlined with it on", rHidden.n === 0 && rShown.n > 5 && rShownSolid.n < rUsed.n * 0.3, `${rHidden.n} px with the filter off; with it on ${rShown.n} faint red px, ${rShownSolid.n} solid (solid bubble: ${rUsed.n})`);
await page.getByRole("button", { name: /Unconfirmed presses/ }).click();

// 7. Sub-toggles: Items off hides item bubbles; Ability bubbles off hides all; remembered.
await page.locator('[data-testid="overlay-options"]').click();
await page.getByRole("checkbox", { name: "Ward" }).uncheck();
await seek(fw + 0.3);
const wardOff = await scan("ward");
await page.getByRole("checkbox", { name: "Ward" }).check();
await page.getByRole("checkbox", { name: "Ability bubbles" }).uncheck();
await seek(42.0);
const allOff = await page.evaluate(() => {
  const c = document.querySelector('[data-testid="input-overlay"]');
  const d = c.getContext("2d").getImageData(0, 0, c.width, c.height).data;
  let n = 0;
  for (let i = 3; i < d.length; i += 4) if (d[i]) n++;
  return n;
});
const stored = await page.evaluate(() => JSON.parse(localStorage.getItem("cv.inputOverlay")));
check("category toggle and group toggle", wardOff.n === 0 && allOff === 0 && stored.bubbles === false && stored.bubbleCats.ward === true, `ward px with Ward off: ${wardOff.n}; painted px with bubbles off: ${allOff}`);
await page.getByRole("checkbox", { name: "Ability bubbles" }).check();
await page.locator(".ovpop .x").click();

// 8. Overlay off: no canvas, no drawing loop.
await page.keyboard.press("i");
await page.waitForTimeout(150);
const off = await page.evaluate(async () => {
  const n0 = (window.__cvOverlayDrawMs ?? []).length;
  const v = document.querySelector(".vhost video");
  await v.play();
  await new Promise((r) => setTimeout(r, 500));
  v.pause();
  return { canvas: !!document.querySelector('[data-testid="input-overlay"]'), draws: (window.__cvOverlayDrawMs ?? []).length - n0 };
});
check("overlay off: no canvas, nothing drawn", !off.canvas && off.draws === 0, JSON.stringify(off));

await browser.close();
const failed = results.filter((r) => !r.ok);
console.log(`\n${results.length - failed.length}/${results.length} passed`);
process.exit(failed.length ? 1 : 0);
