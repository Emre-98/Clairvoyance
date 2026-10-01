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

type Cfg = { sessions: string[]; runs?: number; early_runs?: number; label?: string; finalize?: boolean };

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
    (out.results as any)[id] = { opens, early };
  }
  go({ page: "home" });
  void app;
  await log("finishing");
  await api.benchFinish(out).catch((e) => log(`finish failed: ${e}`));
}
