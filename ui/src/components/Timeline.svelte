<script lang="ts">
  import type { GameEvent } from "../lib/types";
  import { KIND } from "../lib/eventmeta";
  import { clock } from "../lib/format";
  import Icon from "./Icon.svelte";

  interface Placed {
    e: GameEvent;
    pos: number;
    lane: number;
  }

  let {
    events,
    offset,
    duration,
    current,
    clips = [],
    range = $bindable(null),
    onseek,
    onmarker,
  }: {
    events: GameEvent[];
    offset: number;
    duration: number;
    current: number;
    clips?: { start: number; end: number; title: string }[];
    range?: [number, number] | null;
    onseek: (t: number) => void;
    onmarker: (e: GameEvent, pos: number) => void;
  } = $props();

  let width = $state(800);
  let track: HTMLDivElement;
  let hover = $state<Placed | null>(null);
  let hoverX = $state<number | null>(null);

  const MARK = 22;
  const pct = (t: number) => (duration > 0 ? Math.min(100, Math.max(0, (t / duration) * 100)) : 0);

  // Stack markers that would overlap into up to 3 lanes.
  const placed = $derived.by(() => {
    const list = events
      .map((e) => ({ e, pos: e.game_time + offset }))
      .filter((p) => p.pos >= 0 && p.pos <= duration + 1)
      .sort((a, b) => a.pos - b.pos);
    const lastX = [-1e9, -1e9, -1e9];
    const out: Placed[] = [];
    for (const p of list) {
      const x = (p.pos / Math.max(duration, 1)) * width;
      let lane = lastX.findIndex((lx) => x - lx >= MARK - 4);
      if (lane === -1) lane = lastX.indexOf(Math.min(...lastX));
      lastX[lane] = x;
      out.push({ ...p, lane });
    }
    return out;
  });
  const lanes = $derived(Math.max(1, ...placed.map((p) => p.lane + 1)));

  function timeAt(clientX: number) {
    const r = track.getBoundingClientRect();
    return Math.min(duration, Math.max(0, ((clientX - r.left) / r.width) * duration));
  }

  let dragging = $state<null | "seek" | "start" | "end">(null);
  function down(e: PointerEvent, what: "seek" | "start" | "end") {
    e.preventDefault();
    e.stopPropagation();
    dragging = what;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    move(e);
  }
  function move(e: PointerEvent) {
    const r = track?.getBoundingClientRect();
    if (r) hoverX = e.clientX - r.left;
    if (!dragging) return;
    const t = timeAt(e.clientX);
    if (dragging === "seek") onseek(t);
    else if (range) {
      if (dragging === "start") range = [Math.min(t, range[1] - 1), range[1]];
      else range = [range[0], Math.max(t, range[0] + 1)];
      onseek(dragging === "start" ? range[0] : range[1]);
    }
  }
  function up() {
    dragging = null;
  }

  const hoverTime = $derived(hoverX != null && width ? (hoverX / width) * duration : null);
</script>

<div class="timeline" style="--lanes:{lanes}">
  <div class="markers" style="height:{lanes * 26 + 4}px">
    {#each placed as p (p.e.id)}
      {@const m = KIND[p.e.kind] ?? KIND.manual_marker}
      <button
        class="mk"
        class:steal={p.e.steal}
        style="left:{pct(p.pos)}%;bottom:{p.lane * 26}px;--c:{m.color}"
        onmouseenter={() => (hover = p)}
        onmouseleave={() => (hover = null)}
        onclick={() => onmarker(p.e, p.pos)}
        aria-label="{p.e.title} at {clock(p.e.game_time)}"
      >
        <Icon name={m.icon} size={12} stroke={2.4} />
      </button>
    {/each}
    {#if hover}
      {@const m = KIND[hover.e.kind] ?? KIND.manual_marker}
      <div class="tip" style="left:clamp(120px, {pct(hover.pos)}%, calc(100% - 120px));bottom:{hover.lane * 26 + 30}px">
        <div class="tip-head">
          <span class="tip-ic" style="background:{m.color}"><Icon name={m.icon} size={12} stroke={2.4} /></span>
          <strong>{hover.e.title}</strong>
          {#if hover.e.steal}<span class="stealbadge">STEAL</span>{/if}
        </div>
        <div class="tip-sub">{m.label} · {clock(hover.e.game_time)} game time</div>
        {#if hover.e.details}<div class="tip-det">{hover.e.details}</div>{/if}
        <div class="tip-hint">Click to jump to 5 s before</div>
      </div>
    {/if}
  </div>

  <div
    class="track"
    bind:this={track}
    bind:clientWidth={width}
    role="slider"
    tabindex="0"
    aria-valuemin={0}
    aria-valuemax={duration}
    aria-valuenow={current}
    onpointerdown={(e) => down(e, "seek")}
    onpointermove={move}
    onpointerup={up}
    onpointerleave={() => (hoverX = null)}
  >
    <div class="rail"></div>
    <div class="played" style="width:{pct(current)}%"></div>
    {#each clips as c}
      <div class="cliprange" title="Clip: {c.title}" style="left:{pct(c.start)}%;width:{pct(c.end) - pct(c.start)}%"></div>
    {/each}
    {#if offset > 0}
      <div class="loading" style="width:{pct(offset)}%" title="Loading screen (before the game clock started)"></div>
    {/if}
    {#if range}
      <div class="sel" style="left:{pct(range[0])}%;width:{pct(range[1]) - pct(range[0])}%"></div>
      <div class="handle" style="left:{pct(range[0])}%" role="slider" tabindex="0" aria-label="Clip start" aria-valuenow={range[0]} onpointerdown={(e) => down(e, "start")} onpointermove={move} onpointerup={up}></div>
      <div class="handle" style="left:{pct(range[1])}%" role="slider" tabindex="0" aria-label="Clip end" aria-valuenow={range[1]} onpointerdown={(e) => down(e, "end")} onpointermove={move} onpointerup={up}></div>
    {/if}
    {#each placed as p (p.e.id)}
      <div class="tick" style="left:{pct(p.pos)}%;background:{(KIND[p.e.kind] ?? KIND.manual_marker).color}"></div>
    {/each}
    <div class="head" style="left:{pct(current)}%"></div>
    {#if hoverTime != null && !dragging}
      <div class="hovertime" style="left:{hoverX}px">{clock(hoverTime - offset)}</div>
    {/if}
  </div>
</div>

<style>
  .timeline {
    position: relative;
    padding: 0 12px;
    user-select: none;
  }
  .markers {
    position: relative;
    margin-bottom: 4px;
  }
  .mk {
    position: absolute;
    transform: translateX(-50%);
    width: 22px;
    height: 22px;
    border-radius: 50%;
    border: 2px solid var(--surface);
    background: var(--c);
    color: var(--on-ev);
    display: grid;
    place-items: center;
    padding: 0;
    transition: transform 0.1s;
    z-index: 1;
  }
  .mk:hover {
    transform: translateX(-50%) scale(1.25);
    z-index: 3;
  }
  .mk.steal::after {
    content: "";
    position: absolute;
    right: -3px;
    top: -3px;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--text);
    border: 2px solid var(--surface);
  }
  .tip {
    position: absolute;
    transform: translateX(-50%);
    min-width: 200px;
    max-width: 300px;
    background: var(--popover);
    border: 1px solid var(--border-2);
    border-radius: 10px;
    padding: 10px 12px;
    z-index: 10;
    box-shadow: var(--shadow);
    pointer-events: none;
    font-size: 12.5px;
  }
  .tip-head {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .tip-ic {
    width: 20px;
    height: 20px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    color: var(--on-ev);
    flex: none;
  }
  .stealbadge {
    font-size: 10px;
    font-weight: 800;
    letter-spacing: 0.08em;
    padding: 1px 6px;
    border-radius: 4px;
    background: var(--fav);
    color: var(--on-fav);
  }
  .tip-sub {
    color: var(--muted);
    margin-top: 5px;
  }
  .tip-det {
    color: var(--text-2);
    margin-top: 4px;
  }
  .tip-hint {
    color: var(--muted);
    font-size: 11px;
    margin-top: 6px;
  }
  .track {
    position: relative;
    height: 22px;
    cursor: pointer;
    touch-action: none;
  }
  .rail,
  .played,
  .loading,
  .cliprange,
  .sel {
    position: absolute;
    top: 8px;
    height: 6px;
    border-radius: 3px;
  }
  .rail {
    left: 0;
    right: 0;
    background: var(--surface-3);
  }
  .played {
    left: 0;
    background: color-mix(in srgb, var(--text) 35%, transparent);
  }
  .loading {
    left: 0;
    background: repeating-linear-gradient(135deg, color-mix(in srgb, var(--text) 10%, transparent) 0 4px, transparent 4px 8px);
  }
  .cliprange {
    top: 16px;
    height: 3px;
    background: var(--ev-clip);
    opacity: 0.8;
  }
  .sel {
    top: 4px;
    height: 14px;
    border-radius: 4px;
    background: color-mix(in srgb, var(--accent) 22%, transparent);
    border: 1px solid var(--accent);
  }
  .handle {
    position: absolute;
    top: 0;
    width: 10px;
    height: 22px;
    margin-left: -5px;
    border-radius: 3px;
    background: var(--accent);
    cursor: ew-resize;
    z-index: 4;
  }
  .tick {
    position: absolute;
    top: 6px;
    width: 2px;
    height: 10px;
    margin-left: -1px;
    border-radius: 1px;
    opacity: 0.9;
  }
  .head {
    position: absolute;
    top: 2px;
    width: 3px;
    height: 18px;
    margin-left: -1.5px;
    border-radius: 2px;
    background: var(--text);
    box-shadow: 0 0 0 2px var(--surface);
    z-index: 3;
  }
  .hovertime {
    position: absolute;
    top: -22px;
    transform: translateX(-50%);
    font-size: 11px;
    padding: 1px 6px;
    border-radius: 4px;
    background: var(--text);
    color: var(--surface);
    pointer-events: none;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
</style>
