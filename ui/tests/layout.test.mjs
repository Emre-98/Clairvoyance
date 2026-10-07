// Layout audit (v1.7): every page at many window sizes and Windows scaling factors, in Chromium
// against the mock backend (`VITE_MOCK=1 npx vite`). For every page state and every scroll
// position of the page it checks, automatically:
//   - no interactive element (button, input, select, link, chip...) overlaps another one;
//   - no text overlaps other text or a control it doesn't belong to;
//   - every control is really on top where it's drawn (a hit test at its centre reaches it);
//   - nothing is cut off: no control or text clipped by a box that can't scroll, no text cut
//     without an ellipsis (an ellipsis needs the full text in a tooltip), no popover outside the
//     window, and no horizontal scrolling of the page.
// Screenshots of a set of sizes go to <outdir>/layout/.
//
//   node tests/layout.test.mjs [http://localhost:5173] [outdir] [--quick] [--report=<file.json>]
// --quick: fewer sizes (smoke run). Exit code 1 when anything failed.
import { chromium } from "playwright-core";
import fs from "node:fs";
import path from "node:path";

const args = process.argv.slice(2).filter((a) => !a.startsWith("--"));
const flags = process.argv.slice(2).filter((a) => a.startsWith("--"));
const BASE = args[0] ?? "http://localhost:5173";
const OUT = path.join(args[1] ?? "/tmp", "layout");
const QUICK = flags.includes("--quick");
const REPORT = flags.find((f) => f.startsWith("--report="))?.slice(9);
// --only=<state>[,<state>] and --size=<dpr>x<w>x<h>: a subset (debugging).
const ONLY = flags.find((f) => f.startsWith("--only="))?.slice(7).split(",");
const ONLY_SIZE = flags.find((f) => f.startsWith("--size="))?.slice(7);
const exe = process.env.CHROME ?? "/opt/pw-browsers/chromium-1194/chrome-linux/chrome";
fs.mkdirSync(OUT, { recursive: true });

// Window sizes in CSS px (= Windows "logical" px: the physical size divided by the scaling).
// The app's minimum window is 940 x 560 (app/src/main.rs).
const SIZES = QUICK
  ? [
      [1, 940, 560],
      [1, 1920, 1080],
      [1.5, 1280, 688],
    ]
  : [
      // 100 %
      [1, 940, 560], // the minimum window
      [1, 960, 600],
      [1, 1024, 640],
      [1, 1100, 700],
      [1, 1280, 720],
      [1, 1366, 768],
      [1, 1600, 900],
      [1, 1920, 1080],
      [1, 2560, 1440],
      [1, 3840, 2160],
      [1, 940, 1100], // narrow and tall
      [1, 2560, 600], // wide and short
      [1, 1180, 600],
      // 125 %
      [1.25, 940, 560],
      [1.25, 1093, 576], // 1366 x 768 screen, maximized with the taskbar
      [1.25, 1229, 662], // 1536 x 864 screen, maximized with the taskbar
      [1.25, 1536, 824], // 1920 x 1080 screen, maximized
      [1.25, 2048, 1112], // 2560 x 1440
      [1.25, 3072, 1688], // 4K
      // 150 %
      [1.5, 940, 560],
      [1.5, 1280, 688], // 1920 x 1080 screen, maximized
      [1.5, 1707, 928], // 2560 x 1440
      [1.5, 2560, 1408], // 4K
      [1.5, 1100, 640],
    ];
// Screenshots for the report (all page states) at these sizes.
const SHOT = new Set(["1x940x560", "1x1280x720", "1x1920x1080", "1x3840x2160", "1.5x940x560", "1.25x1536x824", "1x2560x600", "1x940x1100"]);

const results = [];
const failures = [];
const check = (name, ok, detail = "") => {
  results.push({ name, ok, detail });
  if (!ok) failures.push({ name, detail });
  console.log(`${ok ? "PASS" : "FAIL"} ${name}${detail && !ok ? " — " + detail : ""}`);
};

// ---------- page states ----------
async function nav(page, label) {
  await page.locator(".sidebar button.nav", { hasText: label }).click();
  await page.waitForTimeout(260);
}
async function openGame(page) {
  await nav(page, "Games");
  await page.waitForSelector("button.card.game");
  await page.locator("button.card.game").first().click();
  await page.waitForSelector(".vhost video");
  await page.waitForFunction(() => document.querySelector(".vhost video")?.readyState >= 2, null, { timeout: 15000 });
  await page.waitForTimeout(350);
}
const settingsSection = (label) => async (page) => {
  await nav(page, "Settings");
  await page.locator(".subnav button", { hasText: label }).first().click();
  await page.waitForTimeout(250);
};
const STATES = {
  home: (page) => nav(page, "Home"),
  games: async (page) => {
    await nav(page, "Games");
    await page.waitForSelector("button.card.game");
  },
  clips: (page) => nav(page, "Clips"),
  "settings-recorder": settingsSection("Recorder"),
  "settings-recording": settingsSection("Recording"),
  "settings-modes": settingsSection("Game modes"),
  "settings-hotkeys": settingsSection("Hotkeys"),
  "settings-events": settingsSection("Events & clips"),
  "settings-games": settingsSection("Games"),
  "settings-storage": settingsSection("Storage"),
  "settings-appearance": settingsSection("Appearance"),
  "settings-general": settingsSection("General & updates"),
  "settings-performance": settingsSection("Performance test"),
  "settings-advanced": settingsSection("Advanced"),
  game: openGame,
  "game-overlay-options": async (page) => {
    await openGame(page);
    await page.locator('[data-testid="overlay-toggle"]').click();
    await page.waitForSelector('[data-testid="input-overlay"]');
    await openMenu(page, '[data-testid="overlay-options"]');
  },
  // The clip editor under the player: its widest row (name, Exact cut, Input overlay, the long
  // "Get ffmpeg to export" button the mock shows).
  "game-clip-editor": async (page) => {
    await openGame(page);
    await page.locator("button", { hasText: "Create clip" }).first().click();
    await page.waitForSelector('[data-testid="export-overlay"]');
    await page.locator('[data-testid="export-overlay"]').check();
    await page.waitForTimeout(250);
  },
  "game-player-settings": async (page) => {
    await openGame(page);
    await openMenu(page, '[data-testid="player-settings"]');
  },
  "game-zoomed": async (page) => {
    await openGame(page);
    for (let i = 0; i < 3; i++) await clickControl(page, '[data-testid="zoom-in"]');
    await page.waitForTimeout(300);
  },
  "game-paused-frame": async (page) => {
    await openGame(page);
    // Paused and zoomed: the time shows milliseconds + the frame number (the longest label).
    await clickControl(page, '[data-testid="zoom-in"]');
    await page.keyboard.press(".");
    await page.waitForTimeout(300);
  },
  "game-fullscreen": async (page) => {
    await openGame(page);
    await page.evaluate(async () => {
      const changed = new Promise((r) => document.addEventListener("fullscreenchange", r, { once: true }));
      await document.querySelector(".player").requestFullscreen();
      await changed;
    });
    await page.waitForTimeout(400);
  },
  "game-fullscreen-overlay-options": async (page) => {
    await STATES["game-fullscreen"](page);
    await page.locator('[data-testid="overlay-toggle"]').click();
    await openMenu(page, '[data-testid="overlay-options"]');
  },
};

/** Controls that may sit in a "more" menu at small sizes: open it first when the control is in there. */
async function clickControl(page, sel) {
  const vis = await page.locator(sel).first().isVisible().catch(() => false);
  if (!vis) {
    const more = page.locator('[data-testid="player-more"]');
    if (await more.isVisible().catch(() => false)) {
      await more.click();
      await page.waitForTimeout(120);
    }
  }
  // Collapsed controls whose content is inline in the menu (player settings): the menu is it.
  if (!(await page.locator(sel).count()) && (await page.locator('[data-testid="player-more-menu"]').count())) return;
  await page.locator(sel).first().click();
}
const openMenu = async (page, sel) => {
  await clickControl(page, sel);
  await page.waitForTimeout(250);
};

// ---------- the in-page audit ----------
// Runs in the page: returns every problem found at the current scroll position.
function audit(first = true) {
  const vw = window.innerWidth, vh = window.innerHeight;
  const problems = [];
  const FLOAT = ".ovpop, .tip, .toasts, .toast, [role=dialog], .modal, .menu-pop, .more-pop, .setup";
  const cs = (el) => getComputedStyle(el);
  const name = (el) => {
    const t = (el.getAttribute("aria-label") || el.getAttribute("title") || el.textContent || el.getAttribute("data-testid") || "").trim().replace(/\s+/g, " ").slice(0, 40);
    const cls = typeof el.className === "string" && el.className ? "." + el.className.trim().split(/\s+/).slice(0, 2).join(".") : "";
    return `${el.tagName.toLowerCase()}${cls}${t ? ` "${t}"` : ""}`;
  };
  const shown = (el) => {
    for (let e = el; e && e !== document.documentElement; e = e.parentElement) {
      const s = cs(e);
      if (s.display === "none" || s.visibility === "hidden" || s.visibility === "collapse" || Number(s.opacity) === 0 || e.hasAttribute("inert")) return false;
    }
    return true;
  };
  // The part of an element that can be seen: clipped by the window and by every ancestor that
  // clips its overflow. `cut` = clipped by a box that can't be scrolled (= really cut off).
  const scrollableX = (s, e) => (s.overflowX === "auto" || s.overflowX === "scroll") && e.scrollWidth > e.clientWidth + 1;
  const scrollableY = (s, e) => (s.overflowY === "auto" || s.overflowY === "scroll") && e.scrollHeight > e.clientHeight + 1;
  function visibleRect(el, r = el.getBoundingClientRect(), self = false) {
    let x0 = r.left, y0 = r.top, x1 = r.right, y1 = r.bottom;
    let cut = null;
    const fixedRoot = (() => {
      for (let e = el; e; e = e.parentElement) if (cs(e).position === "fixed") return e;
      return null;
    })();
    for (let e = self ? el : el.parentElement; e; e = e.parentElement) {
      const s = cs(e);
      const clipX = s.overflowX !== "visible", clipY = s.overflowY !== "visible";
      if (!clipX && !clipY) continue;
      const b = e.getBoundingClientRect();
      const bx0 = b.left + e.clientLeft, by0 = b.top + e.clientTop;
      const bx1 = bx0 + (e === document.documentElement ? vw : e.clientWidth), by1 = by0 + (e === document.documentElement ? vh : e.clientHeight);
      if (clipX) {
        if (!cut && !scrollableX(s, e) && (x0 < bx0 - 1 || x1 > bx1 + 1) && x1 > bx0 && x0 < bx1) cut = `clipped horizontally by ${name(e)}`;
        x0 = Math.max(x0, bx0);
        x1 = Math.min(x1, bx1);
      }
      if (clipY) {
        if (!cut && !scrollableY(s, e) && (y0 < by0 - 1 || y1 > by1 + 1) && y1 > by0 && y0 < by1) cut = `clipped vertically by ${name(e)}`;
        y0 = Math.max(y0, by0);
        y1 = Math.min(y1, by1);
      }
      if (fixedRoot && e === fixedRoot) break;
    }
    // The window (the document itself never scrolls in this app).
    if (!cut && (x0 < -1 || x1 > vw + 1 || y0 < -1 || y1 > vh + 1) && x1 > 0 && x0 < vw && y1 > 0 && y0 < vh) cut = "outside the window";
    x0 = Math.max(x0, 0);
    y0 = Math.max(y0, 0);
    x1 = Math.min(x1, vw);
    y1 = Math.min(y1, vh);
    return { x0, y0, x1, y1, w: Math.max(0, x1 - x0), h: Math.max(0, y1 - y0), cut };
  }
  const inter = (a, b) => {
    const w = Math.min(a.x1, b.x1) - Math.max(a.x0, b.x0);
    const h = Math.min(a.y1, b.y1) - Math.max(a.y0, b.y0);
    return w > 1 && h > 1 ? { w, h } : null;
  };
  const layer = (el) => el.closest(FLOAT);
  // In fullscreen only the fullscreen element is on screen (the rest of the page is under it).
  const root = document.fullscreenElement ?? document.body;

  // 0. No horizontal page scroll.
  const de = document.documentElement;
  if (de.scrollWidth > vw + 1 || document.body.scrollWidth > vw + 1) problems.push({ kind: "page-hscroll", what: `page ${de.scrollWidth}px wide in a ${vw}px window` });

  // 1. Controls.
  const SEL = "button, a[href], input:not([type=hidden]), select, textarea, [role=button], [role=radio], [role=slider], [role=tab], summary";
  const ctrls = [];
  for (const el of root.querySelectorAll(SEL)) {
    const r = el.getBoundingClientRect();
    if (r.width < 1 || r.height < 1 || !shown(el)) continue;
    if (cs(el).pointerEvents === "none") continue;
    const v = visibleRect(el, r);
    // Scrolled out of view inside a scrolling box: not on screen now (checked at another scroll).
    if (v.w < 1 || v.h < 1) continue;
    const mk = !!el.closest(".timeline .mk, .mks button, .markers button");
    ctrls.push({ el, r, v, mk, layer: layer(el) });
    // Cut off: partly clipped by a box that can't scroll. Timeline marker hit targets at the
    // ends of a zoomed view are cut by design (the view is a window over the timeline).
    if (v.cut && !mk && (v.w * v.h < r.width * r.height * 0.97)) problems.push({ kind: "control-cut", what: `${name(el)} ${v.cut} (${Math.round(v.w)}x${Math.round(v.h)} of ${Math.round(r.width)}x${Math.round(r.height)})` });
  }
  // Overlaps between controls (not nested, not both timeline markers, same layer).
  for (let i = 0; i < ctrls.length; i++)
    for (let j = i + 1; j < ctrls.length; j++) {
      const a = ctrls[i], b = ctrls[j];
      if (a.mk && b.mk) continue;
      if (a.layer !== b.layer) continue;
      if (a.el.contains(b.el) || b.el.contains(a.el)) continue;
      // A label and the input it wraps / the input inside it.
      if ((a.el.tagName === "LABEL" && a.el.contains(b.el)) || (b.el.tagName === "LABEL" && b.el.contains(a.el))) continue;
      const o = inter(a.v, b.v);
      if (o) problems.push({ kind: "control-overlap", what: `${name(a.el)} overlaps ${name(b.el)} by ${Math.round(o.w)}x${Math.round(o.h)} px` });
    }
  // Hit test: the control is what's on top at its centre (and a few points inside).
  for (const c of ctrls) {
    // A sliver at the edge of a scrolling box (fractional px at 125 / 150 %) can't be hit-tested.
    if (c.mk || c.v.w < 4 || c.v.h < 4) continue;
    // The centre, and near both ends when the control is entirely visible (a partly visible
    // round button's visible strip may not reach its ends).
    const whole = c.v.w >= c.r.width - 1 && c.v.h >= c.r.height - 1;
    const cy = (c.v.y0 + c.v.y1) / 2;
    const pts = [[(c.v.x0 + c.v.x1) / 2, cy], ...(whole ? [[c.v.x0 + Math.min(4, c.v.w / 3), cy], [c.v.x1 - Math.min(4, c.v.w / 3), cy]] : [])];
    for (const [x, y] of pts) {
      const hit = document.elementFromPoint(x, y);
      if (!hit || hit === c.el || c.el.contains(hit) || hit.contains(c.el)) continue;
      // A label around the control, or the control's own label.
      if (hit.closest("label")?.contains(c.el)) continue;
      // Covered by an open popover / tooltip / toast over the page: by design.
      const hl = layer(hit);
      if (hl && hl !== c.layer) continue;
      // Video-area overlays that pass clicks through don't count; anything else on top does.
      problems.push({ kind: "control-covered", what: `${name(c.el)} is under ${name(hit)} at (${Math.round(x)}, ${Math.round(y)})` });
      break;
    }
  }

  // 2. Text: every visible text node, line by line.
  const lines = [];
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT, {
    acceptNode: (n) => (n.nodeValue.trim() ? NodeFilter.FILTER_ACCEPT : NodeFilter.FILTER_REJECT),
  });
  const range = document.createRange();
  for (let n = walker.nextNode(); n; n = walker.nextNode()) {
    const el = n.parentElement;
    if (!el || ["SCRIPT", "STYLE", "OPTION", "TEXTAREA", "TITLE", "SVG"].includes(el.tagName) || el.closest("svg, select, .sr-only, .visually-hidden")) continue;
    if (!shown(el)) continue;
    range.selectNodeContents(n);
    const rects = [...range.getClientRects()].filter((r) => r.width > 0.5 && r.height > 0.5);
    if (!rects.length) continue;
    const s = cs(el);
    // Text cut by its own box: hidden overflow without an ellipsis, or an ellipsis without the
    // full text in a tooltip (title) on it or an ancestor.
    let box = el;
    while (box && cs(box).display === "inline") box = box.parentElement;
    if (box) {
      const bs = cs(box);
      if ((bs.overflowX === "hidden" || bs.overflowX === "clip") && box.scrollWidth > box.clientWidth + 1 && !scrollableX(bs, box)) {
        const ellipsis = bs.textOverflow === "ellipsis";
        const tip = !!box.closest("[title]") || !!box.closest("[data-full]");
        if (!ellipsis) problems.push({ kind: "text-cut", what: `"${n.nodeValue.trim().slice(0, 40)}" cut off in ${name(box)} (${box.scrollWidth} > ${box.clientWidth} px)` });
        else if (!tip) problems.push({ kind: "text-ellipsis-no-tip", what: `"${n.nodeValue.trim().slice(0, 40)}" shortened with "…" in ${name(box)} without a tooltip` });
      }
    }
    for (const r of rects) {
      const v = visibleRect(el, r, true);
      if (v.w < 1 || v.h < 1) continue;
      // Clipped by a box that can't scroll (vertical cuts and horizontal ones without ellipsis).
      if (v.cut && v.w * v.h < r.width * r.height * 0.9) {
        const anc = el.closest("*");
        const ell = box && cs(box).textOverflow === "ellipsis";
        const isMk = !!el.closest(".timeline");
        if (!(ell && v.cut.startsWith("clipped horizontally")) && !isMk) problems.push({ kind: "text-cut", what: `"${n.nodeValue.trim().slice(0, 40)}" ${v.cut} in ${name(anc)}` });
      }
      lines.push({ n, el, v, layer: layer(el) });
    }
  }
  // Text over text (different nodes, same layer), and text under a control it isn't part of.
  for (let i = 0; i < lines.length; i++) {
    for (let j = i + 1; j < lines.length; j++) {
      const a = lines[i], b = lines[j];
      if (a.n === b.n || a.layer !== b.layer) continue;
      const o = inter(a.v, b.v);
      if (o && o.w > 2 && o.h > 3) problems.push({ kind: "text-overlap", what: `"${a.n.nodeValue.trim().slice(0, 30)}" overlaps "${b.n.nodeValue.trim().slice(0, 30)}" by ${Math.round(o.w)}x${Math.round(o.h)} px` });
    }
    const a = lines[i];
    for (const c of ctrls) {
      if (c.mk || c.layer !== a.layer) continue;
      if (c.el.contains(a.el) || a.el.contains(c.el)) continue;
      const lab = a.el.closest("label");
      if (lab && (lab.contains(c.el) || (c.el.id && lab.htmlFor === c.el.id))) continue;
      const o = inter(a.v, c.v);
      if (o && o.w > 2 && o.h > 3) problems.push({ kind: "text-under-control", what: `"${a.n.nodeValue.trim().slice(0, 30)}" overlaps ${name(c.el)} by ${Math.round(o.w)}x${Math.round(o.h)} px` });
    }
  }

  // 3. Popovers and tooltips: entirely inside the window, nothing of them clipped.
  for (const el of first ? root.querySelectorAll(".ovpop") : []) {
    if (!shown(el)) continue;
    const r = el.getBoundingClientRect();
    const v = visibleRect(el, r);
    if (v.cut || v.w * v.h < r.width * r.height * 0.99) problems.push({ kind: "popover-cut", what: `${name(el)} ${v.cut ?? "partly hidden"} (${Math.round(r.left)},${Math.round(r.top)} ${Math.round(r.width)}x${Math.round(r.height)})` });
  }
  return problems;
}

// Scrolls every scrollable page box through its height, auditing at each position.
async function auditAll(page) {
  const all = await page.evaluate(audit);
  const boxes = await page.evaluate(() => {
    const out = [];
    let i = 0;
    for (const el of document.querySelectorAll("main *")) {
      const s = getComputedStyle(el);
      if ((s.overflowY === "auto" || s.overflowY === "scroll") && el.scrollHeight > el.clientHeight + 4 && el.clientHeight > 150) {
        el.setAttribute("data-audit-scroll", String(i));
        out.push({ i: i++, h: el.scrollHeight, ch: el.clientHeight });
      }
    }
    return out;
  });
  for (const b of boxes) {
    const steps = Math.min(12, Math.ceil((b.h - b.ch) / (b.ch * 0.7)));
    for (let k = 1; k <= steps; k++) {
      await page.evaluate(([i, y]) => {
        const el = document.querySelector(`[data-audit-scroll="${i}"]`);
        if (el) el.scrollTop = y;
      }, [b.i, Math.round(((b.h - b.ch) * k) / steps)]);
      await page.waitForTimeout(60);
      all.push(...(await page.evaluate(audit, false)));
    }
    await page.evaluate((i) => {
      const el = document.querySelector(`[data-audit-scroll="${i}"]`);
      if (el) el.scrollTop = 0;
    }, b.i);
  }
  // Same problem seen at several scroll positions: once.
  const seen = new Set();
  return all.filter((p) => {
    const k = p.kind + p.what;
    if (seen.has(k)) return false;
    seen.add(k);
    return true;
  });
}

const browser = await chromium.launch({ executablePath: exe, args: ["--autoplay-policy=no-user-gesture-required"] });
const summary = {};
const t0 = Date.now();
for (const [dpr, w, h] of SIZES.filter(([d, w, h]) => !ONLY_SIZE || ONLY_SIZE === `${d}x${w}x${h}`)) {
  const ctx = await browser.newContext({ viewport: { width: w, height: h }, deviceScaleFactor: dpr });
  const page = await ctx.newPage();
  if (process.env.FITDBG) page.on("pageerror", async () => { const l = await page.evaluate(() => (window.__cvFitLog ?? []).slice(-6)).catch(() => []); for (const x of l) console.log("FIT", JSON.stringify({ ...x, n: undefined })); });
  page.on("pageerror", (e) => console.log("pageerror", e.message.split("\n")[0], (e.stack ?? "").split("\n").slice(1, 6).join(" | ")));
  const size = `${dpr}x${w}x${h}`;
  for (const [state, enter] of Object.entries(STATES).filter(([k]) => !ONLY || ONLY.includes(k))) {
    await page.goto(BASE);
    await page.evaluate(() => {
      localStorage.clear();
      // A long, realistic overlay setup: the longest labels.
      localStorage.setItem("cv.inputOverlay", JSON.stringify({ v: 2, keys: true }));
    });
    await page.reload();
    await page.waitForSelector(".sidebar");
    let problems;
    try {
      await enter(page);
      await page.waitForTimeout(150);
      problems = await auditAll(page);
    } catch (e) {
      problems = [{ kind: "error", what: String(e).split("\n")[0] }];
    }
    if (SHOT.has(size)) await page.screenshot({ path: path.join(OUT, `${state}-${size}.png` ) });
    else if (problems.length) await page.screenshot({ path: path.join(OUT, `FAIL-${state}-${size}.png`) });
    const key = `${state} @ ${w}x${h} ${Math.round(dpr * 100)}%`;
    summary[key] = problems;
    check(key, problems.length === 0, problems.slice(0, 6).map((p) => `[${p.kind}] ${p.what}`).join("; ") + (problems.length > 6 ? `; +${problems.length - 6} more` : ""));
    if (await page.evaluate(() => !!document.fullscreenElement)) await page.evaluate(() => document.exitFullscreen());
  }
  await ctx.close();
}
await browser.close();
if (REPORT) fs.writeFileSync(REPORT, JSON.stringify(summary, null, 1));
const kinds = {};
for (const ps of Object.values(summary)) for (const p of ps) kinds[p.kind] = (kinds[p.kind] ?? 0) + 1;
console.log(`\nproblems by kind: ${JSON.stringify(kinds)}`);
console.log(`${results.length - failures.length}/${results.length} passed (${Object.keys(STATES).length} page states x ${SIZES.length} sizes, ${((Date.now() - t0) / 1000).toFixed(0)} s)`);
process.exit(failures.length ? 1 : 0);
