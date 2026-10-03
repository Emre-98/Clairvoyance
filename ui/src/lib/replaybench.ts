// Replay benchmark (developer tool, see app/src/bench.rs). Opens games in the real window and
// measures, from the click that opens the page:
//   page        timeline markers on screen (painted)
//   firstFrame  a video frame on screen (not a black box / not only the poster)
//   playable    the video can play (readyState >= HAVE_FUTURE_DATA)
//   seek        marker click -> the frame at that position on screen (3 markers per open)
//   early       marker clicked as soon as it appears (video maybe not ready) -> frame there
// Only reads the DOM (`.timeline .mk`, `.player video`), so it measures any version of the UI.

import { api, fileSrc } from "./api";
import { go, app } from "./store.svelte";
import { nextPaint } from "./perfmarks";

type Cfg = { sessions: string[]; runs?: number; early_runs?: number; label?: string; finalize?: boolean; overlay?: boolean; player?: boolean };

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

async function waitFor<T>(f: () => T | null | undefined | false, timeout = 30000): Promise<T | null> {
  const t0 = performance.now();
  for (;;) {
    const v = f();
    if (v) return v;
    if (performance.now() - t0 > timeout) return null;
    // A frame, or 50 ms if frames are throttled (window covered).
    await new Promise<void>((r) => {
      const id = requestAnimationFrame(() => r());
      setTimeout(() => {
        cancelAnimationFrame(id);
        r();
      }, 50);
    });
  }
}

const video = () => document.querySelector<HTMLVideoElement>(".player video");
const markers = () => [...document.querySelectorAll<HTMLButtonElement>(".timeline .mk")];

/** Resolves when `video` presents a new frame (or the next paint, if the API is missing). */
function frameShown(v: HTMLVideoElement, timeout = 500): Promise<number> {
  return new Promise((resolve) => {
    let done = false;
    const fin = (t: number) => {
      if (!done) {
        done = true;
        resolve(t);
      }
    };
    if ("requestVideoFrameCallback" in v) (v as any).requestVideoFrameCallback(() => fin(performance.now()));
    else nextPaint().then(fin);
    setTimeout(() => nextPaint().then(fin), timeout);
  });
}

function nextEvent(v: HTMLVideoElement, name: string, timeout = 30000): Promise<number | null> {
  return new Promise((resolve) => {
    const h = () => {
      clearTimeout(to);
      resolve(performance.now());
    };
    const to = setTimeout(() => {
      v.removeEventListener(name, h);
      resolve(null);
    }, timeout);
    v.addEventListener(name, h, { once: true });
  });
}

async function seekVia(mk: HTMLButtonElement, t0?: number): Promise<number | null> {
  const v = video();
  if (!v) return null;
  const start = t0 ?? performance.now();
  const seeked = nextEvent(v, "seeked");
  mk.click();
  // Some players apply a click made before the video is ready later: wait for that seek.
  let t = await seeked;
  if (t == null) return null;
  // A pending seek may land in two steps (metadata, then the target): wait until we're there.
  await waitFor(() => !v.seeking, 30000);
  const shown = await frameShown(v);
  v.pause();
  return Math.round(shown - start);
}

async function openOnce(id: string, early: boolean) {
  go({ page: "games" });
  await sleep(900);
  const t0 = performance.now();
  go({ page: "game", id });
  const r: Record<string, number | null | number[]> = {};
  // The video element may appear before or after the markers; watch both.
  let firstFrame: Promise<number | null> = (async () => {
    const v = await waitFor(() => video(), 30000);
    if (!v) return null;
    if (v.readyState >= 2) return (await nextPaint()) - t0;
    // The frame callback fires when the first frame is composited; a paused video may not
    // call it, so "data for the first frame + the next paint" counts too (whichever is first).
    const viaCallback = new Promise<number>((res) => {
      if ("requestVideoFrameCallback" in v) (v as any).requestVideoFrameCallback(() => res(performance.now()));
    });
    const viaPaint = nextEvent(v, "loadeddata").then((t) => (t == null ? null : nextPaint()));
    const t = await Promise.race([viaCallback, viaPaint]);
    return t == null ? null : t - t0;
  })();
  const playable: Promise<number | null> = (async () => {
    const v = await waitFor(() => video(), 30000);
    if (!v) return null;
    if (v.readyState >= 3) return performance.now() - t0;
    const c = await nextEvent(v, "canplay");
    return c == null ? null : c - t0;
  })();
  const mk = await waitFor(() => markers().length > 0 && markers(), 30000);
  r.page = mk ? Math.round((await nextPaint()) - t0) : null;
  if (early && mk) {
    // Click a marker right away, whatever state the video is in.
    const m = mk[Math.floor(mk.length / 2)];
    r.early = await seekVia(m, performance.now());
  }
  const ff = await firstFrame;
  r.firstFrame = ff == null ? null : Math.round(ff);
  const pl = await playable;
  r.playable = pl == null ? null : Math.round(pl);
  if (!early && mk) {
    const seeks: number[] = [];
    for (const f of [0.25, 0.5, 0.75]) {
      const list = markers();
      const m = list[Math.min(list.length - 1, Math.floor(list.length * f))];
      const s = m ? await seekVia(m) : null;
      if (s != null) seeks.push(s);
      await sleep(300);
    }
    r.seeks = seeks;
  }
  video()?.pause();
  return r;
}

async function environment(sample: string | null) {
  const env: Record<string, unknown> = { userAgent: navigator.userAgent };
  if (sample) {
    // Does the video protocol answer range requests (206 + Content-Range)?
    try {
      const res = await fetch(fileSrc(sample), { headers: { Range: "bytes=0-99" } });
      const body = await res.arrayBuffer();
      env.range = { status: res.status, contentRange: res.headers.get("content-range"), bytes: body.byteLength };
    } catch (e) {
      env.range = { error: String(e) };
    }
  }
  return env;
}

async function decodingInfo(codec: string | null, w: number, h: number) {
  if (!codec || !("mediaCapabilities" in navigator)) return null;
  try {
    const i = await navigator.mediaCapabilities.decodingInfo({
      type: "file",
      video: { contentType: `video/mp4; codecs="${codec}"`, width: w || 1920, height: h || 1080, bitrate: 12_000_000, framerate: 60 },
    });
    return { codec, supported: i.supported, smooth: i.smooth, powerEfficient: i.powerEfficient };
  } catch (e) {
    return { codec, error: String(e) };
  }
}

const overlayCanvas = () => document.querySelector<HTMLCanvasElement>('[data-testid="input-overlay"]');
const overlayToggle = () => document.querySelector<HTMLButtonElement>('[data-testid="overlay-toggle"]');
const drawTimes = (): number[] => [...((window as any).__cvOverlayDrawMs ?? [])];
const pressI = () => window.dispatchEvent(new KeyboardEvent("keydown", { key: "i" }));
function pct(a: number[], p: number) {
  const s = [...a].sort((x, y) => x - y);
  return s.length ? Math.round(s[Math.min(s.length - 1, Math.floor(p * s.length))] * 1000) / 1000 : null;
}

/** Plays `secs` at `rate` while switching the overlay every 300 ms: stalls and progress. */
async function playToggling(v: HTMLVideoElement, rate: number, secs: number) {
  let waiting = 0;
  const onw = () => waiting++;
  v.addEventListener("waiting", onw);
  v.playbackRate = rate;
  await v.play().catch(() => {});
  await waitFor(() => !v.paused && v.readyState >= 3, 5000);
  const c0 = v.currentTime;
  const w0 = performance.now();
  let toggles = 0;
  while (performance.now() - w0 < secs * 1000) {
    await sleep(300);
    pressI();
    toggles++;
  }
  const adv = v.currentTime - c0;
  const wall = (performance.now() - w0) / 1000;
  v.pause();
  v.playbackRate = 1;
  v.removeEventListener("waiting", onw);
  return { rate, toggles, advanced: Math.round(adv * 100) / 100, expected: Math.round(wall * rate * 100) / 100, stalls: waiting };
}

/** The input overlay in the real window: off on open, first switch-on (loads the recording),
 * switching on again, toggling while playing at 1x/2x/0.25x, draw time per frame with the default
 * options and with everything on, marker jumps with the overlay on. */
async function overlayBench(id: string) {
  const r: Record<string, unknown> = {};
  go({ page: "games" });
  await sleep(900);
  go({ page: "game", id });
  const v = await waitFor(() => video(), 30000);
  const tg = await waitFor(() => overlayToggle(), 10000);
  if (!v || !tg) return { error: "no player" };
  await waitFor(() => v.readyState >= 3, 30000);
  r.offOnOpen = tg.getAttribute("aria-pressed") === "false" && !overlayCanvas();
  r.disabled = tg.disabled;
  if (tg.disabled) return r;
  // Default options for the first part.
  localStorage.removeItem("cv.inputOverlay");
  for (const k of ["__cvBubblesLoadMs", "__cvBubbleCount", "__cvBubbles", "__cvBubbleStats"]) delete (window as any)[k];
  // First switch-on: load + first frame drawn.
  const n0 = drawTimes().length;
  let t0 = performance.now();
  tg.click();
  await waitFor(() => overlayCanvas() && (drawTimes().length > n0 || (window as any).__cvOverlayLoadMs), 30000);
  await nextPaint();
  r.firstOnMs = Math.round(performance.now() - t0);
  r.loadMs = Math.round((window as any).__cvOverlayLoadMs ?? -1);
  // Ability bubbles (v1.5; older builds have none): loaded next to the recording.
  await waitFor(() => (window as any).__cvBubblesLoadMs != null, 5000);
  r.bubblesMs = (window as any).__cvBubblesLoadMs != null ? Math.round((window as any).__cvBubblesLoadMs) : null;
  r.bubbles = (window as any).__cvBubbleCount ?? null;
  // Off and on again (already loaded).
  const again: number[] = [];
  for (let i = 0; i < 5; i++) {
    pressI();
    await sleep(200);
    t0 = performance.now();
    pressI();
    await waitFor(() => overlayCanvas(), 5000);
    await nextPaint();
    again.push(Math.round(performance.now() - t0));
    await sleep(100);
  }
  r.againOnMs = again;
  // Toggling during playback.
  v.currentTime = Math.min(v.duration * 0.4, 600);
  await nextEvent(v, "seeked", 10000);
  r.playing = [await playToggling(v, 1, 3), await playToggling(v, 2, 3), await playToggling(v, 0.25, 2)];
  // Draw times while playing, overlay on: default options, then everything on.
  for (const [label, opts] of [
    ["default", null],
    ["everything", { trail: true, trailSecs: 3, clicks: true, dot: true, keys: true, heat: true, heatRange: "game" }],
  ] as const) {
    if (opts) {
      // Options apply on the next open of the popover / effect; set them and re-open the page.
      localStorage.setItem("cv.inputOverlay", JSON.stringify(opts));
      go({ page: "games" });
      await sleep(600);
      go({ page: "game", id });
      await waitFor(() => video()?.readyState! >= 3 && overlayToggle(), 30000);
      overlayToggle()!.click();
      await waitFor(() => overlayCanvas(), 30000);
    }
    if (!overlayCanvas()) pressI();
    const vv = video()!;
    vv.currentTime = Math.min(vv.duration * 0.5, 900);
    await nextEvent(vv, "seeked", 10000);
    await vv.play().catch(() => {});
    await sleep(4000);
    vv.pause();
    const d = drawTimes().slice(-200);
    r[`draw_${label}`] = { frames: d.length, median: pct(d, 0.5), p95: pct(d, 0.95), max: pct(d, 1) };
  }
  // Ability bubbles at their worst: the densest 3 s of presses (Q spam), fade 3 s, played.
  const B = (window as any).__cvBubbles;
  if (B?.v?.presses?.t?.length) {
    const show: number[] = B.v.presses.show;
    let best = 0, at = 0;
    for (let i = 0, j = 0; i < show.length; i++) {
      while (show[i] - show[j] >= 3) j++;
      if (i - j + 1 > best) {
        best = i - j + 1;
        at = show[i];
      }
    }
    localStorage.setItem("cv.inputOverlay", JSON.stringify({ trailSecs: 3, v: 2 }));
    go({ page: "games" });
    await sleep(600);
    go({ page: "game", id });
    await waitFor(() => video()?.readyState! >= 3 && overlayToggle(), 30000);
    overlayToggle()!.click();
    await waitFor(() => overlayCanvas() && (window as any).__cvBubbles?.v, 30000);
    const vv = video()!;
    vv.currentTime = Math.max(0, at - 2.6);
    await nextEvent(vv, "seeked", 10000);
    await vv.play().catch(() => {});
    let maxDrawn = 0;
    const w0 = performance.now();
    while (performance.now() - w0 < 3200) {
      await nextPaint();
      maxDrawn = Math.max(maxDrawn, (window as any).__cvBubbleStats?.drawn ?? 0);
    }
    vv.pause();
    const d = drawTimes().slice(-150);
    r.draw_spam = { densest3s: best, at: Math.round(at * 10) / 10, maxOnScreen: maxDrawn, layoutMs: Math.round(((window as any).__cvBubbleStats?.layoutMs ?? 0) * 100) / 100, frames: d.length, median: pct(d, 0.5), p95: pct(d, 0.95), max: pct(d, 1) };
    localStorage.removeItem("cv.inputOverlay");
  }
  // Marker jumps with the overlay on.
  const seeks: number[] = [];
  for (const f of [0.2, 0.5, 0.8]) {
    const list = markers();
    const m = list[Math.min(list.length - 1, Math.floor(list.length * f))];
    const s = m ? await seekVia(m) : null;
    if (s != null) seeks.push(s);
    await sleep(300);
  }
  r.seeksWithOverlay = seeks;
  localStorage.removeItem("cv.inputOverlay");
  // Reopening: off again.
  go({ page: "games" });
  await sleep(600);
  go({ page: "game", id });
  const tg2 = await waitFor(() => overlayToggle(), 10000);
  await sleep(300);
  r.offOnReopen = tg2?.getAttribute("aria-pressed") === "false" && !overlayCanvas();
  video()?.pause();
  return r;
}

const key = (k: string, code: string, shift = false) => window.dispatchEvent(new KeyboardEvent("keydown", { key: k, code, shiftKey: shift }));
const stepTimes = (): number[] => [...((window as any).__cvStepMs ?? [])];

/** v1.6 player: frame steps (forward / back), timeline zoom speed, fullscreen switch and the
 * panel. Builds without these controls report `{ supported: false }`. */
async function playerBench(id: string) {
  const r: Record<string, unknown> = {};
  go({ page: "games" });
  await sleep(900);
  go({ page: "game", id });
  const v = await waitFor(() => video(), 30000);
  if (!v) return { error: "no player" };
  await waitFor(() => v.readyState >= 3, 30000);
  // v1.7: in a narrow window the zoom controls are in the "More controls" menu.
  if (!document.querySelector('[data-testid="zoom-slider"]')) {
    document.querySelector<HTMLButtonElement>('[data-testid="player-more"]')?.click();
    await sleep(200);
  }
  if (!document.querySelector('[data-testid="zoom-slider"]')) return { supported: false };
  await sleep(800); // frame times
  // Frame steps in the middle of the game: 30 forward, 30 back (each waits for its frame).
  v.currentTime = v.duration * 0.5;
  await nextEvent(v, "seeked", 10000);
  await sleep(300);
  const step = async (k: string, code: string) => {
    const n0 = stepTimes().length;
    key(k, code);
    await waitFor(() => stepTimes().length > n0, 5000);
    return stepTimes()[stepTimes().length - 1];
  };
  const fwd: number[] = [], back: number[] = [];
  for (let i = 0; i < 30; i++) fwd.push(await step(".", "Period"));
  for (let i = 0; i < 30; i++) back.push(await step(",", "Comma"));
  r.stepForward = { median: pct(fwd, 0.5), p95: pct(fwd, 0.95), max: pct(fwd, 1) };
  r.stepBack = { median: pct(back, 0.5), p95: pct(back, 0.95), max: pct(back, 1) };
  // Zoom: the slider moved every frame for 4 s (in and out), redraw time and frame intervals.
  const s = document.querySelector<HTMLInputElement>('[data-testid="zoom-slider"]')!;
  (window as any).__cvTimelineDrawMs && ((window as any).__cvTimelineDrawMs.length = 0);
  const gaps: number[] = [];
  let last = performance.now();
  for (let i = 0; i < 240; i++) {
    s.value = String(0.5 - 0.5 * Math.cos((i / 120) * Math.PI));
    s.dispatchEvent(new Event("input", { bubbles: true }));
    await new Promise((res) => requestAnimationFrame(res));
    const now = performance.now();
    gaps.push(now - last);
    last = now;
  }
  const d: number[] = [...((window as any).__cvTimelineDrawMs ?? [])];
  r.zoom = { markers: markers().length, draw: { median: pct(d, 0.5), p95: pct(d, 0.95), max: pct(d, 1) }, frameInterval: { median: pct(gaps.slice(5), 0.5), p95: pct(gaps.slice(5), 0.95), max: pct(gaps.slice(5), 1) } };
  document.querySelector<HTMLButtonElement>('[data-testid="zoom-fit"]')?.click();
  // Fullscreen (needs a user gesture in some WebView2 versions: then only the error is noted).
  const p = document.querySelector<HTMLElement>('[data-testid="player"]');
  const fs: number[] = [], exit: number[] = [];
  try {
    for (let i = 0; i < 3 && p; i++) {
      let t0 = performance.now();
      let ch = new Promise((res) => document.addEventListener("fullscreenchange", res, { once: true }));
      await p.requestFullscreen();
      await ch;
      await nextPaint();
      fs.push(Math.round(performance.now() - t0));
      await sleep(400);
      if (i === 0) {
        const pr = document.querySelector('[data-testid="player-panel"]')!.getBoundingClientRect();
        const vr = v.getBoundingClientRect();
        r.fullscreenLayout = { screen: [innerWidth, innerHeight, devicePixelRatio], video: [vr.x, vr.y, vr.width, vr.height], panelHeight: Math.round(pr.height), panelPct: Math.round((pr.height / innerHeight) * 1000) / 10 };
        const panel: number[] = [];
        for (let k = 0; k < 4; k++) {
          const t1 = performance.now();
          const end = new Promise((res) => document.querySelector('[data-testid="player-panel"]')!.addEventListener("transitionend", res, { once: true }));
          key("h", "KeyH");
          await Promise.race([end, sleep(1000)]);
          panel.push(Math.round(performance.now() - t1));
          await sleep(200);
        }
        r.panelSlideMs = panel;
      }
      t0 = performance.now();
      ch = new Promise((res) => document.addEventListener("fullscreenchange", res, { once: true }));
      await document.exitFullscreen();
      await ch;
      await nextPaint();
      exit.push(Math.round(performance.now() - t0));
      await sleep(400);
    }
    r.fullscreenEnterMs = fs;
    r.fullscreenExitMs = exit;
  } catch (e) {
    r.fullscreen = String(e);
  }
  v.pause();
  return r;
}

export async function runBench(cfg: Cfg) {
  const log = (s: string) => api.benchLog(s).catch(() => {});
  const runs = cfg.runs ?? 5;
  const earlyRuns = cfg.early_runs ?? 2;
  await log(`start ${cfg.label ?? ""}: ${cfg.sessions.join(", ")}`);
  let prepared: Record<string, any>;
  try {
    prepared = await api.benchPrepare(cfg.sessions, !!cfg.finalize);
  } catch (e) {
    await log(`prepare failed: ${e}`);
    throw e;
  }
  // Let the library pick up the finalized files.
  await sleep(1500);
  const out: Record<string, unknown> = { label: cfg.label, at: new Date().toISOString(), prepared, results: {} };
  const firstVideo = (Object.values(prepared)[0] as any)?.video ?? null;
  out.environment = await environment(firstVideo);
  const codecs: unknown[] = [];
  for (const p of Object.values(prepared) as any[]) {
    const i = p.after ?? p.before;
    codecs.push(await decodingInfo(i?.codec ?? null, i?.width, i?.height));
  }
  out.decoding = codecs;
  await log(`prepared: ${JSON.stringify(prepared).slice(0, 400)}`);
  for (const id of cfg.sessions) {
    const opens = [];
    for (let i = 0; i < runs; i++) {
      const r = await openOnce(id, false);
      opens.push(r);
      await log(`${id} open ${i + 1}: ${JSON.stringify(r)}`);
    }
    const early = [];
    for (let i = 0; i < earlyRuns; i++) {
      const r = await openOnce(id, true);
      early.push(r);
      await log(`${id} early ${i + 1}: ${JSON.stringify(r)}`);
    }
    let overlay: unknown = undefined;
    if (cfg.overlay) {
      try {
        overlay = await overlayBench(id);
      } catch (e) {
        overlay = { error: String(e) };
      }
      await log(`${id} overlay: ${JSON.stringify(overlay)}`);
    }
    let player: unknown = undefined;
    if (cfg.player) {
      try {
        player = await playerBench(id);
      } catch (e) {
        player = { error: String(e) };
      }
      await log(`${id} player: ${JSON.stringify(player)}`);
    }
    (out.results as any)[id] = { opens, early, overlay, player };
  }
  go({ page: "home" });
  void app;
  await log("finishing");
  await api.benchFinish(out).catch((e) => log(`finish failed: ${e}`));
}
