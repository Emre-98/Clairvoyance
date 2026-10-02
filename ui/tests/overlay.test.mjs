// End-to-end check of the replay input overlay in Chromium against the mock backend
// (`VITE_MOCK=1 npx vite`), with a test video whose frames show the "cursor" as a magenta box
// at the same positions as the synthetic input data (lib/inputoverlay.ts `synthetic`).
// The video is 4:3 with the 16:9 game letterboxed inside it (like the recorder does when the
// game's aspect differs), and the 16:9 player pillarboxes the 4:3 video: both mappings count.
//
//   node tests/overlay.test.mjs [http://localhost:5173] [outdir]
// Make the video first: python3 tests/make-sample.py (public/dev-assets/sample.webm).
import { chromium } from "playwright-core";

const BASE = process.argv[2] ?? "http://localhost:5173";
const OUT = process.argv[3] ?? "/tmp";
const exe = process.env.CHROME ?? "/opt/pw-browsers/chromium-1194/chrome-linux/chrome";
const results = [];
const check = (name, ok, detail = "") => {
  results.push({ name, ok, detail });
  console.log(`${ok ? "PASS" : "FAIL"} ${name}${detail ? " — " + detail : ""}`);
};

const browser = await chromium.launch({ executablePath: exe, args: ["--autoplay-policy=no-user-gesture-required"] });
const page = await browser.newPage({ viewport: { width: 1500, height: 950 } });
page.on("pageerror", (e) => console.log("pageerror", e.message));
await page.goto(BASE);
// The v1.4 overlay as it was: ability bubbles (v1.5, tests/bubbles.test.mjs) off, so the
// cursor-dot checks only see the dot.
await page.evaluate(() => localStorage.setItem("cv.inputOverlay", JSON.stringify({ bubbles: false })));
await page.reload();
await page.waitForSelector("button.card.game");

async function openGame(i) {
  await page.evaluate(() => document.querySelector(".sidebar a, .sidebar button") && null);
  await page.locator("button.card.game").nth(i).click();
  await page.waitForSelector(".vhost video");
  await page.waitForFunction(() => document.querySelector(".vhost video")?.readyState >= 2, null, { timeout: 15000 });
}
const toggle = () => page.locator('[data-testid="overlay-toggle"]');
const pressed = async () => (await toggle().getAttribute("aria-pressed")) === "true";
async function seek(t) {
  await page.evaluate(async (t) => {
    const v = document.querySelector(".vhost video");
    v.pause();
    await new Promise((r) => {
      v.addEventListener("seeked", r, { once: true });
      v.currentTime = t;
    });
    await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
  }, t);
  await page.waitForTimeout(120);
}
/**
 * Where the video shows its "cursor" (the magenta box) and where the overlay draws its cursor dot,
 * both in CSS pixels of the player screen. The video frame is read with drawImage (the decoded
 * frame at currentTime) and placed like CSS object-fit: contain does; the overlay is read from
 * its own canvas.
 */
async function positions() {
  return page.evaluate(() => {
    const v = document.querySelector(".vhost video");
    const scr = document.querySelector(".screen").getBoundingClientRect();
    const c = document.createElement("canvas");
    c.width = v.videoWidth;
    c.height = v.videoHeight;
    const g = c.getContext("2d");
    g.drawImage(v, 0, 0);
    const px = g.getImageData(0, 0, c.width, c.height).data;
    let sx = 0, sy = 0, n = 0;
    for (let i = 0; i < px.length; i += 4)
      if (px[i] > 180 && px[i + 2] > 180 && px[i + 1] < 90) {
        sx += (i / 4) % c.width;
        sy += Math.floor(i / 4 / c.width);
        n++;
      }
    const s = Math.min(scr.width / v.videoWidth, scr.height / v.videoHeight);
    const ox = (scr.width - v.videoWidth * s) / 2;
    const oy = (scr.height - v.videoHeight * s) / 2;
    const video = n ? { x: ox + (sx / n + 0.5) * s, y: oy + (sy / n + 0.5) * s, n } : { n: 0 };
    const oc = document.querySelector('[data-testid="input-overlay"]');
    let overlay = { n: 0 };
    if (oc) {
      const og = oc.getContext("2d");
      const d = og.getImageData(0, 0, oc.width, oc.height).data;
      const k = oc.width / scr.width;
      let ax = 0, ay = 0, m = 0;
      for (let i = 0; i < d.length; i += 4)
        if (d[i] > 235 && d[i + 1] > 235 && d[i + 2] > 235 && d[i + 3] > 200) {
          ax += (i / 4) % oc.width;
          ay += Math.floor(i / 4 / oc.width);
          m++;
        }
      if (m) overlay = { x: (ax / m + 0.5) / k, y: (ay / m + 0.5) / k, n: m };
    }
    return { video, overlay, screen: { w: scr.width, h: scr.height } };
  });
}

// 1. Open a game with an input recording: the overlay is off and nothing is loaded.
await openGame(0);
check("overlay off when a replay opens", !(await pressed()));
check("nothing loaded or drawn while off", (await page.locator('[data-testid="input-overlay"]').count()) === 0 && (await page.evaluate(() => window.__cvOverlayLoadMs)) === undefined);

// 2. Alignment: the overlay's cursor dot sits on the video's cursor (magenta box).
await seek(10);
await page.keyboard.press("i");
await page.waitForSelector('[data-testid="input-overlay"]');
await page.waitForTimeout(200);
check("I switches it on", await pressed());
const loadMs = await page.evaluate(() => window.__cvOverlayLoadMs);
console.log(`first switch-on incl. loading (mock backend, 30 ms fake delay): ${loadMs?.toFixed(1)} ms`);
// Switching on again (already loaded): time from the key press to the first drawn frame.
const reOn = [];
for (let k = 0; k < 5; k++) {
  await page.keyboard.press("i");
  await page.waitForTimeout(100);
  reOn.push(
    await page.evaluate(async () => {
      const t0 = performance.now();
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "i" }));
      while (!document.querySelector('[data-testid="input-overlay"]') || !(window.__cvOverlayDrawMs ?? []).length) await new Promise((r) => requestAnimationFrame(r));
      const c = document.querySelector('[data-testid="input-overlay"]');
      // until the canvas has pixels
      for (let i = 0; i < 10; i++) {
        const d = c.getContext("2d").getImageData(0, 0, c.width, c.height).data;
        if (d.some((x, j) => j % 4 === 3 && x > 0)) break;
        await new Promise((r) => requestAnimationFrame(r));
      }
      return performance.now() - t0;
    }),
  );
}
reOn.sort((a, b) => a - b);
check("on within 100 ms (switching on again)", reOn[reOn.length - 1] < 100, `median ${reOn[2].toFixed(1)} ms, max ${reOn[4].toFixed(1)} ms`);
const errs = [];
for (const t of [10, 37.5, 61.2, 99.9, 140]) {
  await seek(t);
  const { video: v, overlay: o } = await positions();
  const e = Math.hypot(v.x - o.x, v.y - o.y);
  errs.push(e);
  check(`dot on the video's cursor at ${t}s`, v.n > 20 && o.n > 5 && e <= 2, `video (${v.x?.toFixed(1)}, ${v.y?.toFixed(1)}) overlay (${o.x?.toFixed(1)}, ${o.y?.toFixed(1)}) = ${e.toFixed(2)} px`);
}
await page.locator(".screen").screenshot({ path: `${OUT}/overlay-paused.png` });

// 3. Toggling while playing (2x and 0.25x) doesn't stall the video; draw time.
for (const rate of [2, 0.25, 1]) {
  const r = await page.evaluate(async (rate) => {
    const v = document.querySelector(".vhost video");
    let waiting = 0;
    const onw = () => waiting++;
    v.addEventListener("waiting", onw);
    v.playbackRate = rate;
    v.currentTime = 20;
    await new Promise((r) => v.addEventListener("seeked", r, { once: true }));
    await v.play();
    const t0 = v.currentTime;
    const w0 = performance.now();
    for (let i = 0; i < 8; i++) {
      document.dispatchEvent(new KeyboardEvent("keydown", { key: "i", bubbles: true }));
      await new Promise((r) => setTimeout(r, 150));
    }
    const adv = v.currentTime - t0;
    const wall = (performance.now() - w0) / 1000;
    v.pause();
    v.removeEventListener("waiting", onw);
    return { adv, wall, waiting, paused: v.paused };
  }, rate);
  const expected = r.wall * rate;
  check(`playback keeps going while toggling at ${rate}x`, r.waiting === 0 && Math.abs(r.adv - expected) < Math.max(0.25, expected * 0.15), `advanced ${r.adv.toFixed(2)} s in ${r.wall.toFixed(2)} s, ${r.waiting} stalls`);
}
// Make sure it's on, play a bit, and read the draw times.
if (!(await pressed())) await page.keyboard.press("i");
await page.evaluate(async () => {
  const v = document.querySelector(".vhost video");
  v.playbackRate = 1;
  await v.play();
  await new Promise((r) => setTimeout(r, 2500));
  v.pause();
});
const draw = await page.evaluate(() => [...(window.__cvOverlayDrawMs ?? [])].sort((a, b) => a - b));
const p = (q) => draw[Math.min(draw.length - 1, Math.floor(q * draw.length))];
check("draw time < 2 ms per frame", draw.length > 30 && p(0.95) < 2, `median ${p(0.5)?.toFixed(3)} ms, p95 ${p(0.95)?.toFixed(3)} ms, max ${draw[draw.length - 1]?.toFixed(3)} ms over ${draw.length} frames`);

// 4. Options: keys strip, heatmap (whole game), trail length.
await page.locator('[data-testid="overlay-options"]').click();
await page.getByRole("checkbox", { name: "Keys pressed" }).check();
await page.getByRole("checkbox", { name: "Heatmap" }).check();
await seek(33.4);
await page.locator(".player").screenshot({ path: `${OUT}/overlay-options.png` });
const stored = await page.evaluate(() => JSON.parse(localStorage.getItem("cv.inputOverlay")));
check("options remembered", stored.keys === true && stored.heat === true);
await page.getByRole("checkbox", { name: "Keys pressed" }).uncheck();
await page.getByRole("checkbox", { name: "Heatmap" }).uncheck();
await page.locator(".ovpop .x").click();

// 5. Shortcut ignored while typing in a text field.
const before = await pressed();
await page.evaluate(() => {
  const i = document.createElement("input");
  i.type = "text";
  i.id = "typing-test";
  document.body.appendChild(i);
  i.focus();
});
await page.keyboard.press("i");
check("I ignored while typing", (await pressed()) === before && (await page.inputValue("#typing-test")) === "i");
await page.evaluate(() => document.getElementById("typing-test").remove());

// 6. Fullscreen: still aligned.
await page.evaluate(() => document.querySelector(".player").requestFullscreen());
await page.waitForTimeout(400);
const fs = await page.evaluate(() => !!document.fullscreenElement);
if (!(await pressed())) await page.keyboard.press("i");
await seek(77.7);
const fp = await positions();
const fe = Math.hypot(fp.video.x - fp.overlay.x, fp.video.y - fp.overlay.y);
check("fullscreen: dot on the video's cursor", fs && fp.video.n > 20 && fe <= 2, `fullscreen=${fs}, screen ${fp.screen.w}x${fp.screen.h}, error ${fe.toFixed(2)} px`);
await page.locator(".screen").screenshot({ path: `${OUT}/overlay-fullscreen.png` });
await page.evaluate(() => document.exitFullscreen());
await page.waitForTimeout(300);

// 7. Mechanics: drag a range on the APM chart -> range stats; selected-range heatmap.
const chart = page.locator('[data-testid="mechanics"] svg');
await chart.scrollIntoViewIfNeeded();
const cb = await chart.boundingBox();
await page.mouse.move(cb.x + cb.width * 0.4, cb.y + cb.height / 2);
await page.mouse.down();
await page.mouse.move(cb.x + cb.width * 0.95, cb.y + cb.height / 2, { steps: 5 });
await page.mouse.up();
await page.waitForTimeout(200);
const scope = await page.locator('[data-testid="mechanics"] .scope').innerText();
check("range selected on the APM chart", /1:00.*3:00|1:00–3:00/.test(scope) && (await page.locator('[data-testid="mechanics"] .big').first().innerText()) === "205", scope.replace(/\s+/g, " "));
await page.locator('[data-testid="mechanics"]').screenshot({ path: `${OUT}/mechanics.png` });

// 8. Another game without input: disabled + tooltip. Back: off again.
await page.locator("button.btn.ghost.icon").first().click();
await page.waitForSelector("button.card.game");
await openGame(2);
check("disabled without an input recording", await toggle().isDisabled());
check("tooltip says why", (await toggle().getAttribute("title")) === "No input recorded for this game");
await page.locator("button.btn.ghost.icon").first().click();
await page.waitForSelector("button.card.game");
await openGame(0);
check("off again when reopened", !(await pressed()) && (await page.locator('[data-testid="input-overlay"]').count()) === 0);

await browser.close();
const failed = results.filter((r) => !r.ok);
console.log(`\n${results.length - failed.length}/${results.length} passed`);
process.exit(failed.length ? 1 : 0);
