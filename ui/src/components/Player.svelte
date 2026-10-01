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
    heatRange = null,
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
    /** Range (video seconds) for the "selected range" heatmap. */
    heatRange?: [number, number] | null;
  } = $props();

  // Input overlay: always off when a replay opens; the recording is only loaded the first time
  // it's switched on; its options are remembered between replays.
  let overlayOn = $state(false);
  let overlay = $state.raw<Overlay | null>(null);
  let overlayLoading = $state(false);
  let overlayError = $state<string | null>(null);
  let overlayOpts = $state(loadOptions());
  let optsOpen = $state(false);
  $effect(() => saveOptions($state.snapshot(overlayOpts)));

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
      const buf = await api.inputLoad(inputId);
      overlay = new Overlay(parseInput(buf));
      requestAnimationFrame(() => {
        const ms = performance.now() - t0;
        record("overlay_on", ms);
        (window as any).__cvOverlayLoadMs = ms;
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
    if (path) {
      const p = path;
      api.videoKeyframes(p).then((k) => {
        if (p === path) keyframes = k;
      }).catch(() => {});
    }
    return {
      update(next: string) {
        // The file changed (e.g. finalized for playback): same position, new data.
        const at = v.currentTime;
        const wasPlaying = !v.paused;
        setVideoUrl(next);
        reset();
        if (path) api.videoKeyframes(path).then((k) => (keyframes = k)).catch(() => {});
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
    if (k === " " || k === "k") {
      e.preventDefault();
      toggle();
    } else if (k === "arrowleft") seek(current - (e.shiftKey ? 1 : 5));
    else if (k === "arrowright") seek(current + (e.shiftKey ? 1 : 5));
    else if (k === "n") next();
    else if (k === "p") prev();
    else if (k === "f") fullscreen();
    else if (k === "m") muted = !muted;
    else if (k === ",") seek(current - 1 / 60);
    else if (k === ".") seek(current + 1 / 60);
  }
</script>

<svelte:window onkeydown={key} />

<div class="player" bind:this={box}>
  <div class="screen">
    {#if src}
      <div class="vhost" use:host={src}></div>
      {#if !frameReady && !error}
        {#if poster}<img class="poster" src={poster} alt="" draggable="false" />{/if}
      {/if}
      {#if busy && !error}
        <div class="loading" role="status" aria-label="Loading video"><span class="spinner"></span></div>
      {/if}
      {#if paused && !error && frameReady && !busy}
        <button class="bigplay" onclick={toggle} aria-label="Play"><Icon name="play" size={30} fill /></button>
      {/if}
      {#if overlayOn && overlay}
        <InputOverlay {overlay} {video} options={overlayOpts} {heatRange} />
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

  <div class="controls">
    <button class="cbtn" onclick={prev} title="Previous event (P)"><Icon name="prev" size={17} /></button>
    <button class="cbtn play" onclick={toggle} title="Play/pause (Space)"><Icon name={paused ? "play" : "pause"} size={18} fill /></button>
    <button class="cbtn" onclick={next} title="Next event (N)"><Icon name="next" size={17} /></button>
    <div class="time">
      <span class="game">{current - offset < 0 ? "Loading screen" : clock(current - offset)}</span>
      <span class="muted"> game · {clock(current)} / {clock(dur)}</span>
    </div>
    <div class="spacer"></div>
    <div class="ovwrap">
      <button
        class="chip ovchip"
        class:off={!overlayOn}
        style="--c:var(--accent)"
        onclick={toggleOverlay}
        disabled={!inputId}
        aria-pressed={overlayOn}
        title={inputId ? (overlayError ? `Input overlay: ${overlayError}` : "Input overlay: your cursor, clicks and keys from this game (I)") : "No input recorded for this game"}
        data-testid="overlay-toggle"
      >
        <span class="chip-ic">{#if overlayLoading}<span class="mini-spin"></span>{:else}<Icon name="mouse" size={11} stroke={2.6} />{/if}</span>
        Input overlay
      </button>
      <button class="cbtn optbtn" onclick={() => (optsOpen = !optsOpen)} disabled={!inputId} title="Input overlay options" aria-expanded={optsOpen} data-testid="overlay-options"><Icon name="sliders" size={15} /></button>
      {#if optsOpen && inputId}
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div class="ovpop card" onkeydown={(e) => e.key === "Escape" && (optsOpen = false)}>
          <div class="ovhead"><strong>Input overlay</strong><button class="x" onclick={() => (optsOpen = false)} aria-label="Close"><Icon name="x" size={14} /></button></div>
          <label class="check"><input type="checkbox" bind:checked={overlayOpts.trail} />Cursor trail</label>
          <label class="slider" class:dim={!overlayOpts.trail}>
            <span>Trail length <b>{overlayOpts.trailSecs.toFixed(2).replace(/0$/, "")} s</b></span>
            <input type="range" min="0.25" max="3" step="0.25" bind:value={overlayOpts.trailSecs} disabled={!overlayOpts.trail} aria-label="Trail length in seconds" />
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
          <small class="muted">Shortcut: I</small>
        </div>
      {/if}
    </div>
    <select class="rate" bind:value={rate} title="Speed">
      {#each [0.25, 0.5, 1, 1.5, 2] as r}<option value={r}>{r}×</option>{/each}
    </select>
    <button class="cbtn" onclick={() => (muted = !muted)} title="Mute (M)"><Icon name={muted ? "mute" : "volume"} size={17} /></button>
    <input class="vol" type="range" min="0" max="1" step="0.05" bind:value={volume} aria-label="Volume" />
    <button class="cbtn" onclick={fullscreen} title="Fullscreen (F)"><Icon name="fullscreen" size={17} /></button>
  </div>

  <Timeline events={visible} {offset} duration={dur} {current} {clips} bind:range onseek={(t) => seek(t)} onmarker={(e) => jumpTo(e)} />

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

<style>
  .player {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    overflow: hidden;
    padding-bottom: 12px;
  }
  .player:fullscreen {
    display: flex;
    flex-direction: column;
    border-radius: 0;
  }
  .player:fullscreen .screen {
    flex: 1;
    aspect-ratio: auto;
  }
  .screen {
    position: relative;
    aspect-ratio: 16 / 9;
    background: var(--media-bg);
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
  .controls {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 8px 10px 6px;
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
</style>
