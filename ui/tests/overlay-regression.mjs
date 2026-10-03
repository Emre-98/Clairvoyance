// Regression check: the input overlay draws exactly the same pixels as an older build, at a set
// of times. Both builds run against the mock backend (`VITE_MOCK=1 npx vite --port ...`).
// - bubbles off, default options and everything on (trail 3 s): pixel-identical (v1.4.0 on);
// - v1.7 (one "Trail & bubbles" time, bubbles end with their trail piece) against v1.6.x with
//   bubbles on: identical while every bubble on screen is popping in or holding (only the end of
//   a bubble's life changed), with the trail and the bubbles at the same time in both builds
//   (old options: trailSecs = bubbleFade, migrated to the one time).
//
//   node tests/overlay-regression.mjs <old url> <new url>
import { chromium } from "playwright-core";

const [OLD, NEW] = [process.argv[2] ?? "http://localhost:5174", process.argv[3] ?? "http://localhost:5173"];
const exe = process.env.CHROME ?? "/opt/pw-browsers/chromium-1194/chrome-linux/chrome";
const TIMES = [5, 10.3, 33.4, 37.5, 41.6, 61.2, 99.9, 140];
// Bubbles at s + 0.3 every second (s = 1, 2, ...), wards at s + 0.65 (s % 10 = 5), Q spam 40-43 s:
// these times only show bubbles 0.0-0.2 s old (pop-in / hold) and none near their end.
const HOLD_TIMES = [2.35, 12.45, 33.4, 61.45, 99.5, 140.45];
const OPTIONS = {
  default: [{ bubbles: false }, TIMES],
  everything: [{ trail: true, trailSecs: 3, clicks: true, dot: true, keys: true, heat: true, heatRange: "game", bubbles: false }, TIMES],
  "bubbles+trail 1 s": [{ trail: true, trailSecs: 1, bubbleFade: 1, bubbles: true }, HOLD_TIMES],
  "bubbles+trail 0.5 s": [{ trail: true, trailSecs: 0.5, bubbleFade: 0.5, bubbles: true, keys: true }, HOLD_TIMES],
};

async function capture(url) {
  const browser = await chromium.launch({ executablePath: exe });
  const page = await browser.newPage({ viewport: { width: 1500, height: 950 }, deviceScaleFactor: 1 });
  const out = {};
  for (const [name, [opts, times]] of Object.entries(OPTIONS)) {
    await page.goto(url);
    await page.evaluate((o) => localStorage.setItem("cv.inputOverlay", JSON.stringify(o)), opts);
    await page.reload();
    await page.waitForSelector("button.card.game");
    await page.locator("button.card.game").first().click();
    await page.waitForFunction(() => document.querySelector(".vhost video")?.readyState >= 2, null, { timeout: 15000 });
    await page.locator('[data-testid="overlay-toggle"]').click();
    await page.waitForSelector('[data-testid="input-overlay"]');
    await page.waitForTimeout(400);
    for (const t of times) {
      const px = await page.evaluate(async (t) => {
        const v = document.querySelector(".vhost video");
        v.pause();
        await new Promise((r) => {
          v.addEventListener("seeked", r, { once: true });
          v.currentTime = t;
        });
        for (let i = 0; i < 4; i++) await new Promise((r) => requestAnimationFrame(r));
        const c = document.querySelector('[data-testid="input-overlay"]');
        const d = c.getContext("2d").getImageData(0, 0, c.width, c.height).data;
        let n = 0;
        // Checksum of the raw RGBA bytes (FNV-1a) + the painted pixel count.
        let h = 2166136261;
        for (let i = 0; i < d.length; i++) {
          h ^= d[i];
          h = Math.imul(h, 16777619) >>> 0;
          if ((i & 3) === 3 && d[i]) n++;
        }
        return { w: c.width, h: c.height, n, sum: h.toString(16) };
      }, t);
      out[`${name}@${t}`] = { w: px.w, h: px.h, painted: px.n, hash: px.sum };
    }
  }
  await browser.close();
  return out;
}

const a = await capture(OLD);
const b = await capture(NEW);
let fails = 0;
for (const k of Object.keys(a)) {
  const x = a[k], y = b[k];
  const same = x.w === y.w && x.h === y.h && x.hash === y.hash && x.painted === y.painted;
  if (!same) fails++;
  console.log(`${same ? "PASS" : "FAIL"} ${k}: ${x.w}x${x.h}, ${x.painted} painted px (new ${y.painted}), checksum old ${x.hash} new ${y.hash}`);
}
console.log(`\n${Object.keys(a).length - fails}/${Object.keys(a).length} identical`);
process.exit(fails ? 1 : 0);
