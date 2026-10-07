// v1.8 timeline in Chromium against the mock backend (`VITE_MOCK=1 npx vite`): the APM chart
// behind the markers (drawn, hover value, never in the way of chips or seeking) and the richer
// hover cards (item / summoner icons, facts, champion portraits).
//
//   node tests/timelinechips.test.mjs [http://localhost:5173] [outdir]
import { chromium } from "playwright-core";
import fs from "node:fs";

const BASE = process.argv[2] ?? "http://localhost:5173";
const OUT = process.argv[3] ?? "/tmp/timelinechips";
fs.mkdirSync(OUT, { recursive: true });
const exe = process.env.CHROME ?? "/opt/pw-browsers/chromium-1194/chrome-linux/chrome";
const results = [];
const check = (name, ok, detail = "") => {
  results.push({ name, ok, detail });
  console.log(`${ok ? "PASS" : "FAIL"} ${name}${detail ? " — " + detail : ""}`);
};
const browser = await chromium.launch({ executablePath: exe, args: ["--autoplay-policy=no-user-gesture-required"] });
// The mock game: video offset 20 s, APM bins (lib/mock.ts MOCK_APM).
const OFFSET = 20;
const APM = [null, null, 96, 142, 168, 150, 131, 205, 262, 188, 140, 176, 159, 120, 84];

for (const theme of ["dark", "light"]) {
  const ctx = await browser.newContext({ viewport: { width: 1600, height: 900 }, colorScheme: theme });
  const page = await ctx.newPage();
  page.on("pageerror", (e) => console.log("pageerror", e.message));
  await page.goto(`${BASE}/`);
  await page.evaluate(() => localStorage.removeItem("cv.player"));
  await page.reload();
  await page.waitForSelector("button.card.game");
  await page.locator("button.card.game").first().click();
  await page.waitForFunction(() => document.querySelector(".vhost video")?.readyState >= 2, null, { timeout: 20000 });
  await page.evaluate(() => document.querySelector(".vhost video").pause());
  await page.waitForTimeout(300);

  const geo = await page.evaluate(() => {
    const tl = document.querySelector('[data-testid="timeline"]');
    const tr = tl.querySelector(".track").getBoundingClientRect();
    const mk = tl.querySelector(".markers").getBoundingClientRect();
    const v = document.querySelector(".vhost video");
    return { tx: tr.left, tw: tr.width, ty: tr.top, th: tr.height, my: mk.top, mh: mk.height, dur: v.duration, apm: tl.querySelector(".markers").dataset.apm };
  });
  const drawn = await page.evaluate(() => document.querySelector('[data-testid="timeline"]').dataset.apmBins);
  check(`${theme}: timeline has the APM chart`, geo.apm === "1" && drawn === String(APM.filter((a) => a != null).length), `bins drawn ${drawn}`);
  const xOf = (t) => geo.tx + (t / geo.dur) * geo.tw;

  // 1. Drawn: the canvas has faint pixels above the baseline at a busy bin (80-90 s: 262 APM)
  // and none high up at an unfocused bin (0-20 s). Away from markers: sample the top half.
  const alphaAt = (t) =>
    page.evaluate(
      ({ x }) => {
        const c = document.querySelector(".timeline .mkcv");
        const r = c.getBoundingClientRect();
        const g = c.getContext("2d");
        const sx = Math.round(((x - r.left) / r.width) * c.width);
        let best = 0;
        let topmost = c.height;
        for (let y = 0; y < c.height; y++) {
          const a = g.getImageData(sx, y, 1, 1).data[3];
          if (a > 0 && y < topmost) topmost = y;
          best = Math.max(best, a);
        }
        return { best, topFrac: topmost / c.height };
      },
      { x: xOf(t) },
    );
  // Bins without markers nearby: 5 s (unfocused), 85 s busy is next to the triple kill (98-100)…
  // use 25 s (96 APM) vs 0-20 s.
  const empty = await alphaAt(5);
  const busy = await alphaAt(115);
  check(`${theme}: chart drawn where there's APM, nothing where unfocused`, busy.best > 0 && busy.best < 140 && empty.best === 0, `busy alpha ${busy.best} top ${busy.topFrac.toFixed(2)}, unfocused alpha ${empty.best}`);
  await page.screenshot({ path: `${OUT}/${theme}-chart.png`, clip: { x: geo.tx - 20, y: geo.my - 20, width: geo.tw + 40, height: geo.mh + geo.th + 40 } });

  // 2. Hover over an empty part of the marker row: the time label shows that bin's APM.
  await page.mouse.move(xOf(115), geo.my + 4);
  await page.waitForTimeout(150);
  const lbl = await page.locator('[data-testid="hovertime"]').textContent().catch(() => "");
  check(`${theme}: hovering the chart shows the APM there`, lbl.includes(`${APM[11]} APM`), JSON.stringify(lbl));
  await page.mouse.move(xOf(8), geo.ty + geo.th / 2);
  await page.waitForTimeout(150);
  const lbl2 = await page.locator('[data-testid="hovertime"]').textContent().catch(() => "");
  check(`${theme}: no APM where the game wasn't focused`, !lbl2.includes("APM"), JSON.stringify(lbl2));

  // 3. Chips still get the pointer over the chart: hover the item chip -> its card with facts.
  const item = page.locator('.timeline .mk[aria-label^="Completed Luden"]');
  await item.hover();
  await page.waitForTimeout(200);
  const card = await page.locator(".timeline .tip").textContent().catch(() => "");
  check(`${theme}: item chip card: name, cost, components`, card.includes("Completed Luden's Echo") && card.includes("2750 gold") && card.includes("Built from"), card.slice(0, 160));
  const icon = await page.locator(".timeline .tip .gi").first().getAttribute("data-src").catch(() => null);
  check(`${theme}: item icon from Data Dragon (game's version)`, icon === "https://ddragon.leagueoflegends.com/cdn/16.20.1/img/item/6655.png", String(icon));
  await page.screenshot({ path: `${OUT}/${theme}-item-card.png` });
  const spell = page.locator('.timeline .mk[aria-label^="Flash"]');
  await spell.hover();
  await page.waitForTimeout(200);
  const scard = await page.locator(".timeline .tip").textContent().catch(() => "");
  const sicon = await page.locator(".timeline .tip .gi").first().getAttribute("data-src").catch(() => null);
  check(`${theme}: summoner chip card: spell name, key, icon`, scard.includes("Flash") && scard.includes("Summoner spell") && sicon?.endsWith("/img/spell/SummonerFlash.png"), `${scard.slice(0, 120)} ${sicon}`);
  const tower = page.locator('.timeline .mk[aria-label^="Destroyed the mid outer"]');
  await tower.hover();
  await page.waitForTimeout(200);
  const tcard = await page.locator(".timeline .tip").textContent().catch(() => "");
  const who = await page.locator(".timeline .tip .tip-who .gi").count();
  check(`${theme}: tower card: lane, tier, gold, who`, ["Mid", "Outer turret", "≈ +250", "Last hit"].every((s) => tcard.includes(s)) && who === 2, `${tcard.slice(0, 160)} portraits ${who}`);
  await page.screenshot({ path: `${OUT}/${theme}-tower-card.png` });

  // 4. Clicking a chip still jumps (5 s before it), and the track still seeks.
  await item.click();
  await page.waitForTimeout(400);
  const t1 = await page.evaluate(() => document.querySelector(".vhost video").currentTime);
  check(`${theme}: clicking a chip over the chart jumps to it`, Math.abs(t1 - (40.2 + OFFSET - 5)) < 1.2, `t ${t1.toFixed(2)}`);
  await page.mouse.click(xOf(115), geo.ty + geo.th / 2);
  await page.waitForTimeout(400);
  const t2 = await page.evaluate(() => document.querySelector(".vhost video").currentTime);
  check(`${theme}: clicking the track under the chart seeks`, Math.abs(t2 - 115) < 1.5, `t ${t2.toFixed(2)}`);

  // 5. Event list: the item / spell rows show their icon and facts.
  const rows = await page.evaluate(() => [...document.querySelectorAll(".ev-list .ev")].filter((b) => b.querySelector(".gi")).map((b) => b.textContent));
  check(`${theme}: event list rows with icons + facts`, rows.length === 3 && rows.some((r) => r.includes("Cost: 2750 gold")), JSON.stringify(rows).slice(0, 200));

  await ctx.close();
}

// A game without input data (the mock's third game): no chart, no APM in the label.
{
  const ctx = await browser.newContext({ viewport: { width: 1400, height: 850 } });
  const page = await ctx.newPage();
  await page.goto(`${BASE}/`);
  await page.waitForSelector("button.card.game");
  await page.locator("button.card.game").nth(2).click();
  await page.waitForSelector('[data-testid="timeline"]');
  await page.waitForTimeout(500);
  const r = await page.evaluate(() => {
    const m = document.querySelector(".timeline .markers");
    return { apm: m.dataset.apm ?? null, bins: document.querySelector('[data-testid="timeline"]').dataset.apmBins };
  });
  check("game without input data: no chart drawn", r.apm == null && r.bins === "0", JSON.stringify(r));
  await ctx.close();
}

await browser.close();
fs.writeFileSync(`${OUT}/results.json`, JSON.stringify(results, null, 2));
const failed = results.filter((r) => !r.ok);
console.log(`${results.length - failed.length}/${results.length} passed`);
process.exit(failed.length ? 1 : 0);
