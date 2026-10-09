// Share (2026-10-08): the Share button on clips puts the clip on the clipboard (a copy under
// 18 MB with "Fit for Discord" on), the tick is on by default and is the same saved setting as
// in Settings, and the button fits on the clip cards at the smallest window size.
// Chromium against the mock backend (`VITE_MOCK=1 npx vite`).
//
//   node tests/share.test.mjs [http://localhost:5173] [outdir]
import { chromium } from "playwright-core";
import fs from "node:fs";

const BASE = process.argv[2] ?? "http://localhost:5173";
const OUT = process.argv[3] ?? "/tmp/share";
fs.mkdirSync(OUT, { recursive: true });
const exe = process.env.CHROME ?? "/opt/pw-browsers/chromium-1194/chrome-linux/chrome";
const results = [];
const check = (name, ok, detail = "") => {
  results.push({ name, ok, detail });
  console.log(`${ok ? "PASS" : "FAIL"} ${name}${detail ? " — " + detail : ""}`);
};
const browser = await chromium.launch({ executablePath: exe });
const ctx = await browser.newContext({ viewport: { width: 1600, height: 900 } });
const page = await ctx.newPage();
page.on("pageerror", (e) => console.log("pageerror", e.message));

const nav = async (label) => {
  await page.locator(`nav button.nav:has-text("${label}")`).click();
  await page.waitForTimeout(300);
};
const lastToast = async () => {
  await page.waitForSelector(".toast", { timeout: 5000 });
  return page.$$eval(".toast", (els) => els.at(-1).textContent.trim());
};
const fitBox = () => page.locator(".fitdiscord input");

await page.goto(BASE);
await nav("Clips");
await page.waitForSelector(".card.clip .sharebtn");
check("Fit for Discord is ticked by default", await fitBox().isChecked());

// Share with the tick on: a Discord copy, copied.
await page.locator(".card.clip .sharebtn").first().click();
const busy = await page.locator(".card.clip .sharebtn").first().textContent();
check("the button says it's preparing while it works", busy.includes("Preparing"), busy.trim());
let t = await lastToast();
check("sharing says it's copied and how to paste it", t.startsWith("Copied! Paste it in Discord with Ctrl+V"), t);
check("the toast gives the copy's size", t.includes("16.8 MB"), t);
check("the copy fits Discord (under 18 MB, 30 fps)", /1[0-9](\.\d)? MB/.test(t) && t.includes("30 fps"), t);
await page.screenshot({ path: `${OUT}/shared.png` });

// Untick: the original clip, and Settings shows the same setting off.
await fitBox().uncheck();
await page.waitForTimeout(200);
await page.waitForFunction(() => document.querySelectorAll(".toast").length === 0, null, { timeout: 8000 }).catch(() => {});
await page.locator(".card.clip .sharebtn").first().click();
t = await lastToast();
check("with the tick off the original clip is shared", t.includes("Copied") && t.includes("60 fps") && !/1[0-9](\.\d)? MB/.test(t), t);
await nav("Settings");
await page.locator('button:has-text("Events & clips")').first().click();
await page.waitForTimeout(300);
const settingsBox = page.locator('label.check:has-text("Fit shared clips for Discord") input');
check("Settings shows the same setting (off)", (await settingsBox.count()) === 1 && !(await settingsBox.isChecked()));
await nav("Clips");
check("the tick stays off when coming back", !(await fitBox().isChecked()));
await fitBox().check();

// The game page's clip list has Share and the tick too.
await page.locator(".card.clip button:has-text('Game')").first().click();
await page.waitForSelector(".cliplist .sharebtn", { timeout: 8000 }).catch(() => {});
check("the game page's clips have a Share button", (await page.locator(".cliplist .sharebtn").count()) > 0);
check("the game page has the Fit for Discord tick", (await page.locator(".cliphead .fitdiscord").count()) === 1);

// The clipboard can't be opened: an error with "Show file", which opens the file's folder.
await page.goto(`${BASE}/?clipboard=busy`);
await nav("Clips");
await page.waitForSelector(".card.clip .sharebtn");
await page.locator(".card.clip .sharebtn").first().click();
t = await lastToast();
const errToast = page.locator(".toast.error", { hasText: "Couldn't copy to the clipboard" });
check("when copying fails it says so (error toast)", (await errToast.count()) === 1, t);
const showFile = errToast.getByRole("button", { name: "Show file" });
check("with a \"Show file\" button", (await showFile.count()) === 1);
await page.screenshot({ path: `${OUT}/copy-failed.png` });
await showFile.click();
await page.waitForTimeout(300);
const revealed = await page.evaluate(() => window.__cvRevealed ?? []);
check("\"Show file\" opens the shared file's folder", revealed.length === 1 && typeof revealed[0] === "string" && revealed[0].length > 0, JSON.stringify(revealed));
await page.goto(BASE);

// Every window size from the smallest up: the card's buttons (Share, keep, delete) all fit.
for (const [w, h] of [[940, 560], [1280, 720], [1600, 900], [1920, 1080], [2560, 1440]]) {
  await page.setViewportSize({ width: w, height: h });
  await nav("Clips");
  await page.waitForSelector(".card.clip .sharebtn");
  await page.waitForTimeout(200);
  const clipped = await page.$$eval(".card.clip .actions", (els) =>
    els.map((e) => { const box = e.getBoundingClientRect(); return [...e.children].filter((b) => b.getBoundingClientRect().right > box.right + 0.5).length + (e.scrollWidth > e.clientWidth ? 1 : 0); }).filter((n) => n > 0));
  check(`the clip cards' buttons fit at ${w} × ${h}`, clipped.length === 0, clipped.length ? `${clipped.length} cards cut off` : "");
  const head = await page.$eval(".page-head", (e) => e.scrollWidth - e.clientWidth);
  check(`the page header fits at ${w} × ${h}`, head <= 0, `${head} px`);
  if (w === 940 || w === 1600) await page.screenshot({ path: `${OUT}/cards-${w}.png` });
}

await browser.close();
const failed = results.filter((r) => !r.ok).length;
console.log(`${results.length - failed}/${results.length} passed`);
fs.writeFileSync(`${OUT}/results.json`, JSON.stringify(results, null, 2));
process.exit(failed ? 1 : 0);
