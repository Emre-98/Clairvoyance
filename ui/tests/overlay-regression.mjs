// Regression check: with the ability bubbles off, the input overlay draws exactly the same
// pixels as an older build (v1.4.0), for the default options and with everything on, at a set of
// times. Both builds run against the mock backend (`VITE_MOCK=1 npx vite --port ...`).
//
//   node tests/overlay-regression.mjs <old url> <new url>
import { chromium } from "playwright-core";

const [OLD, NEW] = [process.argv[2] ?? "http://localhost:5174", process.argv[3] ?? "http://localhost:5173"];
const exe = process.env.CHROME ?? "/opt/pw-browsers/chromium-1194/chrome-linux/chrome";
const TIMES = [5, 10.3, 33.4, 37.5, 41.6, 61.2, 99.9, 140];
const OPTIONS = {
  default: { bubbles: false },
  everything: { trail: true, trailSecs: 3, clicks: true, dot: true, keys: true, heat: true, heatRange: "game", bubbles: false },
};

async function capture(url) {
  const browser = await chromium.launch({ executablePath: exe });
  const page = await browser.newPage({ viewport: { width: 1500, height: 950 }, deviceScaleFactor: 1 });
  const out = {};
  for (const [name, opts] of Object.entries(OPTIONS)) {
    await page.goto(url);
    await page.evaluate((o) => localStorage.setItem("cv.inputOverlay", JSON.stringify(o)), opts);
    await page.reload();
    await page.waitForSelector("button.card.game");
    await page.locator("button.card.game").first().click();
    await page.waitForFunction(() => document.querySelector(".vhost video")?.readyState >= 2, null, { timeout: 15000 });
    await page.locator('[data-testid="overlay-toggle"]').click();
    await page.waitForSelector('[data-testid="input-overlay"]');
    await page.waitForTimeout(400);
    for (const t of TIMES) {
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
