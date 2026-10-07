// Clip export with the input overlay burned in (2026-10-07), in Chromium against the mock backend
// (`VITE_MOCK=1 npx vite`): the clip editor draws one overlay PNG per output frame at the moments
// the app asks for and streams them out. The mock keeps the PNGs; this test decodes them and checks
// the cursor dot lands on the test video's own cursor (the magenta box) at the same moment.
// (The ffmpeg side, which lays the PNGs over the clip, is tested in
// crates/cv-capture/tests/overlay_export.rs.)
//
//   node tests/export.test.mjs [http://localhost:5173] [outdir]
import { chromium } from "playwright-core";
import fs from "node:fs";

const BASE = process.argv[2] ?? "http://localhost:5173";
const OUT = process.argv[3] ?? "/tmp/export";
fs.mkdirSync(OUT, { recursive: true });
const exe = process.env.CHROME ?? "/opt/pw-browsers/chromium-1194/chrome-linux/chrome";
const results = [];
const check = (name, ok, detail = "") => {
  results.push({ name, ok, detail });
  console.log(`${ok ? "PASS" : "FAIL"} ${name}${detail ? " — " + detail : ""}`);
};
const browser = await chromium.launch({ executablePath: exe, args: ["--autoplay-policy=no-user-gesture-required"] });
const page = await browser.newPage({ viewport: { width: 1600, height: 1000 } });
page.on("pageerror", (e) => console.log("pageerror", e.message));
await page.goto(`${BASE}/?video=sample169&ffmpeg=1`);
// Only the cursor dot (white): what this test measures. Trail / clicks / bubbles have their own
// pixel tests in the player and use the same drawing code.
await page.evaluate(() => localStorage.setItem("cv.inputOverlay", JSON.stringify({ v: 2, trail: false, clicks: false, dot: true, bubbles: false, keys: false, heat: false })));
await page.reload();
await page.waitForSelector("button.card.game");
await page.locator("button.card.game").first().click();
await page.waitForFunction(() => document.querySelector(".vhost video")?.readyState >= 2, null, { timeout: 20000 });
await page.evaluate(async () => {
  const v = document.querySelector(".vhost video");
  v.pause();
  await new Promise((r) => {
    v.addEventListener("seeked", r, { once: true });
    v.currentTime = 40;
  });
});
await page.getByRole("button", { name: "Create clip" }).click();
await page.waitForSelector('[data-testid="export-overlay"]');
check("the clip editor offers the input overlay for a game with an input recording", true);
await page.locator('[data-testid="export-overlay"]').check();
const exact = page.locator(".editor label", { hasText: "Exact cut" }).locator("input");
check("with the overlay the clip is re-encoded (Exact cut on, locked)", (await exact.isChecked()) && (await exact.isDisabled()));

const t0 = Date.now();
await page.getByRole("button", { name: "Save clip" }).click();
await page.waitForFunction(() => window.__cvExports?.[0]?.done, null, { timeout: 120000 });
const ms = Date.now() - t0;
const ex = await page.evaluate(() => {
  const e = window.__cvExports[0];
  return { n: e.frames.length, times: e.times, start: e.start, end: e.end, bytes: e.frames.reduce((a, f) => a + f.length, 0) };
});
check("one overlay frame per output frame", ex.n === ex.times.length && ex.n > 0, `${ex.n} PNGs for ${ex.times.length} frames (${ex.start.toFixed(2)}-${ex.end.toFixed(2)} s)`);
check(`rendered at ${((ex.n / ms) * 1000).toFixed(0)} frames/s`, true, `${ex.n} frames in ${ms} ms, ${(ex.bytes / ex.n / 1024).toFixed(1)} KB per PNG (headless, software rendering)`);
check("the editor closed with \"Clip saved\"", (await page.locator(".editor").count()) === 0);

// The dot in exported frame k vs the video's cursor at times[k] (both in video pixels).
const picks = [0, Math.floor(ex.n / 3), Math.floor((2 * ex.n) / 3), ex.n - 1];
for (const k of picks) {
  const r = await page.evaluate(async (k) => {
    const e = window.__cvExports[0];
    const img = await createImageBitmap(new Blob([e.frames[k]], { type: "image/png" }));
    const c = document.createElement("canvas");
    c.width = img.width;
    c.height = img.height;
    const g = c.getContext("2d", { willReadFrequently: true });
    g.drawImage(img, 0, 0);
    let d = g.getImageData(0, 0, c.width, c.height).data;
    let ax = 0, ay = 0, m = 0, opaque = 0;
    for (let i = 0; i < d.length; i += 4) {
      if (d[i + 3] > 0) opaque++;
      if (d[i] > 235 && d[i + 1] > 235 && d[i + 2] > 235 && d[i + 3] > 200) {
        ax += (i / 4) % c.width;
        ay += Math.floor(i / 4 / c.width);
        m++;
      }
    }
    const v = document.querySelector("video");
    await new Promise((res) => {
      v.addEventListener("seeked", res, { once: true });
      v.currentTime = e.times[k];
    });
    await new Promise((res) => requestAnimationFrame(() => requestAnimationFrame(res)));
    const vc = document.createElement("canvas");
    vc.width = v.videoWidth;
    vc.height = v.videoHeight;
    const vg = vc.getContext("2d", { willReadFrequently: true });
    vg.drawImage(v, 0, 0);
    d = vg.getImageData(0, 0, vc.width, vc.height).data;
    let sx = 0, sy = 0, n = 0;
    for (let i = 0; i < d.length; i += 4)
      if (d[i] > 180 && d[i + 2] > 180 && d[i + 1] < 90) {
        sx += (i / 4) % vc.width;
        sy += Math.floor(i / 4 / vc.width);
        n++;
      }
    return { size: [img.width, img.height], video: [vc.width, vc.height], dot: m ? [ax / m, ay / m] : null, cursor: n ? [sx / n, sy / n] : null, opaque, total: c.width * c.height };
  }, k);
  const dist = r.dot && r.cursor ? Math.hypot(r.dot[0] - r.cursor[0], r.dot[1] - r.cursor[1]) : Infinity;
  check(
    `frame ${k}: the burned-in cursor dot is on the video's cursor`,
    r.size[0] === r.video[0] && r.size[1] === r.video[1] && dist < 1.5 && r.opaque < r.total * 0.01,
    `PNG ${r.size.join("x")} (video ${r.video.join("x")}), dot ${r.dot?.map((v) => v.toFixed(1))}, cursor ${r.cursor?.map((v) => v.toFixed(1))}, ${dist.toFixed(2)} px apart; ${((r.opaque / r.total) * 100).toFixed(2)} % of the frame drawn (transparent elsewhere)`,
  );
}

// Cancelling stops the export: no clip, no error, the editor stays open.
await page.getByRole("button", { name: "Create clip" }).click();
await page.locator('[data-testid="export-overlay"]').check();
await page.getByRole("button", { name: "Save clip" }).click();
await page.waitForSelector('[data-testid="export-cancel"]');
await page.waitForTimeout(150);
await page.locator('[data-testid="export-cancel"]').click();
await page.waitForFunction(() => window.__cvExports?.[1]?.cancelled, null, { timeout: 10000 });
await page.waitForTimeout(300);
const after = await page.evaluate(() => ({ e: window.__cvExports[1], toasts: [...document.querySelectorAll(".toast")].map((t) => t.textContent) }));
check("Cancel stops the export (nothing saved, no error shown)", after.e.cancelled && !after.e.done && !after.toasts.some((t) => /error|cancel/i.test(t)), `${after.e.frames.length}/${after.e.times.length} frames sent; toasts: ${JSON.stringify(after.toasts)}`);
check("the editor stays open after cancelling, ready again", (await page.getByRole("button", { name: "Save clip" }).count()) === 1);
await page.screenshot({ path: `${OUT}/editor.png` });

await browser.close();
const failed = results.filter((r) => !r.ok).length;
console.log(`${results.length - failed}/${results.length} passed`);
fs.writeFileSync(`${OUT}/results.json`, JSON.stringify(results, null, 2));
process.exit(failed ? 1 : 0);
