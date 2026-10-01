<script lang="ts">
  // Post-game "Mechanics": stats from the mouse/keyboard recording, computed after the game
  // (cached in session.json). Drag across the per-minute APM chart to see the same numbers for
  // a range (computed on demand from the recording); the range also drives the overlay's
  // "selected range" heatmap.
  import type { Mechanics } from "../lib/types";
  import { api } from "../lib/api";
  import { clock } from "../lib/format";

  let {
    whole,
    id,
    offset,
    canRange,
    range = $bindable(null),
  }: {
    whole: Mechanics;
    id: string;
    offset: number;
    /** The recording is still there (range stats need it). */
    canRange: boolean;
    /** Selected range in game seconds. */
    range?: [number, number] | null;
  } = $props();

  let ranged = $state<Mechanics | null>(null);
  let rangeErr = $state<string | null>(null);
  let rangeBusy = $state(false);
  const m = $derived(range && ranged ? ranged : whole);

  $effect(() => {
    const r = range;
    ranged = null;
    rangeErr = null;
    if (!r || !canRange) return;
    let alive = true;
    rangeBusy = true;
    api
      .inputStats(id, r[0] + offset, r[1] + offset)
      .then((x) => alive && (ranged = x))
      .catch((e) => alive && (rangeErr = String(e)))
      .finally(() => alive && (rangeBusy = false));
    return () => (alive = false);
  });

  // ---- chart ----
  const bars = $derived(whole.apm_per_min.map((v, i) => ({ i, v })));
  let width = $state(600);
  const height = 140;
  const pad = { l: 40, r: 10, t: 10, b: 22 };
  const yStep = $derived.by(() => {
    const max = Math.max(1, ...bars.map((b) => b.v ?? 0));
    const raw = max / 3;
    const mag = Math.pow(10, Math.floor(Math.log10(raw)));
    const n = raw / mag;
    return (n <= 1 ? 1 : n <= 2 ? 2 : n <= 5 ? 5 : 10) * mag;
  });
  const y1 = $derived(Math.ceil(Math.max(1, ...bars.map((b) => b.v ?? 0)) / yStep) * yStep);
  const yTicks = $derived(Array.from({ length: Math.round(y1 / yStep) + 1 }, (_, i) => i * yStep));
  const bw = $derived((width - pad.l - pad.r) / Math.max(1, bars.length));
  const bx = (i: number) => pad.l + i * bw;
  const by = (v: number) => pad.t + (1 - v / y1) * (height - pad.t - pad.b);
  const xEvery = $derived(bars.length > 40 ? 10 : 5);

  let hover = $state<number | null>(null);
  let drag: { a: number; b: number } | null = $state(null);
  function minuteAt(e: PointerEvent) {
    const r = (e.currentTarget as SVGElement).getBoundingClientRect();
    return Math.max(0, Math.min(bars.length - 1, Math.floor((e.clientX - r.left - pad.l) / bw)));
  }
  function down(e: PointerEvent) {
    if (!bars.length) return;
    (e.currentTarget as SVGElement).setPointerCapture(e.pointerId);
    const i = minuteAt(e);
    drag = { a: i, b: i };
  }
  function move(e: PointerEvent) {
    hover = minuteAt(e);
    if (drag) drag = { ...drag, b: hover };
  }
  function up() {
    if (!drag) return;
    const a = Math.min(drag.a, drag.b);
    const b = Math.max(drag.a, drag.b);
    drag = null;
    if (!canRange) return;
    range = [a * 60, (b + 1) * 60];
  }
  const sel = $derived.by(() => {
    if (drag) return [Math.min(drag.a, drag.b), Math.max(drag.a, drag.b)] as [number, number];
    if (range) return [Math.floor(range[0] / 60), Math.ceil(range[1] / 60) - 1] as [number, number];
    return null;
  });

  const pct = (v: number | null | undefined) => (v == null ? "–" : `${Math.round(v * 100)}%`);
  const dur = (s: number) => (s >= 60 ? `${Math.floor(s / 60)}m ${Math.round(s % 60)}s` : `${s.toFixed(0)} s`);
</script>

<div class="mech card" data-testid="mechanics">
  <div class="head">
    <h3>Mechanics</h3>
    <span class="muted scope">
      {#if range}
        {clock(range[0])}–{clock(range[1])}{rangeBusy ? " · computing…" : ""}{rangeErr ? ` · ${rangeErr}` : ""}
        <button class="btn small ghost" onclick={() => (range = null)}>Whole game</button>
      {:else}
        Whole game · {canRange ? "drag across the chart to see a part of it" : "the recording was removed, so ranges aren't available"}
      {/if}
    </span>
  </div>
  <div class="tiles">
    <div class="tile"><span class="label">APM</span><span class="big">{Math.round(m.apm)}</span><span class="sub">{m.clicks} clicks + {m.key_presses} keys</span></div>
    <div class="tile"><span class="label">Right-clicks</span><span class="big">{m.right_click_hz.toFixed(2)}<small>&nbsp;/s</small></span><span class="sub">{m.right_clicks} in total</span></div>
    <div class="tile"><span class="label">Cursor distance</span><span class="big">{m.cursor_distance.toFixed(0)}<small>&nbsp;screens</small></span><span class="sub">screen widths travelled</span></div>
    <div class="tile"><span class="label">Path efficiency</span><span class="big">{pct(m.path_efficiency)}</span><span class="sub">straight line ÷ actual path between clicks</span></div>
    <div class="tile"><span class="label">Idle time</span><span class="big">{dur(m.idle_secs)}</span><span class="sub">no input for over 1 s ({m.focused_secs > 0 ? Math.round((m.idle_secs / m.focused_secs) * 100) : 0}% of play)</span></div>
  </div>

  <div class="chartbox" bind:clientWidth={width}>
    <div class="ctitle">APM per minute</div>
    {#if bars.length}
      <svg
        {width}
        {height}
        role="img"
        aria-label="Actions per minute for each minute of the game"
        onpointerdown={down}
        onpointermove={move}
        onpointerup={up}
        onpointerleave={() => (hover = null)}
        style="touch-action:none"
      >
        {#each yTicks as t}
          <line x1={pad.l} x2={width - pad.r} y1={by(t)} y2={by(t)} class="grid" />
          <text x={pad.l - 8} y={by(t)} class="tick" dominant-baseline="middle" text-anchor="end">{t}</text>
        {/each}
        {#if sel}
          <rect x={bx(sel[0])} y={pad.t} width={(sel[1] - sel[0] + 1) * bw} height={height - pad.t - pad.b} class="selrect" />
        {/if}
        {#each bars as b}
          {#if b.v != null && b.v > 0}
            {@const h = Math.max(1, by(0) - by(b.v))}
            {@const w = Math.max(1, bw - 2)}
            {@const r = Math.min(4, w / 2, h)}
            <path
              class="bar"
              class:dim={sel && (b.i < sel[0] || b.i > sel[1])}
              d="M{bx(b.i) + 1},{by(0)} v{-(h - r)} q0,{-r} {r},{-r} h{w - 2 * r} q{r},0 {r},{r} v{h - r} z"
            />
          {/if}
          {#if b.i % xEvery === 0}
            <text x={bx(b.i) + bw / 2} y={height - 6} class="tick" text-anchor="middle">{b.i}:00</text>
          {/if}
        {/each}
        {#if hover != null}
          <rect x={bx(hover)} y={pad.t} width={bw} height={height - pad.t - pad.b} class="hov" />
        {/if}
      </svg>
      {#if hover != null}
        {@const b = bars[hover]}
        <div class="tip" style="left:{Math.min(width - 120, Math.max(0, bx(hover) + bw / 2 - 60))}px">
          <span class="muted">{b.i}:00–{b.i + 1}:00</span> <strong>{b.v == null ? "not in game" : `${Math.round(b.v)} APM`}</strong>
        </div>
      {/if}
    {:else}
      <div class="none muted">Not enough data</div>
    {/if}
  </div>
</div>

<style>
  .mech {
    padding: 16px;
    margin-top: 16px;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 12px;
    flex-wrap: wrap;
    margin-bottom: 12px;
  }
  .scope {
    font-size: 13px;
    display: inline-flex;
    align-items: center;
    gap: 8px;
  }
  .tiles {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(170px, 1fr));
    gap: 10px;
  }
  .tile {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 12px;
    border-radius: 10px;
    background: var(--surface-2);
  }
  .label {
    font-size: 12px;
    color: var(--muted);
  }
  .big {
    font-size: 22px;
    font-weight: 700;
    font-variant-numeric: tabular-nums;
  }
  .big small {
    font-size: 13px;
    font-weight: 600;
    color: var(--text-2);
  }
  .sub {
    font-size: 12px;
    color: var(--text-2);
  }
  .chartbox {
    position: relative;
    margin-top: 16px;
  }
  .ctitle {
    font-size: 13px;
    font-weight: 600;
    margin-bottom: 6px;
  }
  svg {
    display: block;
    cursor: crosshair;
    user-select: none;
  }
  .grid {
    stroke: var(--border);
  }
  .tick {
    fill: var(--muted);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
  }
  .bar {
    fill: var(--accent);
  }
  .bar.dim {
    opacity: 0.35;
  }
  .selrect {
    fill: color-mix(in srgb, var(--accent) 14%, transparent);
  }
  .hov {
    fill: color-mix(in srgb, var(--text) 6%, transparent);
    pointer-events: none;
  }
  .tip {
    position: absolute;
    top: 14px;
    width: 120px;
    text-align: center;
    font-size: 12px;
    background: var(--popover);
    border: 1px solid var(--border-2);
    border-radius: 6px;
    padding: 2px 6px;
    pointer-events: none;
    font-variant-numeric: tabular-nums;
  }
  .none {
    height: 100px;
    display: grid;
    place-items: center;
  }
</style>
