// Timeline filters (2026-10-07): kills, deaths and assists are shown by default, every other
// group is a greyed chip that can be switched on, and the choice is remembered between replays.
// Chromium against the mock backend (`VITE_MOCK=1 npx vite`).
//
//   node tests/filters.test.mjs [http://localhost:5173] [outdir]
import { chromium } from "playwright-core";
import fs from "node:fs";

const BASE = process.argv[2] ?? "http://localhost:5173";
const OUT = process.argv[3] ?? "/tmp/filters";
fs.mkdirSync(OUT, { recursive: true });
const exe = process.env.CHROME ?? "/opt/pw-browsers/chromium-1194/chrome-linux/chrome";
const results = [];
const check = (name, ok, detail = "") => {
  results.push({ name, ok, detail });
  console.log(`${ok ? "PASS" : "FAIL"} ${name}${detail ? " — " + detail : ""}`);
};
const browser = await chromium.launch({ executablePath: exe, args: ["--autoplay-policy=no-user-gesture-required"] });
const ctx = await browser.newContext({ viewport: { width: 1600, height: 900 } });
const page = await ctx.newPage();
page.on("pageerror", (e) => console.log("pageerror", e.message));

async function openGame() {
  await page.waitForSelector("button.card.game");
  await page.locator("button.card.game").first().click();
  await page.waitForSelector(".filters .chip");
  await page.waitForTimeout(300);
}
const chips = () =>
  page.$$eval(".filters .chip", (els) => els.map((e) => ({ label: e.textContent.replace(/\d+/g, "").trim(), on: e.getAttribute("aria-pressed") === "true", title: e.title })));
/** Kinds of the markers drawn on the timeline (their hit targets carry the event's label). */
const markers = () => page.$$eval(".timeline .mk", (els) => els.length);

await page.goto(`${BASE}/?video=sample169`);
await page.evaluate(() => localStorage.removeItem("cv.timelineFilters"));
await page.reload();
await openGame();

let c = await chips();
const on = c.filter((x) => x.on).map((x) => x.label);
check("only kills, deaths and assists are on by default", on.length > 0 && on.every((l) => ["Kills", "Deaths", "Assists"].includes(l)), `on: ${on.join(", ")}; off: ${c.filter((x) => !x.on).map((x) => x.label).join(", ")}`);
check("the other groups are there as greyed chips", c.some((x) => !x.on), `${c.filter((x) => !x.on).length} off`);
const off = c.find((x) => !x.on);
check("a greyed chip says it can be shown", !!off && off.title.startsWith("Show "), off?.title);
await page.screenshot({ path: `${OUT}/default.png` });

// Switching one on adds its markers, and it stays on for the next replay.
const before = await markers();
await page.locator(`.filters .chip[title="${off.title}"]`).click();
await page.waitForTimeout(300);
const after = await markers();
check(`switching "${off.label}" on adds its markers`, after > before, `${before} -> ${after} markers`);
await page.reload();
await openGame();
c = await chips();
check(`"${off.label}" is still on after reopening`, c.find((x) => x.label === off.label)?.on === true);
check("kills, deaths and assists are still on", ["Kills", "Deaths", "Assists"].filter((l) => c.some((x) => x.label === l)).every((l) => c.find((x) => x.label === l).on));

// Switching kills off is remembered too.
await page.locator('.filters .chip[title="Hide kills"]').click();
await page.reload();
await openGame();
c = await chips();
check("switching kills off is remembered", c.find((x) => x.label === "Kills")?.on === false);

await browser.close();
const failed = results.filter((r) => !r.ok).length;
console.log(`${results.length - failed}/${results.length} passed`);
fs.writeFileSync(`${OUT}/results.json`, JSON.stringify(results, null, 2));
process.exit(failed ? 1 : 0);
