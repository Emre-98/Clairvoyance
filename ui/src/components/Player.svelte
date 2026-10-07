<script lang="ts">
  import type { GameEvent } from "../lib/types";
  import { GROUPS, HIDDEN_BY_DEFAULT, KIND, type Group } from "../lib/eventmeta";
  import { clock } from "../lib/format";
  import Icon from "./Icon.svelte";
  import Timeline from "./Timeline.svelte";
  import { record, navAt } from "../lib/perfmarks";
  import { api } from "../lib/api";
  import { attachVideo, detachVideo, setVideoUrl } from "../lib/videopool";
  import InputOverlay from "./InputOverlay.svelte";
  import { Overlay, parse as parseInput, loadOptions, saveOptions } from "../lib/inputoverlay";
  import { SECS_MAX, SECS_MIN, SECS_STEP } from "../lib/overlayoptions";
  import { frameIndexAt, frameNearest, frameSeekTime, preciseClock } from "../lib/timelineview";
  import { loadPrefs, savePrefs } from "../lib/playerprefs";
  import { collapseLevel, COLLAPSE, type CollapseId } from "../lib/controlsfit";
  import { popfit } from "../lib/popfit";
  import ScoreboardView from "./Scoreboard.svelte";
  import type { PlayerInfo, PlayerStats, Scoreboard } from "../lib/types";

  let {
    src,
    path = null,
    poster = null,
    events,
    offset,
    knownDuration = 0,
    clips = [],
    hidden = $bindable(new Set<Group>(HIDDEN_BY_DEFAULT)),
    range = $bindable(null),
    current = $bindable(0),
    startAt = 0,
    removed = false,
    inputId = null,
    bubbleCats = [],
    heatRange = null,
    apm = null,
    scoreboard = null,
    sbPlayer = null,
    sbStats = null,
  }: {
    src: string | null;
    /** The video file (so the app doesn't replace it while it's open). */
    path?: string | null;
    /** Thumbnail shown until the first video frame is ready. */
    poster?: string | null;
    events: GameEvent[];
    offset: number;
    knownDuration?: number;
    clips?: { start: number; end: number; title: string }[];
    hidden?: Set<Group>;
    range?: [number, number] | null;
    current?: number;
    startAt?: number;
    /** The storage clean-up removed the full video (its kept clips remain). */
    removed?: boolean;
    /** Game id when this game has an input recording (replay overlay); null = none. */
    inputId?: string | null;
    /** Categories of the game's ability bubbles (sub-toggles; empty = the game has none). */
    bubbleCats?: { id: string; label: string }[];
    /** Range (video seconds) for the "selected range" heatmap. */
    heatRange?: [number, number] | null;
    /** APM per 10 s of video (the chart behind the timeline). */
    apm?: (number | null)[] | null;
    /** The time-synced scoreboard (O toggles it over the video; Tab held in fullscreen). */
    scoreboard?: Scoreboard | null;
    sbPlayer?: PlayerInfo | null;
    sbStats?: PlayerStats | null;
  } = $props();
  let sbOpen = $state(false);
  let sbHold = $state(false);
  const sbShown = $derived((sbOpen || sbHold) && !!(scoreboard || sbPlayer));

  // Input overlay: always off when a replay opens; the recording is only loaded the first time
  // it's switched on; its options are remembered between replays.
  let overlayOn = $state(false);
  let overlay = $state.raw<Overlay | null>(null);
  let overlayLoading = $state(false);
  let overlayError = $state<string | null>(null);
  let overlayOpts = $state(loadOptions());
  let optsOpen = $state(false);
  $effect(() => saveOptions($state.snapshot(overlayOpts)));
  /** 0.25 -> "0.25", 1 -> "1", 1.5 -> "1.5". */
  const secsLabel = (v: number) => String(Math.round(v * 100) / 100);

  export async function toggleOverlay() {
    if (!inputId) return;
    if (overlayOn) {
      overlayOn = false;
      return;
    }
    overlayOn = true;
    if (overlay || overlayLoading) return;
    const t0 = performance.now();
    overlayLoading = true;
    overlayError = null;
    try {
      // The ability bubbles load next to the recording (the trail doesn't wait for them; they
      // appear as soon as they're there).
      const id = inputId;
      const load = api.inputLoad(id);
      const actions = api.inputActions(id).catch((e) => {
        console.warn("ability bubbles:", e);
        return null;
      });
      const buf = await load;
      const ov = new Overlay(parseInput(buf));
      overlay = ov;
      requestAnimationFrame(() => {
        const ms = performance.now() - t0;
        record("overlay_on", ms);
        (window as any).__cvOverlayLoadMs = ms;
      });
      actions.then((v) => {
        if (overlay !== ov) return;
        ov.setBubbles(v);
        (window as any).__cvBubblesLoadMs = performance.now() - t0;
        (window as any).__cvBubbleCount = v?.presses.t.length ?? 0;
      });
    } catch (e) {
      overlayError = String(e);
      overlayOn = false;
    } finally {
      overlayLoading = false;
    }
  }

  let video = $state<HTMLVideoElement>();
  let box: HTMLDivElement;
  let timeline = $state<Timeline>();
  let zoomLevel = $state(0);
  let zoomed = $state(false);

  // ---------- fullscreen ----------
  // The video always fills the screen; the controls, timeline and filter chips are a see-through
  // panel drawn over its bottom, which can be slid down (true fullscreen) with the small arrow
  // bubble or H, and brought back from the arrow that appears when hovering the bottom centre.
  let prefs = $state(loadPrefs());
  $effect(() => savePrefs($state.snapshot(prefs)));
  let isFs = $state(false);
  /** Just switched in/out of fullscreen: the panel takes its place without sliding. */
  let fsJust = $state(false);
  let chromeH = $state(0);
  let settingsOpen = $state(false);
  let moreOpen = $state(false);
  /** Opens one of the player's popovers (closing the others). */
  function openPop(which: "opts" | "settings" | "more" | null) {
    optsOpen = which === "opts";
    settingsOpen = which === "settings";
    moreOpen = which === "more";
  }

  // ---------- controls row: what fits at this player width ----------
  // The row never overlaps: when the player gets narrower, controls move into the "More controls"
  // menu (or get shorter) in the order of COLLAPSE (lib/controlsfit.ts), from the measured widths.
  let chromeW = $state(0);
  let timeW = $state(0);
  let cleftEl = $state<HTMLDivElement>();
  let crinEl = $state<HTMLDivElement>();
  /** How many of COLLAPSE are collapsed. */
  let level = $state(0);
  /** Measured widths (CSS px) of the parts that collapse. */
  let nat = $state<Record<string, number>>({});
  /** Last level the row had to go up to, and the room it had then. */
  let fitFloor: { lv: number; avail: number; tw: number; fs: boolean } | null = null;
  const shown = (id: CollapseId) => COLLAPSE.indexOf(id) >= level;
  /** Records an element's width while it's shown. */
  function wid(el: HTMLElement, id: string) {
    const ro = new ResizeObserver(() => {
      const cs = getComputedStyle(el);
      const w = el.offsetWidth + (parseFloat(cs.marginLeft) || 0) + (parseFloat(cs.marginRight) || 0);
      if (w > 0 && Math.abs((nat[id] ?? 0) - w) > 0.5) nat[id] = w;
    });
    ro.observe(el);
    return { destroy: () => ro.disconnect() };
  }
  $effect(() => {
    // Inputs: the width, the measured parts, the time label, fullscreen.
    const tw = timeW;
    const w = chromeW;
    const n = { ...nat };
    const fs = isFs;
    // In fullscreen the time label has no second half (nothing to collapse there).
    if (fs) n.tsub = 0.01;
    const lv = level;
    if (!cleftEl || !crinEl || w <= 0) return;
    const gap = parseFloat(getComputedStyle(crinEl).columnGap) || 0;
    const used = cleftEl.offsetWidth + crinEl.offsetWidth;
    // Fullscreen: the filter chips sit between the two groups and keep at least this much.
    const avail = w - (fs ? 150 : 0) - 2;
    let next = collapseLevel(avail, used, lv, n, gap);
    // Never flap between two levels: after having to go up, stay until there's clearly more room.
    // (A shorter time label, e.g. "Loading" gone, also counts as more room.)
    // The label's width is noted once the row has taken the new level.
    if (fitFloor && Number.isNaN(fitFloor.tw) && lv === fitFloor.lv) fitFloor.tw = tw;
    if (fitFloor && (avail > fitFloor.avail + 8 || tw < fitFloor.tw - 4 || fs !== fitFloor.fs)) fitFloor = null;
    if (next > lv) fitFloor = { lv: next, avail, tw: NaN, fs };
    else if (fitFloor && next < fitFloor.lv) next = Math.min(lv, fitFloor.lv);
    if (next !== lv) level = next;
  });
  $effect(() => {
    // Nothing left in the menu: close it.
    if (level === 0 && moreOpen) moreOpen = false;
  });
  const RATES = [0.25, 0.5, 1, 1.5, 2];
  /** The up-arrow circle (panel down, mouse in the bottom-centre zone). */
  let upShown = $state(false);
  let upTimer: ReturnType<typeof setTimeout> | undefined;
  let cursorHidden = $state(false);
  let cursorTimer: ReturnType<typeof setTimeout> | undefined;
  const panelDown = $derived(isFs && prefs.panelDown);
  const fit = $derived<"contain" | "cover">(isFs && prefs.fit === "fill" ? "cover" : "contain");
  $effect(() => {
    const onFs = () => {
      const t0 = performance.now();
      fsJust = true;
      isFs = !!box && document.fullscreenElement === box;
      requestAnimationFrame(() => requestAnimationFrame(() => (fsJust = false)));
      if (!isFs) {
        upShown = false;
        cursorHidden = false;
      }
      requestAnimationFrame(() => ((window as any).__cvFsSwitchMs = performance.now() - t0));
    };
    document.addEventListener("fullscreenchange", onFs);
    return () => document.removeEventListener("fullscreenchange", onFs);
  });
  export function setPanel(down: boolean) {
    prefs.panelDown = down;
    upShown = false;
    clearTimeout(upTimer);
    if (!down) {
      cursorHidden = false;
      clearTimeout(cursorTimer);
    } else wake();
  }
  function hotEnter() {
    clearTimeout(upTimer);
    upShown = true;
  }
  function hotLeave() {
    clearTimeout(upTimer);
    upTimer = setTimeout(() => (upShown = false), 1000);
  }
  /** Mouse moved: show the cursor; hide it again after 2 s without movement (panel down only). */
  function wake() {
    if (cursorHidden) cursorHidden = false;
    clearTimeout(cursorTimer);
    if (panelDown) cursorTimer = setTimeout(() => (cursorHidden = true), 2000);
  }
  $effect(() => () => {
    clearTimeout(upTimer);
    clearTimeout(cursorTimer);
  });

  // ---------- frame-exact stepping ----------
  // Real frame times from the recording's index (not 1/fps guesses). A step seeks to the middle
  // of the target frame and requestVideoFrameCallback confirms which frame is on screen.
  let frames = $state.raw<Float64Array | null>(null);
  /** Frame index the steps are heading to (null = none in flight). */
  let stepTarget: number | null = null;
  let stepBusy = false;
  let stepT0 = 0;
  let stepSeq = 0;
  /** Frame on screen as reported by the video (requestVideoFrameCallback). */
  let shownFrame = $state<number | null>(null);
  const stepTimes: number[] = [];
  const curFrame = $derived(frames && frames.length ? frameIndexAt(frames, current) : null);
  export function step(n: number) {
    const v = video;
    if (!v || v.readyState < 1) return;
    if (!v.paused) v.pause();
    const f = frames;
    if (!f || !f.length) {
      // No index (e.g. an unreadable file): fall back to a 60 fps guess.
      seek(v.currentTime + n / 60);
      return;
    }
    const from = stepTarget ?? frameIndexAt(f, v.currentTime);
    const to = Math.max(0, Math.min(f.length - 1, from + n));
    if (stepTarget == null) stepT0 = performance.now();
    stepTarget = to;
    if (!stepBusy) stepNow();
  }
  function stepNow() {
    const v = video;
    const f = frames;
    if (!v || !f || stepTarget == null) {
      stepBusy = false;
      return;
    }
    const i = stepTarget;
    const t = frameSeekTime(f, i);
    current = t;
    if (Math.abs(v.currentTime - t) < 1e-6 && !v.seeking) {
      // Already there (first/last frame).
      stepTarget = null;
      stepBusy = false;
      shownFrame = i;
      return;
    }
    stepBusy = true;
    const seq = ++stepSeq;
    let done = false;
    const finish = (mediaTime: number | null) => {
      if (done || seq !== stepSeq) return;
      done = true;
      shownFrame = mediaTime != null ? frameNearest(f, mediaTime) : frameIndexAt(f, v.currentTime);
      if (stepTarget != null && stepTarget !== i) {
        stepNow();
        return;
      }
      const ms = performance.now() - stepT0;
      stepTimes.push(ms);
      if (stepTimes.length > 200) stepTimes.shift();
      (window as any).__cvStepMs = stepTimes;
      (window as any).__cvShownFrame = shownFrame;
      stepTarget = null;
      stepBusy = false;
      busy = false;
    };
    const rvfc = (v as any).requestVideoFrameCallback?.bind(v);
    // The frame presented for this seek: ignore frames still from before it (a step that
    // interrupted playback can present one more frame while the seek starts).
    const onFrame = (_now: number, meta: { mediaTime: number }) => {
      if (done || seq !== stepSeq) return;
      if (frameNearest(f, meta.mediaTime) !== i && (v.seeking || !seekedAt)) {
        rvfc(onFrame);
        return;
      }
      finish(meta.mediaTime);
    };
    let seekedAt = 0;
    if (rvfc) rvfc(onFrame);
    // Fallback when no new frame gets presented (same frame shown again) or no rVFC support.
    v.addEventListener(
      "seeked",
      () => {
        seekedAt = performance.now();
        if (!rvfc) finish(null);
        else setTimeout(() => finish(null), 250);
      },
      { once: true },
    );
    v.currentTime = t;
  }
  // Hold the on-screen step buttons to repeat.
  let holdTimer: ReturnType<typeof setTimeout> | undefined;
  function holdStart(e: PointerEvent, n: number) {
    if (e.button !== 0) return;
    e.preventDefault();
    step(e.shiftKey ? n * 10 : n);
    clearTimeout(holdTimer);
    const again = () => {
      step(n);
      holdTimer = setTimeout(again, 55);
    };
    holdTimer = setTimeout(again, 380);
  }
  function holdEnd() {
    clearTimeout(holdTimer);
  }
  $effect(() => () => clearTimeout(holdTimer));
  let paused = $state(true);
  let duration = $state(0);
  let muted = $state(false);
  let volume = $state(1);
  let rate = $state(1);
  let error = $state<string | null>(null);
  let flash = $state<string | null>(null);
  /** A video frame is on screen (until then: the thumbnail + a loading indicator). */
  let frameReady = $state(false);
  /** Loading or seeking: the small spinner. */
  let busy = $state(true);
  /** A jump requested before the video could seek; done as soon as it can. */
  let pending: { t: number; play: boolean } | null = null;
  // Settings > Advanced: page opened -> first video frame on screen.
  let frameMeasured = false;
  $effect(() => {
    if (frameReady && !frameMeasured) {
      frameMeasured = true;
      const since = navAt;
      if (since && performance.now() - since < 30000) requestAnimationFrame(() => record("replay_frame", performance.now() - since));
    }
  });
  const dur = $derived(duration || knownDuration || 1);
  const visible = $derived(events.filter((e) => !hidden.has((KIND[e.kind] ?? KIND.manual_marker).group)));
  const counts = $derived(
    Object.fromEntries(GROUPS.map((g) => [g.id, events.filter((e) => (KIND[e.kind] ?? KIND.manual_marker).group === g.id).length])),
  );
  // Keyframe times of this video (loaded in the background). A jump that lands on a keyframe
  // shows its frame at once; between keyframes the decoder must run through every frame from
  // the previous one first (older recordings have keyframes up to 5 s apart).
  let keyframes: number[] = [];
  function snap(t: number): number {
    let lo = 0;
    let hi = keyframes.length - 1;
    if (hi < 0 || keyframes[0] > t) return t;
    while (lo < hi) {
      const mid = (lo + hi + 1) >> 1;
      if (keyframes[mid] <= t) lo = mid;
      else hi = mid - 1;
    }
    // At most 3 s earlier than asked (still inside the "5 s before" lead-in); a hair after the
    // keyframe so rounding never lands just before it.
    return t - keyframes[lo] <= 3 ? keyframes[lo] + 0.04 : t;
  }
  const target = (e: GameEvent) => snap(Math.max(0, e.game_time + offset - 5));

  export function seek(t: number, play = false) {
    t = Math.max(0, Math.min(t, duration || knownDuration || Infinity));
    // The playhead moves at once, whatever state the video is in.
    current = t;
    if (!video || video.readyState < 1) {
      // Not ready yet: remember the click and do it as soon as the video can seek.
      pending = { t, play };
      busy = true;
      return;
    }
    if (Math.abs(video.currentTime - t) > 0.01) busy = true;
    video.currentTime = t;
    if (play) video.play().catch(() => {});
  }

  export function jumpTo(e: GameEvent) {
    // Measure click -> the video showing the new position (incl. waiting for the video).
    const t0 = performance.now();
    const v = video;
    if (v) {
      const done = () => requestAnimationFrame(() => record("seek", performance.now() - t0));
      v.addEventListener("seeked", done, { once: true });
    }
    seek(target(e), true);
    flash = e.title;
    setTimeout(() => (flash = null), 1600);
  }

  function next() {
    const n = visible.filter((e) => target(e) > current + 0.5).sort((a, b) => a.game_time - b.game_time)[0];
    if (n) jumpTo(n);
  }
  function prev() {
    const p = visible.filter((e) => target(e) < current - 1.5).sort((a, b) => b.game_time - a.game_time)[0];
    if (p) jumpTo(p);
    else seek(0);
  }
  function toggle() {
    if (!video) return;
    if (video.paused) video.play().catch(() => {});
    else video.pause();
  }
  function fullscreen() {
    if (document.fullscreenElement) document.exitFullscreen();
    else box.requestFullscreen().catch(() => {});
  }
  function toggleGroup(g: Group) {
    const s = new Set(hidden);
    if (s.has(g)) s.delete(g);
    else s.add(g);
    hidden = s;
  }

  $effect(() => {
    if (video) video.playbackRate = rate;
  });
  $effect(() => {
    if (video) {
      video.volume = volume;
      video.muted = muted;
    }
  });

  // Hiding the window (tray) or a game starting pauses playback.
  $effect(() => {
    const onVis = () => {
      if (document.hidden && video && !video.paused) video.pause();
    };
    document.addEventListener("visibilitychange", onVis);
    return () => document.removeEventListener("visibilitychange", onVis);
  });

  // Loop the clip selection while editing.
  $effect(() => {
    if (range && current > range[1] && !paused) seek(range[0], true);
  });

  function loaded() {
    duration = video?.duration && isFinite(video.duration) ? video.duration : 0;
    if (video && video.readyState >= 1 && pending) {
      const p = pending;
      pending = null;
      seek(p.t, p.play);
    }
  }

  // The shared <video> element (see lib/videopool.ts) lives in this host while the page is open.
  function host(node: HTMLDivElement, url: string) {
    const v = attachVideo(node, url);
    const offs: (() => void)[] = [];
    const on = (name: string, f: () => void) => {
      v.addEventListener(name, f);
      offs.push(() => v.removeEventListener(name, f));
    };
    const reset = () => {
      frameReady = v.readyState >= 2;
      busy = v.readyState < 3;
      error = null;
      paused = v.paused;
    };
    reset();
    if (startAt > 0) seek(startAt);
    on("loadedmetadata", loaded);
    on("durationchange", loaded);
    on("loadeddata", () => {
      // With a jump pending, keep the thumbnail until the frame at that position is ready.
      if (!pending && !v.seeking) frameReady = true;
      if (!v.seeking) busy = v.readyState < 3 && !v.paused;
    });
    on("seeked", () => {
      frameReady = true;
      busy = false;
      current = v.currentTime;
    });
    on("canplay", () => {
      if (!v.seeking) busy = false;
    });
    on("waiting", () => (busy = true));
    on("playing", () => (busy = false));
    on("timeupdate", () => {
      if (!pending) current = v.currentTime;
    });
    on("play", () => (paused = false));
    on("pause", () => (paused = true));
    on("error", () => {
      error = "This video can't be played here. Try “Open in player”.";
      busy = false;
    });
    on("click", toggle);
    on("dblclick", fullscreen);
    v.playbackRate = rate;
    v.volume = volume;
    v.muted = muted;
    if (v.readyState >= 1) loaded();
    video = v;
    api.playerOpen(path).catch(() => {});
    keyframes = [];
    frames = null;
    shownFrame = null;
    if (path) {
      const p = path;
      api.videoKeyframes(p).then((k) => {
        if (p === path) keyframes = k;
      }).catch(() => {});
      api.videoFrameTimes(p).then((f) => {
        if (p === path && f.length) frames = f;
      }).catch(() => {});
    }
    return {
      update(next: string) {
        // The file changed (e.g. finalized for playback): same position, new data.
        const at = v.currentTime;
        const wasPlaying = !v.paused;
        setVideoUrl(next);
        reset();
        if (path) {
          api.videoKeyframes(path).then((k) => (keyframes = k)).catch(() => {});
          api.videoFrameTimes(path).then((f) => (frames = f.length ? f : null)).catch(() => {});
        }
        frameReady = false;
        pending = { t: at, play: wasPlaying };
      },
      destroy() {
        offs.forEach((f) => f());
        detachVideo();
        api.playerOpen(null).catch(() => {});
      },
    };
  }

  function typing(el: HTMLElement | null): boolean {
    if (!el) return false;
    if (el.isContentEditable || el.tagName === "TEXTAREA") return true;
    if (el.tagName !== "INPUT") return false;
    const t = (el as HTMLInputElement).type;
    return !["checkbox", "radio", "range", "button", "submit", "color"].includes(t);
  }

  function key(e: KeyboardEvent) {
    if (e.key.toLowerCase() === "i" && !e.ctrlKey && !e.altKey && !e.metaKey && !typing(e.target as HTMLElement)) {
      e.preventDefault();
      toggleOverlay();
      return;
    }
    const tag = (e.target as HTMLElement)?.tagName;
    if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return;
    const k = e.key.toLowerCase();
    // Frame steps by physical key (Shift turns "," into "<" on most layouts).
    if (e.code === "Comma" || e.code === "Period" || k === "," || k === "." || k === "<" || k === ">") {
      if (e.ctrlKey || e.altKey || e.metaKey) return;
      e.preventDefault();
      const back = e.code === "Comma" || k === "," || k === "<";
      step((back ? -1 : 1) * (e.shiftKey ? 10 : 1));
      return;
    }
    if (k === "o" && !e.ctrlKey && !e.altKey && !e.metaKey) {
      e.preventDefault();
      sbOpen = !sbOpen;
      return;
    }
    // Like in League: hold Tab for the scoreboard (fullscreen only; Tab moves the focus otherwise).
    if (e.key === "Tab" && isFs && !e.ctrlKey && !e.altKey) {
      e.preventDefault();
      sbHold = true;
      return;
    }
    if (k === "h" && !e.ctrlKey && !e.altKey && !e.metaKey) {
      if (isFs) {
        e.preventDefault();
        setPanel(!prefs.panelDown);
      }
      return;
    }
    if (k === " " || k === "k") {
      e.preventDefault();
      toggle();
    } else if (k === "arrowleft") seek(current - (e.shiftKey ? 1 : 5));
    else if (k === "arrowright") seek(current + (e.shiftKey ? 1 : 5));
    else if (k === "n") next();
    else if (k === "p") prev();
    else if (k === "f") fullscreen();
    else if (k === "m") muted = !muted;
  }
</script>

<svelte:window onkeydown={key} onkeyup={(e) => e.key === "Tab" && (sbHold = false)} onblur={() => (sbHold = false)} />

{#snippet stepBtn(dir: number)}
  {#if dir < 0}
    <button class="cbtn" onpointerdown={(e) => holdStart(e, -1)} onpointerup={holdEnd} onpointerleave={holdEnd} onpointercancel={holdEnd} title="Previous frame (,) · Shift: 10 frames" aria-label="Previous frame" data-testid="step-back" use:wid={"stepb"}><Icon name="stepback" size={16} /></button>
  {:else}
    <button class="cbtn" onpointerdown={(e) => holdStart(e, 1)} onpointerup={holdEnd} onpointerleave={holdEnd} onpointercancel={holdEnd} title="Next frame (.) · Shift: 10 frames" aria-label="Next frame" data-testid="step-fwd" use:wid={"stepf"}><Icon name="stepfwd" size={16} /></button>
  {/if}
{/snippet}
{#snippet zoomCtl()}
  <button class="cbtn sm" onclick={() => timeline?.zoomBy(0.5)} disabled={!zoomed} aria-label="Zoom out" data-testid="zoom-out"><Icon name="zoomout" size={15} /></button>
  <input class="zslider" type="range" min="0" max="1" step="0.001" value={zoomLevel} oninput={(e) => timeline?.setLevel(Number((e.currentTarget as HTMLInputElement).value))} aria-label="Timeline zoom" data-testid="zoom-slider" />
  <button class="cbtn sm" onclick={() => timeline?.zoomBy(2)} aria-label="Zoom in" data-testid="zoom-in"><Icon name="zoomin" size={15} /></button>
  <button class="cbtn sm" onclick={() => timeline?.fit()} disabled={!zoomed} title="Whole game" aria-label="Show the whole game" data-testid="zoom-fit"><Icon name="fitwidth" size={15} /></button>
{/snippet}
{#snippet rateSel()}
  <select class="rate" bind:value={rate} title="Speed" aria-label="Playback speed">
    {#each RATES as r}<option value={r}>{r}×</option>{/each}
  </select>
{/snippet}
{#snippet volSlider()}
  <input class="vol" type="range" min="0" max="1" step="0.05" bind:value={volume} aria-label="Volume" />
{/snippet}
{#snippet fsSettings()}
  <span class="lbl">Video on a screen with another shape</span>
  <div class="seg nomargin" role="radiogroup" aria-label="Video fit in fullscreen">
    <button role="radio" aria-checked={prefs.fit === "fit"} class:on={prefs.fit === "fit"} onclick={() => (prefs.fit = "fit")} data-testid="fit-fit">Fit (whole video)</button>
    <button role="radio" aria-checked={prefs.fit === "fill"} class:on={prefs.fit === "fill"} onclick={() => (prefs.fit = "fill")} data-testid="fit-fill">Fill (crop)</button>
  </div>
  <label class="slider nopad">
    <span>Controls panel opacity <b>{Math.round(prefs.panelOpacity * 100)}%</b></span>
    <input type="range" min="0.4" max="1" step="0.05" bind:value={prefs.panelOpacity} aria-label="Controls panel opacity" data-testid="panel-opacity" />
  </label>
  <small class="muted">In fullscreen: H or the arrow hides the controls; hover the bottom centre to bring them back.</small>
{/snippet}

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="player"
  class:fs={isFs}
  class:down={panelDown}
  class:fsjust={fsJust}
  class:nocursor={panelDown && cursorHidden}
  bind:this={box}
  style="--panel-a:{prefs.panelOpacity};--chrome-h:{isFs && !panelDown ? chromeH : 0}px;--fit:{fit}"
  onpointermove={isFs ? wake : undefined}
  data-testid="player"
>
  <div class="screen">
    {#if src}
      <div class="vhost" use:host={src}></div>
      {#if !frameReady && !error}
        {#if poster}<img class="poster" src={poster} alt="" draggable="false" />{/if}
      {/if}
      {#if busy && !error}
        <div class="loading" role="status" aria-label="Loading video"><span class="spinner"></span></div>
      {/if}
      {#if paused && !error && frameReady && !busy && !panelDown}
        <button class="bigplay" onclick={toggle} aria-label="Play"><Icon name="play" size={30} fill /></button>
      {/if}
      {#if overlayOn && overlay}
        <InputOverlay {overlay} {video} options={overlayOpts} {heatRange} showUnconfirmed={!hidden.has("unconfirmed")} {fit} insetBottom={isFs && !panelDown ? chromeH : 0} />
      {/if}
      {#if sbShown}
        <div class="sbover" data-testid="scoreboard-overlay">
          <ScoreboardView sb={scoreboard} t={current - offset} player={sbPlayer} stats={sbStats} overlay />
        </div>
      {/if}
      {#if flash}<div class="flash">{flash}</div>{/if}
      {#if error}<div class="err"><Icon name="warn" size={18} />{error}</div>{/if}
    {:else}
      <div class="novideo">
        <Icon name="clips" size={40} stroke={1.4} />
        {#if removed}
          <strong>Full recording removed by the storage clean-up</strong>
          <span>The clips you marked "keep" and the timeline are still here.</span>
        {:else}
          <strong>No video for this game</strong>
          <span>The timeline is still available below.</span>
        {/if}
      </div>
    {/if}
  </div>

  <div class="chrome" bind:clientHeight={chromeH} bind:clientWidth={chromeW} inert={panelDown} data-testid="player-panel">
    {#if isFs}
      <button class="dropbtn" onclick={() => setPanel(true)} title="Hide the controls (H)" aria-label="Hide the controls" data-testid="panel-down"><Icon name="chevdown" size={16} stroke={2.6} /></button>
    {/if}
    <div class="cleft" bind:this={cleftEl}>
      {#if shown("evnav")}<button class="cbtn" onclick={prev} title="Previous event (P)" aria-label="Previous event" use:wid={"evprev"}><Icon name="prev" size={17} /></button>{/if}
      {#if shown("step")}{@render stepBtn(-1)}{/if}
      <button class="cbtn play" onclick={toggle} title="Play/pause (Space)" aria-label={paused ? "Play" : "Pause"}><Icon name={paused ? "play" : "pause"} size={18} fill /></button>
      {#if shown("step")}{@render stepBtn(1)}{/if}
      {#if shown("evnav")}<button class="cbtn" onclick={next} title="Next event (N)" aria-label="Next event" use:wid={"evnext"}><Icon name="next" size={17} /></button>{/if}
      <div class="time" data-testid="time" bind:offsetWidth={timeW}>
        {#if frames && (paused || zoomed)}
          <span class="game">{current - offset < 0 ? "Loading " + preciseClock(current - offset) : preciseClock((frames && curFrame != null ? frames[curFrame] : current) - offset)}</span><span class="muted">&nbsp;· {curFrame != null ? `frame ${curFrame}` : clock(current)}</span>{#if shown("tsub") && !isFs}<span class="muted" use:wid={"tsub"}>&nbsp;· {clock(current)} / {clock(dur)}</span>{/if}
        {:else}
          <span class="game">{current - offset < 0 ? "Loading screen" : clock(current - offset)}</span>{#if shown("tsub")}<span class="muted" use:wid={"tsub"}>&nbsp;game · {clock(current)} / {clock(dur)}</span>{/if}
        {/if}
      </div>
    </div>
    <div class="cright">
      <div class="crin" bind:this={crinEl}>
        {#if shown("zoom")}<div class="zoomctl" title="Zoom the timeline (Ctrl + mouse wheel over it)" use:wid={"zoom"}>{@render zoomCtl()}</div>{/if}
        <div class="ovwrap">
          <button
            class="chip ovchip"
            class:off={!overlayOn}
            class:icononly={!shown("ovtext")}
            style="--c:var(--accent)"
            onclick={toggleOverlay}
            disabled={!inputId}
            aria-pressed={overlayOn}
            aria-label="Input overlay"
            title={inputId ? (overlayError ? `Input overlay: ${overlayError}` : "Input overlay: your cursor, clicks and keys from this game (I)") : "No input recorded for this game"}
            data-testid="overlay-toggle"
          >
            <span class="chip-ic">{#if overlayLoading}<span class="mini-spin"></span>{:else}<Icon name="mouse" size={11} stroke={2.6} />{/if}</span>
            {#if shown("ovtext")}<span use:wid={"ovtext"}>Input overlay</span>{/if}
          </button>
          <button class="cbtn optbtn" onclick={() => openPop(optsOpen ? null : "opts")} disabled={!inputId} title="Input overlay options" aria-label="Input overlay options" aria-expanded={optsOpen} data-testid="overlay-options"><Icon name="sliders" size={15} /></button>
          {#if optsOpen && inputId}
            <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div class="ovpop card" use:popfit onkeydown={(e) => e.key === "Escape" && (optsOpen = false)}>
            <div class="ovhead"><strong>Input overlay</strong><button class="x" onclick={() => (optsOpen = false)} aria-label="Close"><Icon name="x" size={14} /></button></div>
            <label class="check"><input type="checkbox" bind:checked={overlayOpts.trail} />Cursor trail</label>
            <!-- One time for both: a bubble goes together with the trail piece of its moment. -->
            <label class="slider" class:dim={!overlayOpts.trail && !(overlayOpts.bubbles && bubbleCats.length)} title={bubbleCats.length ? "How long the cursor trail is, and how long an ability bubble stays: it goes together with the trail piece under it" : "How long the cursor trail is"}>
              <span>{bubbleCats.length ? "Trail & bubbles" : "Trail length"} <b>{secsLabel(overlayOpts.trailSecs)} s</b></span>
              <input type="range" min={SECS_MIN} max={SECS_MAX} step={SECS_STEP} bind:value={overlayOpts.trailSecs} disabled={!overlayOpts.trail && !(overlayOpts.bubbles && bubbleCats.length)} aria-label="Trail and bubble time in seconds" data-testid="trail-secs" />
            </label>
            <label class="check"><input type="checkbox" bind:checked={overlayOpts.clicks} />Clicks <span class="legend"><i style="background:#4dabf7"></i>left <i style="background:#ff6b6b"></i>right</span></label>
            <label class="check"><input type="checkbox" bind:checked={overlayOpts.dot} />Cursor dot</label>
            <label class="check"><input type="checkbox" bind:checked={overlayOpts.keys} />Keys pressed</label>
            <label class="check"><input type="checkbox" bind:checked={overlayOpts.heat} />Heatmap</label>
            <div class="seg" class:dim={!overlayOpts.heat} role="radiogroup" aria-label="Heatmap range">
              <button role="radio" aria-checked={overlayOpts.heatRange === "game"} class:on={overlayOpts.heatRange === "game"} onclick={() => (overlayOpts.heatRange = "game")} disabled={!overlayOpts.heat}>Whole game</button>
              <button role="radio" aria-checked={overlayOpts.heatRange === "range"} class:on={overlayOpts.heatRange === "range"} onclick={() => (overlayOpts.heatRange = "range")} disabled={!overlayOpts.heat}>Selected range</button>
            </div>
            {#if overlayOpts.heat && overlayOpts.heatRange === "range" && !heatRange}
              <small class="muted">Drag across the APM chart under Mechanics to pick a range (whole game until then).</small>
            {/if}
            {#if bubbleCats.length}
              <div class="ovgroup" data-testid="bubble-options">
                <label class="check"><input type="checkbox" bind:checked={overlayOpts.bubbles} />Ability bubbles</label>
                <div class="subs" class:dim={!overlayOpts.bubbles}>
                  {#each bubbleCats as c (c.id)}
                    <label class="check sub">
                      <input
                        type="checkbox"
                        checked={overlayOpts.bubbleCats[c.id] !== false}
                        disabled={!overlayOpts.bubbles}
                        onchange={(e) => (overlayOpts.bubbleCats = { ...overlayOpts.bubbleCats, [c.id]: (e.currentTarget as HTMLInputElement).checked })}
                      />{c.label}
                    </label>
                  {/each}
                </div>
                {#if overlayOpts.bubbles && !overlayOpts.trail}
                  <small class="muted sub">Bubbles stay as long as the trail would ({secsLabel(overlayOpts.trailSecs)} s).</small>
                {/if}
              </div>
            {/if}
            <small class="muted">Shortcut: I</small>
          </div>
          {/if}
        </div>
        {#if shown("rate")}<span class="rwrap" use:wid={"rate"}>{@render rateSel()}</span>{/if}
        <button class="cbtn" onclick={() => (muted = !muted)} title="Mute (M)" aria-label={muted ? "Unmute" : "Mute"}><Icon name={muted ? "mute" : "volume"} size={17} /></button>
        {#if shown("vol")}<span class="vwrap" use:wid={"vol"}>{@render volSlider()}</span>{/if}
        {#if shown("set")}
          <div class="setwrap" use:wid={"set"}>
            <button class="cbtn" onclick={() => openPop(settingsOpen ? null : "settings")} title="Player settings" aria-label="Player settings" aria-expanded={settingsOpen} data-testid="player-settings"><Icon name="gear" size={16} /></button>
            {#if settingsOpen}
              <!-- svelte-ignore a11y_no_static_element_interactions -->
              <div class="ovpop card setpop" use:popfit onkeydown={(e) => e.key === "Escape" && (settingsOpen = false)}>
                <div class="ovhead"><strong>Fullscreen</strong><button class="x" onclick={() => (settingsOpen = false)} aria-label="Close"><Icon name="x" size={14} /></button></div>
                {@render fsSettings()}
              </div>
            {/if}
          </div>
        {/if}
        {#if level > 0}
          <!-- Controls that don't fit at this player width are in here. -->
          <div class="setwrap">
            <button class="cbtn" onclick={() => openPop(moreOpen ? null : "more")} title="More controls" aria-label="More controls" aria-expanded={moreOpen} data-testid="player-more" use:wid={"more"}><Icon name="more" size={18} stroke={3.4} /></button>
            {#if moreOpen}
              <!-- svelte-ignore a11y_no_static_element_interactions -->
              <div class="ovpop card morepop" use:popfit onkeydown={(e) => e.key === "Escape" && (moreOpen = false)} data-testid="player-more-menu">
                <div class="ovhead"><strong>More controls</strong><button class="x" onclick={() => (moreOpen = false)} aria-label="Close"><Icon name="x" size={14} /></button></div>
                {#if !shown("evnav") || !shown("step")}
                  <div class="mrow">
                    {#if !shown("evnav")}<button class="cbtn" onclick={prev} title="Previous event (P)" aria-label="Previous event"><Icon name="prev" size={17} /></button>{/if}
                    {#if !shown("step")}{@render stepBtn(-1)}{/if}
                    {#if !shown("step")}{@render stepBtn(1)}{/if}
                    {#if !shown("evnav")}<button class="cbtn" onclick={next} title="Next event (N)" aria-label="Next event"><Icon name="next" size={17} /></button>{/if}
                  </div>
                {/if}
                {#if !shown("zoom")}
                  <span class="lbl">Timeline zoom <small class="muted">(Ctrl + wheel)</small></span>
                  <div class="zoomctl mrow">{@render zoomCtl()}</div>
                {/if}
                {#if !shown("rate")}
                  <span class="lbl">Speed</span>
                  <div class="seg nomargin" role="radiogroup" aria-label="Playback speed">
                    {#each RATES as r}<button role="radio" aria-checked={rate === r} class:on={rate === r} onclick={() => (rate = r)}>{r}×</button>{/each}
                  </div>
                {/if}
                {#if !shown("vol")}
                  <label class="slider nopad"><span>Volume <b>{Math.round(volume * 100)}%</b></span>{@render volSlider()}</label>
                {/if}
                {#if !shown("set")}
                  <div class="ovgroup">
                    <strong class="lbl">Fullscreen</strong>
                    {@render fsSettings()}
                  </div>
                {/if}
              </div>
            {/if}
          </div>
        {/if}
        <button class="cbtn" onclick={fullscreen} title="Fullscreen (F)" aria-label="Fullscreen"><Icon name="fullscreen" size={17} /></button>
      </div>
    </div>

    <div class="tlwrap">
      <Timeline bind:this={timeline} events={visible} {apm} {offset} duration={dur} {current} {clips} {frames} compact={isFs} playing={!paused} bind:range bind:level={zoomLevel} bind:zoomed onseek={(t) => seek(t)} onmarker={(e) => jumpTo(e)} />
    </div>

    <div class="filters">
      {#each GROUPS as g}
        {#if counts[g.id] > 0}
          <button class="chip" class:off={hidden.has(g.id)} style="--c:{g.color}" onclick={() => toggleGroup(g.id)} aria-pressed={!hidden.has(g.id)}>
            <span class="chip-ic"><Icon name={g.icon} size={11} stroke={2.6} /></span>
            {g.label}
            <span class="count">{counts[g.id]}</span>
          </button>
        {/if}
      {/each}
    </div>
  </div>

  {#if panelDown}
    <!-- Bottom-centre zone: hovering it shows the arrow that brings the controls back. -->
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div class="hotzone" onpointerenter={hotEnter} onpointerleave={hotLeave} onclick={(e) => e.target === e.currentTarget && toggle()} ondblclick={(e) => e.target === e.currentTarget && fullscreen()} data-testid="hotzone">
      <button class="upbtn" class:show={upShown} onclick={() => setPanel(false)} title="Show the controls (H)" aria-label="Show the controls" tabindex={upShown ? 0 : -1} data-testid="panel-up"><Icon name="chevup" size={18} stroke={2.6} /></button>
    </div>
  {/if}
</div>

<style>
  .player {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    /* Not clipped: the popovers above the controls may reach past the player's top. */
    padding-bottom: 12px;
  }
  .screen {
    position: relative;
    aspect-ratio: 16 / 9;
    background: var(--media-bg);
    overflow: hidden;
    border-radius: calc(var(--radius) - 1px) calc(var(--radius) - 1px) 0 0;
  }
  .vhost {
    position: absolute;
    inset: 0;
  }
  .vhost :global(video) {
    width: 100%;
    height: 100%;
    display: block;
    object-fit: contain;
    background: var(--media-bg);
  }
  .poster {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
    pointer-events: none;
    animation: fade 0.15s;
  }
  .loading {
    position: absolute;
    right: 14px;
    bottom: 12px;
    width: 30px;
    height: 30px;
    border-radius: 50%;
    background: var(--media-overlay);
    display: grid;
    place-items: center;
    pointer-events: none;
    animation: fade 0.2s 0.1s both;
  }
  .spinner {
    width: 16px;
    height: 16px;
    border-radius: 50%;
    border: 2px solid color-mix(in srgb, var(--on-media) 30%, transparent);
    border-top-color: var(--on-media);
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .bigplay {
    position: absolute;
    left: 50%;
    top: 50%;
    transform: translate(-50%, -50%);
    width: 68px;
    height: 68px;
    border-radius: 50%;
    border: none;
    background: var(--media-overlay);
    color: var(--on-media);
    display: grid;
    place-items: center;
    backdrop-filter: blur(6px);
    padding-left: 5px;
  }
  .flash {
    position: absolute;
    left: 16px;
    top: 14px;
    padding: 6px 12px;
    border-radius: 8px;
    background: var(--media-overlay);
    color: var(--on-media);
    font-weight: 600;
    animation: fade 0.2s;
  }
  .err,
  .novideo {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    color: var(--muted);
  }
  .err {
    flex-direction: row;
    color: var(--media-warn);
  }
  /* Controls panel: under the video in the window, over its bottom in fullscreen. */
  .chrome {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    grid-template-areas: "cl cr" "tl tl" "fl fl";
  }
  .cleft {
    grid-area: cl;
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 8px 0 6px 10px;
  }
  .cright {
    grid-area: cr;
    display: flex;
    align-items: center;
    justify-content: flex-end;
    min-width: 0;
  }
  /* The right-hand controls at their natural width (measured to decide what fits). */
  .crin {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 8px 10px 6px 8px;
    flex: none;
  }
  .rwrap,
  .vwrap {
    display: inline-flex;
    align-items: center;
  }
  .tlwrap {
    grid-area: tl;
    min-width: 0;
  }
  .zoomctl {
    display: flex;
    align-items: center;
    gap: 0;
    margin-right: 6px;
  }
  .zslider {
    width: 76px;
    accent-color: var(--accent);
  }
  .cbtn.sm {
    width: 28px;
    height: 28px;
  }
  .cbtn:disabled {
    opacity: 0.35;
    cursor: default;
  }
  .cbtn:disabled:hover {
    background: transparent;
    color: var(--text-2);
  }
  .setwrap {
    position: relative;
  }
  .setpop {
    width: 270px;
  }
  .setpop .lbl {
    font-size: 12px;
    color: var(--text-2);
  }
  .seg.nomargin {
    margin-left: 0;
  }
  .ovpop .slider.nopad {
    padding-left: 0;
  }
  .dropbtn,
  .upbtn {
    position: absolute;
    left: 50%;
    border-radius: 50%;
    display: grid;
    place-items: center;
    padding: 0;
    color: #fff;
    background: rgba(12, 14, 20, 0.82);
    border: 1px solid rgba(255, 255, 255, 0.28);
    box-shadow: 0 2px 10px rgba(0, 0, 0, 0.45);
  }
  .dropbtn {
    top: -12px;
    width: 30px;
    height: 24px;
    margin-left: -15px;
    z-index: 2;
  }
  .dropbtn:hover,
  .upbtn:hover {
    background: rgba(30, 34, 46, 0.95);
  }

  /* ---------- fullscreen: the video fills the screen, the panel floats over its bottom ---------- */
  .player:fullscreen {
    position: relative;
    display: block;
    border: none;
    border-radius: 0;
    padding: 0;
    background: var(--media-bg);
    overflow: hidden;
  }
  .player:fullscreen .screen {
    position: absolute;
    inset: 0;
    aspect-ratio: auto;
  }
  .player:fullscreen .vhost :global(video) {
    object-fit: var(--fit, contain);
  }
  .player:fullscreen .poster {
    object-fit: var(--fit, contain);
  }
  .player:fullscreen .loading {
    bottom: calc(var(--chrome-h) + 12px);
  }
  .player:fullscreen .chrome {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    z-index: 5;
    color-scheme: dark;
    color: var(--text);
    grid-template-columns: auto minmax(0, 1fr) auto;
    grid-template-areas: "tl tl tl" "cl fl cr";
    padding-top: 9px;
    background: linear-gradient(
      to bottom,
      rgba(6, 8, 12, calc(var(--panel-a) * 0.35)) 0,
      rgba(6, 8, 12, calc(var(--panel-a) * 0.9)) 12px,
      rgba(6, 8, 12, var(--panel-a)) 100%
    );
    transform: translateY(0);
    transition:
      transform 0.15s ease-out,
      visibility 0s;
  }
  .player:fullscreen.down .chrome {
    transform: translateY(calc(100% + 32px));
    visibility: hidden;
    transition:
      transform 0.15s ease-in,
      visibility 0s linear 0.15s;
  }
  .player:fullscreen .cleft {
    padding: 0 0 1px 8px;
    gap: 2px;
  }
  .player:fullscreen .crin {
    padding: 0 8px 1px 6px;
    gap: 2px;
  }
  .player:fullscreen .cbtn {
    width: 28px;
    height: 28px;
  }
  .player:fullscreen .cbtn.sm {
    width: 24px;
    height: 24px;
  }
  .player:fullscreen .rate {
    padding: 2px;
  }
  .player:fullscreen .time {
    font-size: 12px;
    white-space: nowrap;
  }
  .player:fullscreen .filters {
    flex-wrap: nowrap;
    overflow-x: auto;
    scrollbar-width: none;
    padding: 0 6px 1px;
    align-items: center;
    gap: 5px;
    min-width: 0;
  }
  .player:fullscreen .chip {
    padding: 2px 8px 2px 3px;
    font-size: 11px;
    gap: 5px;
    white-space: nowrap;
    flex: none;
  }
  .player:fullscreen .chip-ic {
    width: 16px;
    height: 16px;
  }
  .player:fullscreen .zslider {
    width: 64px;
  }
  .player:fullscreen .vol {
    width: 64px;
  }
  .player.fsjust .chrome {
    transition: none !important;
  }
  .hotzone {
    position: absolute;
    left: 50%;
    bottom: 0;
    width: 300px;
    height: 100px;
    margin-left: -150px;
    z-index: 6;
  }
  .upbtn {
    bottom: 18px;
    width: 42px;
    height: 42px;
    margin-left: -21px;
    opacity: 0;
    pointer-events: none;
    transition: opacity 0.1s;
  }
  .upbtn.show {
    opacity: 1;
    pointer-events: auto;
  }
  .player.nocursor,
  .player.nocursor :global(*) {
    cursor: none !important;
  }
  .cbtn {
    width: 34px;
    height: 34px;
    border-radius: 8px;
    border: none;
    background: transparent;
    color: var(--text-2);
    display: grid;
    place-items: center;
  }
  .cbtn:hover {
    background: var(--surface-2);
    color: var(--text);
  }
  .cbtn.play {
    background: var(--surface-2);
    color: var(--text);
  }
  .time {
    margin-left: 8px;
    font-variant-numeric: tabular-nums;
    font-size: 13px;
    white-space: nowrap;
  }
  .time .game {
    font-weight: 700;
  }
  .rate {
    background: transparent;
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 4px;
    color: var(--text-2);
    font-size: 12px;
  }
  .vol {
    width: 80px;
    accent-color: var(--accent);
  }
  .filters {
    grid-area: fl;
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    padding: 12px 12px 0;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    padding: 4px 10px 4px 5px;
    border-radius: 999px;
    border: 1px solid var(--border-2);
    background: var(--surface-2);
    font-size: 12px;
    font-weight: 600;
    color: var(--text);
  }
  .chip-ic {
    width: 18px;
    height: 18px;
    border-radius: 50%;
    background: var(--c);
    color: var(--on-ev);
    display: grid;
    place-items: center;
  }
  .chip .count {
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .chip.off {
    opacity: 0.45;
    background: transparent;
  }
  .chip.off .chip-ic {
    background: var(--surface-3);
    color: var(--muted);
  }
  .ovwrap {
    position: relative;
    display: flex;
    align-items: center;
    gap: 2px;
    margin-right: 6px;
  }
  .ovchip {
    cursor: pointer;
    white-space: nowrap;
  }
  .ovchip.icononly {
    padding: 4px 5px;
  }
  .player:fullscreen .ovchip.icononly {
    padding: 2px 3px;
  }
  .morepop {
    width: 260px;
  }
  .morepop .lbl {
    font-size: 12px;
    color: var(--text-2);
  }
  .mrow {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .morepop .zoomctl {
    margin-right: 0;
  }
  .morepop .zslider {
    flex: 1;
    min-width: 0;
  }
  .morepop .vol,
  .player:fullscreen .morepop .vol {
    width: 100%;
  }
  .ovchip:disabled,
  .optbtn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
  .mini-spin {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    border: 1.5px solid color-mix(in srgb, currentColor 35%, transparent);
    border-top-color: currentColor;
    animation: spin 0.8s linear infinite;
  }
  .ovpop {
    position: absolute;
    right: 0;
    bottom: calc(100% + 8px);
    width: 250px;
    padding: 12px 14px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    z-index: 20;
    background: var(--surface);
    border: 1px solid var(--border-2);
    border-radius: 10px;
    box-shadow: 0 10px 30px rgba(0, 0, 0, 0.35);
    font-size: 13px;
    animation: fade 0.15s;
    max-height: min(70vh, 480px);
    overflow-y: auto;
  }
  /* A popover shorter than its content scrolls; its rows never get squeezed. */
  .ovpop > :global(*) {
    flex-shrink: 0;
  }
  .ovhead {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .ovhead .x {
    border: none;
    background: transparent;
    color: var(--muted);
    display: grid;
    place-items: center;
  }
  .ovpop .check {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .ovpop .slider {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding-left: 24px;
    font-size: 12px;
    color: var(--text-2);
  }
  .ovpop .slider input {
    accent-color: var(--accent);
  }
  .ovpop .dim {
    opacity: 0.45;
  }
  .ovgroup {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding-top: 8px;
    border-top: 1px solid var(--border);
  }
  .ovgroup .subs {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 4px 8px;
    padding-left: 24px;
    font-size: 12px;
    color: var(--text-2);
  }
  .legend {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    margin-left: auto;
    font-size: 11px;
    color: var(--muted);
  }
  .legend i {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    display: inline-block;
    margin-left: 4px;
  }
  .seg {
    display: flex;
    margin-left: 24px;
    border: 1px solid var(--border-2);
    border-radius: 7px;
    overflow: hidden;
  }
  .seg button {
    flex: 1;
    border: none;
    background: transparent;
    padding: 4px 6px;
    font-size: 12px;
    color: var(--text-2);
  }
  .seg button.on {
    background: var(--surface-3);
    color: var(--text);
    font-weight: 600;
  }
  .sbover {
    position: absolute;
    left: 50%;
    top: 12px;
    transform: translateX(-50%);
    width: min(1000px, calc(100% - 24px));
    max-height: calc(100% - 24px - var(--chrome-h, 0px));
    overflow: auto;
    z-index: 4;
  }
</style>
