// v1.8 time-synced scoreboard in Chromium against the mock backend (`VITE_MOCK=1 npx vite`):
// the card under the player follows playback, seeking and scrubbing; O shows it over the video;
// a game recorded before v1.8 shows the own final numbers only.
//
//   node tests/scoreboard.test.mjs [http://localhost:5173] [outdir]
import { chromium } from "playwright-core";
import fs from "node:fs";

const BASE = process.argv[2] ?? "http://localhost:5173";
const OUT = process.argv[3] ?? "/tmp/scoreboard";
fs.mkdirSync(OUT, { recursive: true });
const exe = process.env.CHROME ?? "/opt/pw-browsers/chromium-1194/chrome-linux/chrome";
const results = [];
const check = (name, ok, detail = "") => {
  results.push({ name, ok, detail });
  console.log(`${ok ? "PASS" : "FAIL"} ${name}${detail ? " — " + detail : ""}`);
};
const browser = await chromium.launch({ executablePath: exe, args: ["--autoplay-policy=no-user-gesture-required"] });
const OFFSET = 20; // the mock game's video offset

async function open(page, nth) {
  await page.goto(`${BASE}/`);
  await page.waitForSelector("button.card.game");
  await page.locator("button.card.game").nth(nth).click();
  await page.waitForFunction(() => document.querySelector(".vhost video")?.readyState >= 2, null, { timeout: 20000 });
  await page.evaluate(() => document.querySelector(".vhost video").pause());
}
const myRow = (page) =>
  page.evaluate(() => {
    const card = document.querySelector('.sbcard [data-testid="scoreboard"]');
    const me = card.querySelector(".row.me");
    return { frame: Number(card.dataset.frame), kda: me.querySelector(".kda").textContent.trim(), cs: me.querySelector(".cs").textContent.trim(), lv: me.querySelector(".lv")?.textContent.trim(), items: me.querySelectorAll(".items .gi").length, rows: card.querySelectorAll('[data-testid="sb-row"]').length };
  });
async function seekGame(page, gt) {
  await page.evaluate((t) => new Promise((res) => {
    const v = document.querySelector(".vhost video");
    v.addEventListener("seeked", () => res(), { once: true });
    v.currentTime = t;
  }), gt + OFFSET);
  await page.waitForTimeout(150);
}

for (const theme of ["dark", "light"]) {
  const ctx = await browser.newContext({ viewport: { width: 1600, height: 1000 }, colorScheme: theme });
  const page = await ctx.newPage();
  page.on("pageerror", (e) => console.log("pageerror", e.message));
  await open(page, 0);
  await page.waitForSelector(".sbcard");

  await seekGame(page, 5);
  const a = await myRow(page);
  check(`${theme}: 10 players, the state at 0:05`, a.rows === 10 && a.kda === "0/0/0" && a.frame === 0 && a.lv === "1", JSON.stringify(a));
  await seekGame(page, 85);
  const b = await myRow(page);
  check(`${theme}: at 1:25 your 4 kills, levels and items`, b.kda === "4/1/2" && b.frame === 8 && Number(b.lv) > 1 && b.items > a.items, JSON.stringify(b));
  await page.locator(".sbcard").scrollIntoViewIfNeeded();
  await page.screenshot({ path: `${OUT}/${theme}-card.png`, fullPage: false });
  // Back in time: earlier numbers again.
  await seekGame(page, 40);
  const c = await myRow(page);
  check(`${theme}: seeking back shows the earlier state`, c.kda === "1/1/1" && c.frame === 4, JSON.stringify(c));

  // Scrubbing: drag along the track; the scoreboard follows during the drag.
  const tr = await page.locator('[data-testid="timeline"] .track').boundingBox();
  await page.locator('[data-testid="timeline"]').scrollIntoViewIfNeeded();
  const tr2 = await page.locator('[data-testid="timeline"] .track').boundingBox();
  const dur = await page.evaluate(() => document.querySelector(".vhost video").duration);
  const x = (vt) => tr2.x + (vt / dur) * tr2.width;
  const y = tr2.y + tr2.height / 2;
  await page.mouse.move(x(OFFSET + 2), y);
  await page.mouse.down();
  const seen = new Set();
  const lag = [];
  for (let vt = OFFSET + 2; vt <= OFFSET + 125; vt += 6) {
    await page.mouse.move(x(vt), y);
    const t0 = Date.now();
    // The card shows the frame of this time within a few animation frames.
    const want = Math.min(13, Math.floor((vt - OFFSET) / 10));
    let f = -1;
    for (let k = 0; k < 40; k++) {
      f = await page.evaluate(() => Number(document.querySelector('.sbcard [data-testid="scoreboard"]').dataset.frame));
      if (f === want) break;
      await page.waitForTimeout(10);
    }
    lag.push(Date.now() - t0);
    seen.add(f);
  }
  await page.mouse.up();
  lag.sort((p, q) => p - q);
  check(`${theme}: follows while scrubbing (every 10 s read seen, p95 lag)`, seen.size >= 12, `${seen.size} frames seen, lag median ${lag[lag.length >> 1]} ms, p95 ${lag[Math.floor(lag.length * 0.95)]} ms`);
  void tr;

  // O: over the video, following the same time; O again hides it.
  await page.keyboard.press("o");
  await page.waitForTimeout(150);
  const ov = await page.locator('[data-testid="scoreboard-overlay"] [data-testid="sb-row"]').count();
  await page.screenshot({ path: `${OUT}/${theme}-overlay.png` });
  await page.keyboard.press("o");
  await page.waitForTimeout(100);
  const ov2 = await page.locator('[data-testid="scoreboard-overlay"]').count();
  check(`${theme}: O toggles the scoreboard over the video`, ov === 10 && ov2 === 0, `${ov} rows, then ${ov2}`);
  // Tab outside fullscreen keeps moving the focus (no overlay).
  await page.keyboard.down("Tab");
  const ov3 = await page.locator('[data-testid="scoreboard-overlay"]').count();
  await page.keyboard.up("Tab");
  check(`${theme}: Tab outside fullscreen doesn't take over`, ov3 === 0);
  // Icons come from the game's Data Dragon version.
  const src = await page.locator('.sbcard .row.me .items .gi').first().getAttribute("data-src");
  check(`${theme}: item icons from the game's Data Dragon version`, src === "https://ddragon.leagueoflegends.com/cdn/16.20.1/img/item/1056.png", String(src));
  await ctx.close();
}

// A game recorded before v1.8 (mock's third game): the final numbers only.
{
  const ctx = await browser.newContext({ viewport: { width: 1400, height: 900 } });
  const page = await ctx.newPage();
  await open(page, 2);
  const r = await page.evaluate(() => {
    const card = document.querySelector('.sbcard [data-testid="scoreboard"]');
    return { head: card.querySelector(".head").textContent, rows: card.querySelectorAll('[data-testid="sb-row"]').length, kda: card.querySelector(".kda").textContent.trim() };
  });
  check("old recording: final scoreboard (your numbers) only", r.rows === 1 && r.head.includes("Final scoreboard") && r.kda === "5/2/3", JSON.stringify(r));
  await ctx.close();
}

// Narrow window: teams stacked, nothing cut.
{
  const ctx = await browser.newContext({ viewport: { width: 940, height: 560 } });
  const page = await ctx.newPage();
  await open(page, 0);
  const r = await page.evaluate(() => {
    const card = document.querySelector('.sbcard [data-testid="scoreboard"]');
    const teams = [...card.querySelectorAll(".team")].map((t) => t.getBoundingClientRect());
    const rows = [...card.querySelectorAll(".row")];
    const over = rows.filter((row) => row.scrollWidth > row.clientWidth + 1).length;
    return { stacked: teams[1].top >= teams[0].bottom - 1, over };
  });
  check("940 px window: teams stacked, rows fit", r.stacked && r.over === 0, JSON.stringify(r));
  await page.locator(".sbcard").scrollIntoViewIfNeeded();
  await page.screenshot({ path: `${OUT}/narrow.png` });
  await ctx.close();
}

await browser.close();
fs.writeFileSync(`${OUT}/results.json`, JSON.stringify(results, null, 2));
const failed = results.filter((r) => !r.ok);
console.log(`${results.length - failed.length}/${results.length} passed`);
process.exit(failed.length ? 1 : 0);
