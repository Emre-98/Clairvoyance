// v1.6 fullscreen player checks in Chromium against the mock backend (`VITE_MOCK=1 npx vite`):
// the video fills the screen with the controls panel drawn over its bottom, Fit / Fill on other
// screen shapes, the input overlay pixel-aligned in every case, the drop-down panel (arrow bubble,
// bottom-centre hover arrow, H, remembered), timings, and no re-buffering.
//
//   node tests/fullscreen.test.mjs [http://localhost:5173] [outdir]
// Videos first: python3 tests/make-sample.py (public/dev-assets/sample169.webm + sample.webm).
import { chromium } from "playwright-core";
import fs from "node:fs";

const BASE = process.argv[2] ?? "http://localhost:5173";
const OUT = process.argv[3] ?? "/tmp/fs-shots";
fs.mkdirSync(OUT, { recursive: true });
const exe = process.env.CHROME ?? "/opt/pw-browsers/chromium-1194/chrome-linux/chrome";
const results = [];
const check = (name, ok, detail = "") => {
  results.push({ name, ok, detail });
  console.log(`${ok ? "PASS" : "FAIL"} ${name}${detail ? " — " + detail : ""}`);
};
const browser = await chromium.launch({ executablePath: exe, args: ["--autoplay-policy=no-user-gesture-required"] });

async function newPage(w, h, dsf = 1, video = "sample169", prefs = null) {
  const ctx = await browser.newContext({ viewport: { width: w, height: h }, deviceScaleFactor: dsf });
  const page = await ctx.newPage();
  page.on("pageerror", (e) => console.log("pageerror", e.message));
  await page.goto(`${BASE}/?video=${video}`);
  await page.evaluate((p) => {
    localStorage.setItem("cv.inputOverlay", JSON.stringify({ bubbles: false, trail: false, clicks: false }));
    if (p) localStorage.setItem("cv.player", JSON.stringify(p));
    else localStorage.removeItem("cv.player");
  }, prefs);
  await page.reload();
  await page.waitForSelector("button.card.game");
  await page.locator("button.card.game").first().click();
  await page.waitForFunction(() => document.querySelector(".vhost video")?.readyState >= 2, null, { timeout: 20000 });
  return { ctx, page };
}

/** Enter fullscreen on the player; resolves with the time to the first frame after the switch. */
const enterFs = (page) =>
  page.evaluate(async () => {
    const p = document.querySelector('[data-testid="player"]');
    const t0 = performance.now();
    const changed = new Promise((r) => document.addEventListener("fullscreenchange", r, { once: true }));
    await p.requestFullscreen();
    await changed;
    await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
    return performance.now() - t0;
  });
const exitFs = (page) =>
  page.evaluate(async () => {
    const t0 = performance.now();
    const changed = new Promise((r) => document.addEventListener("fullscreenchange", r, { once: true }));
    await document.exitFullscreen();
    await changed;
    await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
    return performance.now() - t0;
  });

async function seekPaused(page, t) {
  await page.evaluate(async (t) => {
    const v = document.querySelector(".vhost video");
    v.pause();
    await new Promise((r) => {
      v.addEventListener("seeked", r, { once: true });
      v.currentTime = t;
    });
    await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
  }, t);
  await page.waitForTimeout(150);
}

/**
 * Where the video shows its magenta "cursor": read from the decoded frame and placed with the
 * test's own object-fit math (contain / cover) in the viewport, in CSS px.
 */
const expectedBox = (page, fit) =>
  page.evaluate((fit) => {
    const v = document.querySelector(".vhost video");
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
    const W = innerWidth, H = innerHeight;
    const s = fit === "cover" ? Math.max(W / c.width, H / c.height) : Math.min(W / c.width, H / c.height);
    return { x: (W - c.width * s) / 2 + (sx / n + 0.5) * s, y: (H - c.height * s) / 2 + (sy / n + 0.5) * s, n, rect: { x: (W - c.width * s) / 2, y: (H - c.height * s) / 2, w: c.width * s, h: c.height * s } };
  }, fit);

/**
 * The exact cursor position of the synthetic input at the shown frame (what the overlay must
 * draw), and where the video's box is (drawn at whole source pixels), both in CSS px.
 */
const exactPos = (page, fit, video) =>
  page.evaluate(
    async ({ fit, video }) => {
      const v = document.querySelector(".vhost video");
      const frames = await (await fetch(`/dev-assets/${video}.frames.json`)).json().catch(() => null);
      let t = v.currentTime;
      if (frames) {
        let i = 0;
        while (i + 1 < frames.length && frames[i + 1] <= t + 1e-6) i++;
        t = frames[i];
      } else t = Math.round(Math.floor(t * 30 + 1e-6) * 1000 / 30) / 1000;
      const [cw, ch, cy, box] = video === "sample" ? [640, 360, 60, 8] : [v.videoWidth, v.videoHeight, 0, 12];
      const x = (0.5 + 0.3 * Math.cos(t)) * cw;
      const y = (0.5 + 0.3 * Math.sin(t)) * ch + cy;
      const W = innerWidth, H = innerHeight;
      const s = fit === "cover" ? Math.max(W / v.videoWidth, H / v.videoHeight) : Math.min(W / v.videoWidth, H / v.videoHeight);
      const ox = (W - v.videoWidth * s) / 2, oy = (H - v.videoHeight * s) / 2;
      return {
        t,
        scale: s,
        dot: { x: ox + x * s, y: oy + y * s },
        box: { x: ox + (Math.round(x - box / 2) + box / 2) * s, y: oy + (Math.round(y - box / 2) + box / 2) * s },
      };
    },
    { fit, video },
  );

/** Analyses a screenshot (decoded by the browser): edge bars, magenta box, overlay dot. In device px. */
async function analyse(page, png, near) {
  return page.evaluate(
    async ({ b64, near }) => {
      const img = new Image();
      img.src = "data:image/png;base64," + b64;
      await img.decode();
      const c = document.createElement("canvas");
      c.width = img.width;
      c.height = img.height;
      const g = c.getContext("2d", { willReadFrequently: true });
      g.drawImage(img, 0, 0);
      const W = c.width, H = c.height;
      const d = g.getImageData(0, 0, W, H).data;
      const at = (x, y) => (y * W + x) * 4;
      const black = (x, y) => {
        const i = at(x, y);
        return d[i] < 14 && d[i + 1] < 14 && d[i + 2] < 14;
      };
      // Bars: black columns from the left/right at 30 % height, rows from the top at 30 % width.
      const y0 = Math.floor(H * 0.3), x0 = Math.floor(W * 0.62);
      let left = 0;
      while (left < W && black(left, y0)) left++;
      let right = 0;
      while (right < W && black(W - 1 - right, y0)) right++;
      let top = 0;
      while (top < H && black(x0, top)) top++;
      // Magenta box and the white overlay dot near it (search window around `near`).
      let mx = 0, my = 0, mn = 0, wx = 0, wy = 0, wn = 0;
      const r = Math.round(60 * (W / innerWidth));
      const cx = Math.round(near.x * (W / innerWidth)), cy = Math.round(near.y * (H / innerHeight));
      for (let y = Math.max(0, cy - r); y < Math.min(H, cy + r); y++)
        for (let x = Math.max(0, cx - r); x < Math.min(W, cx + r); x++) {
          const i = at(x, y);
          if (d[i] > 180 && d[i + 2] > 180 && d[i + 1] < 90) {
            mx += x;
            my += y;
            mn++;
          }
          if (d[i] > 240 && d[i + 1] > 240 && d[i + 2] > 240) {
            wx += x;
            wy += y;
            wn++;
          }
        }
      const k = W / innerWidth;
      return {
        W, H, left, right, top,
        magenta: mn ? { x: (mx / mn + 0.5) / k, y: (my / mn + 0.5) / k, n: mn } : null,
        dot: wn ? { x: (wx / wn + 0.5) / k, y: (wy / wn + 0.5) / k, n: wn } : null,
      };
    },
    { b64: png.toString("base64"), near },
  );
}

// ---------- 1. screen sizes x Fit/Fill x panel shown/hidden ----------
const screens = [
  { w: 3840, h: 2160, dsf: 1, label: "3840x2160" },
  { w: 2560, h: 1440, dsf: 1.5, label: "3840x2160 @150%" },
  { w: 2560, h: 1440, dsf: 1, label: "2560x1440" },
  { w: 2560, h: 1080, dsf: 1, label: "2560x1080 (21:9)" },
  { w: 1920, h: 1200, dsf: 1, label: "1920x1200 (16:10)" },
];
const timings = { enter: [], exit: [], panel: [] };
for (const video of ["sample169", "sample"]) {
  for (const sc of screens) {
    if (video === "sample" && sc.label !== "2560x1440") continue; // the 4:3 video: one screen is enough
    for (const fitPref of ["fit", "fill"]) {
      const { ctx, page } = await newPage(sc.w, sc.h, sc.dsf, video, { panelDown: false, fit: fitPref, panelOpacity: 0.75 });
      // Overlay on (cursor dot only) so its alignment can be checked against the video's box.
      await page.keyboard.press("i");
      await page.waitForSelector('[data-testid="input-overlay"]');
      const ms = await enterFs(page);
      timings.enter.push(ms);
      const fit = fitPref === "fill" ? "cover" : "contain";
      const vidAspect = video === "sample" ? 4 / 3 : 16 / 9;
      const scrAspect = sc.w / sc.h;
      for (const panel of ["shown", "hidden"]) {
        if (panel === "hidden") {
          await page.keyboard.press("h");
          await page.waitForTimeout(260);
        }
        // Frames whose times are on the 4 ms input grid (k multiple of 6 at 60 fps / 3 at 30 fps).
        const t = video === "sample" ? 4.0 + 0.002 : 4.1 + 0.002;
        await seekPaused(page, t);
        const exp = await expectedBox(page, fit);
        const png = await page.screenshot({ path: `${OUT}/${video}-${sc.label.replace(/[^\w]+/g, "_")}-${fitPref}-panel-${panel}.png` });
        const a = await analyse(page, png, exp);
        // The video's box measured without the overlay (its dot covers the box's middle).
        await page.evaluate(() => (document.querySelector('[data-testid="input-overlay"]').style.visibility = "hidden"));
        await page.waitForTimeout(60);
        const bare = await analyse(page, await page.screenshot(), exp);
        await page.evaluate(() => (document.querySelector('[data-testid="input-overlay"]').style.visibility = ""));
        a.magenta = bare.magenta;
        const k = a.W / sc.w;
        const name = `${video} ${sc.label} ${fitPref} panel ${panel}`;
        // Expected bars (device px) from the shapes.
        let barX = 0, barY = 0;
        if (fit === "contain") {
          if (scrAspect > vidAspect + 1e-3) barX = Math.round(((sc.w - sc.h * vidAspect) / 2) * k);
          else if (scrAspect < vidAspect - 1e-3) barY = Math.round(((sc.h - sc.w / vidAspect) / 2) * k);
        }
        // The 4:3 test video has its own black letterbox inside (the recorder's): top bar = that.
        const innerTop = video === "sample" ? Math.round(exp.rect.h * (60 / 480) * k) : 0;
        const okBars =
          Math.abs(a.left - barX) <= 1 && Math.abs(a.right - barX) <= 1 && Math.abs(a.top - (barY + (fit === "contain" ? innerTop : Math.max(0, Math.round(exp.rect.y * k + innerTop))))) <= 2;
        check(`${name}: bars`, okBars, `left ${a.left} right ${a.right} top ${a.top} px (expected ${barX}/${barX}/${barY + (fit === "contain" ? innerTop : Math.max(0, Math.round(exp.rect.y * k + innerTop)))})`);
        const ex = await exactPos(page, fit, video);
        if (a.magenta) {
          // The box's edges are blurred by the video codec: allow a third of a source pixel.
          const dv = Math.hypot(a.magenta.x - ex.box.x, a.magenta.y - ex.box.y);
          const tol = Math.max(1.5, ex.scale * 0.34);
          check(`${name}: video placed as ${fit}`, dv < tol, `box ${a.magenta.x.toFixed(1)},${a.magenta.y.toFixed(1)} vs ${ex.box.x.toFixed(1)},${ex.box.y.toFixed(1)} (${dv.toFixed(2)} px, tolerance ${tol.toFixed(2)})`);
        } else check(`${name}: video placed as ${fit}`, false, "box not found");
        if (a.dot) {
          const dd = Math.hypot(a.dot.x - ex.dot.x, a.dot.y - ex.dot.y);
          check(`${name}: overlay aligned`, dd < 1, `dot ${a.dot.x.toFixed(2)},${a.dot.y.toFixed(2)} vs the cursor's ${ex.dot.x.toFixed(2)},${ex.dot.y.toFixed(2)} (${dd.toFixed(2)} px off)`);
        } else check(`${name}: overlay aligned`, false, "dot not found");
      }
      // Panel back up, then leave fullscreen.
      await page.keyboard.press("h");
      await page.waitForTimeout(220);
      timings.exit.push(await exitFs(page));
      await ctx.close();
    }
  }
}

// ---------- 2. panel: size, arrow bubble, hover arrow, H, remembered, cursor, video never moves ----------
{
  const { ctx, page } = await newPage(1920, 1080, 1, "sample169", null);
  const vref = await page.evaluate(() => {
    const v = document.querySelector(".vhost video");
    window.__evts = [];
    for (const n of ["waiting", "emptied", "loadstart", "abort"]) v.addEventListener(n, () => window.__evts.push(n));
    window.__vid = v;
    return v.currentTime;
  });
  await seekPaused(page, 12.3);
  await page.evaluate(() => (window.__evts = []));
  await enterFs(page);
  const st = await page.evaluate(() => {
    const p = document.querySelector('[data-testid="player-panel"]').getBoundingClientRect();
    const v = document.querySelector(".vhost video").getBoundingClientRect();
    return { panelH: p.height, panelTop: p.top, video: [v.x, v.y, v.width, v.height], same: window.__vid === document.querySelector(".vhost video") };
  });
  check("panel visible by default, compact", st.panelTop < 1080 && st.panelH / 1080 <= 0.085, `${st.panelH.toFixed(0)} px = ${((st.panelH / 1080) * 100).toFixed(1)} % of the screen height`);
  check("video fills the screen with the panel shown", st.video.join() === "0,0,1920,1080", st.video.join());
  check("same <video> element (no re-create)", st.same);
  // The arrow bubble slides the panel down; time it.
  const down = await page.evaluate(async () => {
    const panel = document.querySelector('[data-testid="player-panel"]');
    const t0 = performance.now();
    const end = new Promise((r) => panel.addEventListener("transitionend", (e) => e.propertyName === "transform" && r(), { once: false }));
    document.querySelector('[data-testid="panel-down"]').click();
    await end;
    return performance.now() - t0;
  });
  timings.panel.push(down);
  await page.waitForTimeout(80);
  const after = await page.evaluate(() => {
    const panel = document.querySelector('[data-testid="player-panel"]');
    const b = document.querySelector('[data-testid="panel-down"]');
    const v = document.querySelector(".vhost video").getBoundingClientRect();
    const up = document.querySelector('[data-testid="panel-up"]');
    return {
      vis: getComputedStyle(panel).visibility,
      top: panel.getBoundingClientRect().top,
      bubbleTop: b?.getBoundingClientRect().top ?? 9999,
      bubbleVis: b ? getComputedStyle(b).visibility : "none",
      video: [v.x, v.y, v.width, v.height].join(),
      upOpacity: up ? getComputedStyle(up).opacity : null,
    };
  });
  check("arrow bubble slides the panel down", after.vis === "hidden" && after.top >= 1080, `${down.toFixed(0)} ms, panel top ${after.top.toFixed(0)}, visibility ${after.vis}`);
  check("the arrow bubble is gone too", after.bubbleVis === "hidden" && after.bubbleTop >= 1080, `top ${after.bubbleTop.toFixed(0)}`);
  check("video didn't move or resize", after.video === "0,0,1920,1080", after.video);
  check("hover arrow hidden until hovered", after.upOpacity === "0", `opacity ${after.upOpacity}`);
  // Nothing drawn over the video: the screenshot equals one with the player's UI removed.
  await page.mouse.move(300, 300);
  const shotA = await page.screenshot({ clip: { x: 0, y: 900, width: 1920, height: 180 } });
  await page.addStyleTag({ content: '[data-testid="player-panel"], [data-testid="hotzone"] { display: none !important }' });
  await page.waitForTimeout(60);
  const shotB = await page.screenshot({ clip: { x: 0, y: 900, width: 1920, height: 180 } });
  check("nothing drawn over the video when the panel is down", shotA.equals(shotB), shotA.equals(shotB) ? "pixel-identical to the bare video" : "differs");
  await page.evaluate(() => document.querySelectorAll("style").forEach((s) => s.textContent.includes("player-panel") && s.remove()));
  console.log("  panel display after the bare-video shot:", await page.evaluate(() => getComputedStyle(document.querySelector('[data-testid="player-panel"]')).display));
  // A few more down/up cycles for the timing (H key).
  for (let k = 0; k < 4; k++) {
    const ms = await page.evaluate(async () => {
      const panel = document.querySelector('[data-testid="player-panel"]');
      const t0 = performance.now();
      const end = new Promise((r) => panel.addEventListener("transitionend", (e) => e.propertyName === "transform" && r(performance.now() - t0), { once: false }));
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "h", code: "KeyH" }));
      return Promise.race([end, new Promise((r) => setTimeout(() => r(NaN), 1500))]);
    });
    timings.panel.push(ms);
    await page.waitForTimeout(120);
  }
  // Hover the bottom centre: the up arrow fades in (~100 ms); leaving hides it after ~1 s.
  await page.mouse.move(960, 1060, { steps: 4 });
  await page.waitForTimeout(160);
  const op1 = await page.evaluate(() => getComputedStyle(document.querySelector('[data-testid="panel-up"]')).opacity);
  check("hovering the bottom centre shows the up arrow", op1 === "1", `opacity ${op1} after 160 ms`);
  await page.mouse.move(960, 600, { steps: 4 });
  await page.waitForTimeout(500);
  const op2 = await page.evaluate(() => getComputedStyle(document.querySelector('[data-testid="panel-up"]')).opacity);
  await page.waitForTimeout(750);
  const op3 = await page.evaluate(() => getComputedStyle(document.querySelector('[data-testid="panel-up"]')).opacity);
  check("leaving keeps it ~1 s, then hides it", op2 === "1" && op3 === "0", `0.5 s: ${op2}, 1.25 s: ${op3}`);
  // Outside the zone (bottom-left corner) nothing appears.
  await page.mouse.move(200, 1060, { steps: 3 });
  await page.waitForTimeout(200);
  const op4 = await page.evaluate(() => getComputedStyle(document.querySelector('[data-testid="panel-up"]')).opacity);
  check("no arrow outside the bottom-centre zone", op4 === "0", `opacity ${op4}`);
  // Cursor hides after 2 s without movement.
  await page.mouse.move(700, 500);
  await page.waitForTimeout(400);
  const c1 = await page.evaluate(() => getComputedStyle(document.querySelector(".vhost video")).cursor);
  await page.waitForTimeout(1900);
  const c2 = await page.evaluate(() => getComputedStyle(document.querySelector(".vhost video")).cursor);
  check("cursor hides after 2 s without movement (panel down)", c1 !== "none" && c2 === "none", `${c1} -> ${c2}`);
  // Click the up arrow: panel back.
  await page.mouse.move(960, 1050, { steps: 3 });
  await page.waitForTimeout(150);
  const upMs = await page.evaluate(async () => {
    const panel = document.querySelector('[data-testid="player-panel"]');
    const t0 = performance.now();
    const end = new Promise((r) => panel.addEventListener("transitionend", (e) => e.propertyName === "transform" && r(), { once: false }));
    document.querySelector('[data-testid="panel-up"]').click();
    await end;
    return performance.now() - t0;
  });
  timings.panel.push(upMs);
  const back = await page.evaluate(() => {
    const p = document.querySelector('[data-testid="player-panel"]');
    return { vis: getComputedStyle(p).visibility, bottom: p.getBoundingClientRect().bottom };
  });
  check("clicking the up arrow brings the panel back", back.vis === "visible" && Math.abs(back.bottom - 1080) < 1, `${upMs.toFixed(0)} ms`);
  const evts = await page.evaluate(() => window.__evts);
  check("no re-buffering on fullscreen / panel switches", evts.length === 0, evts.join(",") || "no waiting/emptied/loadstart");
  // H toggles, and the state is remembered.
  await page.keyboard.press("h");
  await page.waitForTimeout(220);
  const h1 = await page.evaluate(() => getComputedStyle(document.querySelector('[data-testid="player-panel"]')).visibility);
  await page.keyboard.press("h");
  await page.waitForTimeout(220);
  const h2 = await page.evaluate(() => getComputedStyle(document.querySelector('[data-testid="player-panel"]')).visibility);
  check("H toggles the panel", h1 === "hidden" && h2 === "visible", `${h1}, ${h2}`);
  // Controls still work in fullscreen: play/pause button, the overlay chip, a marker jump.
  await page.click(".cbtn.play");
  await page.waitForTimeout(300);
  const playing = await page.evaluate(() => !document.querySelector(".vhost video").paused);
  await page.click(".cbtn.play");
  await page.click('[data-testid="overlay-toggle"]');
  await page.waitForSelector('[data-testid="input-overlay"]', { timeout: 3000 }).catch(() => {});
  const ovOn = await page.locator('[data-testid="input-overlay"]').count();
  await page.locator(".timeline .mk").nth(3).click();
  await page.waitForTimeout(400);
  const jumped = await page.evaluate(() => document.querySelector(".vhost video").currentTime);
  check("buttons work in fullscreen (play, overlay, marker)", playing && ovOn === 1 && Math.abs(jumped - 12.3) > 1, `playing ${playing}, overlay ${ovOn}, jumped to ${jumped.toFixed(2)}`);
  // Remembered: panel down, leave fullscreen, reload, open, fullscreen again → still down.
  await page.keyboard.press("h");
  await page.waitForTimeout(200);
  const saved = await page.evaluate(() => [getComputedStyle(document.querySelector('[data-testid="player-panel"]')).visibility, localStorage.getItem("cv.player")]);
  console.log("  before reload:", saved.join(" "));
  await exitFs(page);
  await page.reload();
  await page.waitForSelector("button.card.game");
  await page.locator("button.card.game").first().click();
  await page.waitForFunction(() => document.querySelector(".vhost video")?.readyState >= 2, null, { timeout: 20000 });
  await enterFs(page);
  await page.waitForTimeout(50);
  const rem = await page.evaluate(() => getComputedStyle(document.querySelector('[data-testid="player-panel"]')).visibility);
  check("panel state remembered between replays", rem === "hidden", rem);
  // Double-click on the video leaves fullscreen.
  await page.mouse.dblclick(800, 400);
  await page.waitForTimeout(300);
  const fsAfter = await page.evaluate(() => !!document.fullscreenElement);
  check("double-click toggles fullscreen", !fsAfter);
  // Windowed layout unchanged: the panel is under the video, not over it.
  const win = await page.evaluate(() => {
    const s = document.querySelector(".screen").getBoundingClientRect();
    const p = document.querySelector('[data-testid="player-panel"]').getBoundingClientRect();
    return { below: p.top >= s.bottom - 0.5, aspect: s.width / s.height };
  });
  check("windowed: controls below the 16:9 video", win.below && Math.abs(win.aspect - 16 / 9) < 0.01, `aspect ${win.aspect.toFixed(3)}`);
  await ctx.close();
}

const med = (a) => [...a].sort((x, y) => x - y)[Math.floor(a.length / 2)] ?? 0;
const mx = (a) => Math.max(...a);
// Headless Chromium paints a 4K screen in software: the median is the meaningful number here; the
// real WebView2 numbers come from the benchmark on the owner's PC.
check("entering fullscreen < 150 ms (median)", med(timings.enter) < 150, `median ${med(timings.enter).toFixed(0)} ms, max ${mx(timings.enter).toFixed(0)} ms (${timings.enter.length}x)`);
check("leaving fullscreen < 150 ms", mx(timings.exit) < 150, `median ${med(timings.exit).toFixed(0)} ms, max ${mx(timings.exit).toFixed(0)} ms`);
check("panel hide/show ≤ 150 ms (median, + one frame)", med(timings.panel) <= 175, `median ${med(timings.panel).toFixed(0)} ms: ` + timings.panel.map((x) => x.toFixed(0)).join(", ") + " ms (CSS transition 150 ms)");

await browser.close();
const failed = results.filter((r) => !r.ok);
fs.writeFileSync(`${OUT}/results.json`, JSON.stringify({ results, timings }, null, 1));
console.log(`\n${results.length - failed.length}/${results.length} passed`);
process.exit(failed.length ? 1 : 0);
