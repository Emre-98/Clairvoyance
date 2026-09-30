<script lang="ts">
  // Single-series line chart (title names the series, so no legend). Hover shows a crosshair + tooltip.
  let {
    points,
    color = "var(--accent)",
    xFormat = (v: number) => String(v),
    yFormat = (v: number) => String(Math.round(v)),
    height = 150,
    label,
  }: {
    points: { x: number; y: number }[];
    color?: string;
    xFormat?: (v: number) => string;
    yFormat?: (v: number) => string;
    height?: number;
    label: string;
  } = $props();

  let width = $state(400);
  let hover = $state<number | null>(null);
  const pad = { l: 44, r: 16, t: 12, b: 24 };

  function niceStep(range: number, count: number) {
    const raw = range / Math.max(1, count);
    const mag = Math.pow(10, Math.floor(Math.log10(raw || 1)));
    const n = raw / mag;
    return (n <= 1 ? 1 : n <= 2 ? 2 : n <= 5 ? 5 : 10) * mag;
  }

  const xs = $derived(points.map((p) => p.x));
  const x0 = $derived(Math.min(...xs, 0));
  const x1 = $derived(Math.max(...xs, 1));
  const yStep = $derived(niceStep(Math.max(...points.map((p) => p.y), 1), 3));
  const y1 = $derived(Math.ceil(Math.max(...points.map((p) => p.y), 1) / yStep) * yStep);
  const yTicks = $derived(Array.from({ length: Math.round(y1 / yStep) + 1 }, (_, i) => i * yStep));
  const xStep = $derived(niceStep(x1 - x0, 5) >= 60 ? Math.max(60, Math.round(niceStep(x1 - x0, 5) / 60) * 60) : niceStep(x1 - x0, 5));
  const xTicks = $derived(Array.from({ length: Math.floor((x1 - x0) / xStep) + 1 }, (_, i) => x0 + i * xStep));

  const sx = (x: number) => pad.l + ((x - x0) / Math.max(1e-9, x1 - x0)) * (width - pad.l - pad.r);
  const sy = (y: number) => pad.t + (1 - y / Math.max(1e-9, y1)) * (height - pad.t - pad.b);
  const path = $derived(points.map((p, i) => `${i ? "L" : "M"}${sx(p.x).toFixed(1)},${sy(p.y).toFixed(1)}`).join(""));
  const area = $derived(points.length ? `${path}L${sx(points[points.length - 1].x)},${sy(0)}L${sx(points[0].x)},${sy(0)}Z` : "");
  const last = $derived(points[points.length - 1]);

  function move(e: PointerEvent) {
    const r = (e.currentTarget as SVGElement).getBoundingClientRect();
    const x = e.clientX - r.left;
    let best = 0;
    let bd = Infinity;
    points.forEach((p, i) => {
      const d = Math.abs(sx(p.x) - x);
      if (d < bd) {
        bd = d;
        best = i;
      }
    });
    hover = points.length ? best : null;
  }
</script>

<div class="chart" bind:clientWidth={width}>
  {#if points.length < 2}
    <div class="none">Not enough data</div>
  {:else}
    <svg {width} {height} role="img" aria-label={label} onpointermove={move} onpointerleave={() => (hover = null)}>
      {#each yTicks as t}
        <line x1={pad.l} x2={width - pad.r} y1={sy(t)} y2={sy(t)} class="grid" />
        <text x={pad.l - 8} y={sy(t)} class="ytick" dominant-baseline="middle" text-anchor="end">{yFormat(t)}</text>
      {/each}
      {#each xTicks as t}
        <text x={sx(t)} y={height - 6} class="xtick" text-anchor="middle">{xFormat(t)}</text>
      {/each}
      <path d={area} fill={color} opacity="0.1" />
      <path d={path} fill="none" stroke={color} stroke-width="2" stroke-linejoin="round" stroke-linecap="round" />
      <circle cx={sx(last.x)} cy={sy(last.y)} r="4" fill={color} stroke="var(--surface)" stroke-width="2" />
      {#if hover != null}
        {@const p = points[hover]}
        <line x1={sx(p.x)} x2={sx(p.x)} y1={pad.t} y2={height - pad.b} class="cross" />
        <circle cx={sx(p.x)} cy={sy(p.y)} r="5" fill={color} stroke="var(--surface)" stroke-width="2" />
      {/if}
    </svg>
    {#if hover != null}
      {@const p = points[hover]}
      <div class="tip" style="left:{Math.min(width - 110, Math.max(0, sx(p.x) - 55))}px">
        <span class="muted">{xFormat(p.x)}</span> <strong>{yFormat(p.y)}</strong>
      </div>
    {:else}
      <div class="endlabel" style="left:{Math.min(width - 90, sx(last.x) - 70)}px">{yFormat(last.y)}</div>
    {/if}
  {/if}
</div>

<style>
  .chart {
    position: relative;
    width: 100%;
  }
  svg {
    display: block;
    overflow: visible;
  }
  .grid {
    stroke: var(--border);
    stroke-width: 1;
  }
  .cross {
    stroke: var(--muted);
    stroke-width: 1;
  }
  .ytick,
  .xtick {
    fill: var(--muted);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
  }
  .tip,
  .endlabel {
    position: absolute;
    top: -6px;
    width: 110px;
    text-align: center;
    font-size: 12px;
    background: #0a0c12;
    border: 1px solid var(--border-2);
    border-radius: 6px;
    padding: 2px 6px;
    pointer-events: none;
    font-variant-numeric: tabular-nums;
  }
  .endlabel {
    width: 80px;
    background: transparent;
    border: none;
    text-align: right;
    color: var(--text-2);
    font-weight: 600;
  }
  .none {
    height: 120px;
    display: grid;
    place-items: center;
    color: var(--muted);
    font-size: 13px;
  }
</style>
