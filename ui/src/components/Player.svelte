<script lang="ts">
  import type { GameEvent } from "../lib/types";
  import { GROUPS, HIDDEN_BY_DEFAULT, KIND, type Group } from "../lib/eventmeta";
  import { clock } from "../lib/format";
  import Icon from "./Icon.svelte";
  import Timeline from "./Timeline.svelte";
  import { record, navAt } from "../lib/perfmarks";
  import { api } from "../lib/api";
  import { attachVideo, detachVideo, setVideoUrl } from "../lib/videopool";

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
  } = $props();

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

  function key(e: KeyboardEvent) {
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
</style>
