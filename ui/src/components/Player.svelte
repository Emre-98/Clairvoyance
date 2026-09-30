<script lang="ts">
  import type { GameEvent } from "../lib/types";
  import { GROUPS, KIND, type Group } from "../lib/eventmeta";
  import { clock } from "../lib/format";
  import Icon from "./Icon.svelte";
  import Timeline from "./Timeline.svelte";
  import { record } from "../lib/perfmarks";

  let {
    src,
    events,
    offset,
    knownDuration = 0,
    clips = [],
    hidden = $bindable(new Set<Group>(["game"])),
    range = $bindable(null),
    current = $bindable(0),
    startAt = 0,
    removed = false,
  }: {
    src: string | null;
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

  const dur = $derived(duration || knownDuration || 1);
  const visible = $derived(events.filter((e) => !hidden.has((KIND[e.kind] ?? KIND.manual_marker).group)));
  const counts = $derived(
    Object.fromEntries(GROUPS.map((g) => [g.id, events.filter((e) => (KIND[e.kind] ?? KIND.manual_marker).group === g.id).length])),
  );
  const target = (e: GameEvent) => Math.max(0, e.game_time + offset - 5);

  export function seek(t: number, play = false) {
    if (!video) return;
    video.currentTime = Math.max(0, Math.min(t, dur));
    current = video.currentTime;
    if (play) video.play().catch(() => {});
  }

  export function jumpTo(e: GameEvent) {
    // Measure click -> the video showing the new position.
    if (video) {
      const t0 = performance.now();
      video.addEventListener("seeked", () => requestAnimationFrame(() => record("seek", performance.now() - t0)), { once: true });
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

  let didStart = false;
  function loaded() {
    duration = video?.duration && isFinite(video.duration) ? video.duration : 0;
    if (!didStart && startAt > 0) {
      didStart = true;
      seek(startAt);
    }
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
      <!-- svelte-ignore a11y_media_has_caption -->
      <video
        bind:this={video}
        {src}
        preload="auto"
        onclick={toggle}
        ondblclick={fullscreen}
        onloadedmetadata={loaded}
        ondurationchange={loaded}
        ontimeupdate={() => (current = video?.currentTime ?? 0)}
        onplay={() => (paused = false)}
        onpause={() => (paused = true)}
        onerror={() => (error = "This video can't be played here. Try “Open in player”.")}
      ></video>
      {#if paused && !error}
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
      <span class="game">{current - offset < 0 ? "Loading" : clock(current - offset)}</span>
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
  video {
    width: 100%;
    height: 100%;
    display: block;
    object-fit: contain;
    background: var(--media-bg);
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
