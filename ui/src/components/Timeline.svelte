<script lang="ts">
  // The replay timeline: event markers above a seek track, zoomable from the whole game down to
  // single frames (Ctrl+wheel around the cursor, the zoom controls in the player, Shift+wheel or
  // drag to pan), with a time ruler and one tick per frame when zoomed in.
  //
  // Speed: the markers are plain buttons created once per event list; zooming and panning only
  // move them (one transform per marker, written in a single pass per animation frame), and the
  // ruler, frame ticks and marker ticks are drawn on one canvas. Nothing here reruns per marker
  // through Svelte while zooming.
  import { untrack } from "svelte";
  import type { GameEvent } from "../lib/types";
  import { ICONS, KIND } from "../lib/eventmeta";
  import { clock } from "../lib/format";
  import Icon from "./Icon.svelte";
  import { clampView, follow, frameIndexAt, fullView, isZoomed, lanes as layoutLanes, minSpan, pan, preciseClock, ruler, spanAt, tToX, xToT, zoomAround, zoomLevel, type View } from "../lib/timelineview";

  let {
    events,
    offset,
    duration,
    current,
    clips = [],
    range = $bindable(null),
    frames = null,
    compact = false,
    playing = false,
    level = $bindable(0),
    zoomed = $bindable(false),
    onseek,
    onmarker,
  }: {
    events: GameEvent[];
    offset: number;
    duration: number;
    current: number;
    clips?: { start: number; end: number; title: string }[];
    range?: [number, number] | null;
    /** Start time of every video frame (from the recording's index); null = unknown. */
    frames?: Float64Array | null;
    /** Fullscreen panel: smaller markers, at most 2 lanes. */
    compact?: boolean;
    playing?: boolean;
    /** Zoom slider position 0 (whole game) .. 1 (a few frames). */
    level?: number;
    zoomed?: boolean;
    onseek: (t: number) => void;
    onmarker: (e: GameEvent, pos: number) => void;
  } = $props();

  let width = $state(800);
  let track: HTMLDivElement;
  let layer: HTMLDivElement;
  let root: HTMLDivElement;
  let mkcv: HTMLCanvasElement;
  let canvas: HTMLCanvasElement;
  let hover = $state<{ i: number; x: number; lane: number } | null>(null);
  let hoverX = $state<number | null>(null);

  const MARK = $derived(compact ? 15 : 22);
  const LANE_H = $derived(compact ? 16 : 26);
  const MAX_LANES = $derived(compact ? 2 : 3);

  // Markers in time order (positions in video seconds).
  const list = $derived(
    events
      .map((e) => ({ e, pos: e.game_time + offset }))
      .filter((p) => p.pos >= 0 && p.pos <= duration + 1)
      .sort((a, b) => a.pos - b.pos),
  );
  const posArr = $derived(Float64Array.from(list, (p) => p.pos));

  let view = $state<View>({ start: 0, span: 1 });
  const minS = $derived(minSpan(duration, frames));
  // The whole game until the user zooms; a new duration (metadata loaded) keeps a zoomed view.
  $effect(() => {
    const d = duration;
    const m = minS;
    untrack(() => {
      view = zoomed ? clampView(view, d, m) : fullView(d);
    });
  });
  $effect(() => {
    zoomed = isZoomed(view, duration);
    level = zoomLevel(view, duration, minS);
  });

  // Lanes at the whole-game view decide the height (so it doesn't jump while zooming).
  const fullLanes = $derived.by(() => {
    const w = Math.max(width, 1);
    const xs = Float64Array.from(posArr, (p) => (p / Math.max(duration, 1e-3)) * w);
    return Math.max(1, layoutLanes(xs, w, MARK - 4, MAX_LANES).count);
  });

  const x = (t: number) => tToX(t, view, width);
  /** The track's exact (fractional) width: every time <-> pixel mapping uses this one number. */
  function measure(node: HTMLElement) {
    const ro = new ResizeObserver((es) => {
      const w = es[es.length - 1].contentRect.width;
      if (w > 0 && w !== width) width = w;
    });
    ro.observe(node);
    return { destroy: () => ro.disconnect() };
  }
  const timeAt = (clientX: number) => {
    const r = track.getBoundingClientRect();
    return Math.min(duration, Math.max(0, xToT(clientX - r.left, view, width)));
  };

  // ---------- public controls (zoom buttons / slider in the player) ----------
  function anchor(): number {
    return current >= view.start && current <= view.start + view.span ? current : view.start + view.span / 2;
  }
  export function zoomBy(f: number, at = anchor()) {
    view = zoomAround(view, f, at, duration, minS);
  }
  export function fit() {
    view = fullView(duration);
  }
  export function setLevel(l: number) {
    const span = spanAt(l, duration, minS);
    view = zoomAround(view, view.span / span, anchor(), duration, minS);
  }
  /** The current view (tests, benchmark). */
  export function getView(): View {
    return { ...view };
  }

  // Keep the playhead in view while playing or stepping (a pan alone never moves it back).
  $effect(() => {
    const t = current;
    void playing;
    untrack(() => {
      if (zoomed && !dragging) view = follow(view, t, duration, minS);
    });
  });

  // ---------- drawing (markers + canvas), once per animation frame ----------
  let colors = new Map<string, string>();
  let themeTick = $state(0);
  function color(css: string): string {
    let c = colors.get(css);
    if (!c) {
      const probe = document.createElement("span");
      probe.style.color = css;
      track.appendChild(probe);
      c = getComputedStyle(probe).color;
      probe.remove();
      colors.set(css, c);
    }
    return c;
  }
  $effect(() => {
    const mo = new MutationObserver(() => {
      colors = new Map();
      sprites = new Map();
      themeTick++;
    });
    mo.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme", "style", "class"] });
    const mq = matchMedia("(prefers-color-scheme: dark)");
    const onMq = () => {
      colors = new Map();
      sprites = new Map();
      themeTick++;
    };
    mq.addEventListener("change", onMq);
    return () => {
      mo.disconnect();
      mq.removeEventListener("change", onMq);
    };
  });

  let laneBuf = new Int8Array(0);
  let xsBuf = new Float64Array(0);
  /** Last draw time (ms) of the markers + canvas, for tests and the benchmark. */
  const drawTimes: number[] = [];
  let raf = 0;
  function schedule() {
    if (!raf) raf = requestAnimationFrame(render);
  }
  $effect(() => {
    void compact;
    colors = new Map();
    sprites = new Map();
  });
  $effect(() => {
    // Everything the drawing depends on.
    void view.start;
    void view.span;
    void width;
    void list;
    void compact;
    void themeTick;
    void frames;
    void offset;
    void fullLanes;
    void hover;
    schedule();
  });
  $effect(() => () => cancelAnimationFrame(raf));

  // Marker sprites (one small pre-drawn image per kind / steal badge / size / theme): drawing
  // 600 markers is 600 image copies. The buttons on top are invisible hit targets (click, hover,
  // keyboard focus, screen readers); they're moved only once the view has settled, so zooming
  // never restyles hundreds of elements per frame.
  let sprites = new Map<string, HTMLCanvasElement>();
  function sprite(kind: string, steal: boolean, big: boolean, dpr: number): HTMLCanvasElement {
    const key = `${kind}|${steal}|${big}|${MARK}|${dpr}`;
    let c = sprites.get(key);
    if (c) return c;
    const m = KIND[kind as keyof typeof KIND] ?? KIND.manual_marker;
    const size = MARK * (big ? 1.25 : 1);
    const pad = 6;
    const S = size + pad * 2;
    c = document.createElement("canvas");
    c.width = Math.ceil(S * dpr);
    c.height = Math.ceil(S * dpr);
    const g = c.getContext("2d")!;
    g.scale(dpr, dpr);
    const cx = S / 2;
    const r = size / 2;
    const k = size / MARK;
    g.beginPath();
    g.arc(cx, cx, r, 0, Math.PI * 2);
    g.fillStyle = color("var(--surface)");
    g.fill();
    g.beginPath();
    g.arc(cx, cx, r - (compact ? 1.5 : 2) * k, 0, Math.PI * 2);
    g.fillStyle = color(m.color);
    g.fill();
    const icon = (compact ? 9 : 12) * k;
    g.save();
    g.translate(cx - icon / 2, cx - icon / 2);
    g.scale(icon / 24, icon / 24);
    g.lineWidth = compact ? 2.6 : 2.4;
    g.lineCap = "round";
    g.lineJoin = "round";
    g.strokeStyle = color("var(--on-ev)");
    g.stroke(new Path2D(ICONS[m.icon] ?? ""));
    g.restore();
    if (steal) {
      g.beginPath();
      g.arc(cx + r - 3 * k, cx - r + 3 * k, 6 * k, 0, Math.PI * 2);
      g.fillStyle = color("var(--surface)");
      g.fill();
      g.beginPath();
      g.arc(cx + r - 3 * k, cx - r + 3 * k, 4 * k, 0, Math.PI * 2);
      g.fillStyle = color("var(--text)");
      g.fill();
    }
    sprites.set(key, c);
    return c;
  }

  let domTimer: ReturnType<typeof setTimeout> | undefined;
  let domSynced = false;
  $effect(() => () => clearTimeout(domTimer));
  /** Moves the invisible marker buttons to the drawn positions. */
  function syncDom() {
    domTimer = undefined;
    if (!layer) return;
    const kids = layer.children;
    const n = Math.min(list.length, kids.length);
    const half = MARK / 2;
    for (let i = 0; i < n; i++) {
      const el = kids[i] as HTMLElement;
      const l = laneBuf[i];
      if (l < 0) {
        if (el.style.display !== "none") el.style.display = "none";
        continue;
      }
      if (el.style.display === "none") el.style.display = "";
      el.style.transform = `translate(${(xsBuf[i] - half).toFixed(1)}px,${-l * LANE_H}px)`;
    }
    domSynced = true;
  }

  function render() {
    raf = 0;
    if (!layer || !canvas || !mkcv) return;
    const t0 = performance.now();
    const w = width;
    const v = view;
    const items = list;
    const n = items.length;
    if (xsBuf.length < n) {
      xsBuf = new Float64Array(n);
      laneBuf = new Int8Array(n);
    }
    for (let i = 0; i < n; i++) xsBuf[i] = tToX(posArr[i], v, w);
    const xs = xsBuf.subarray(0, n);
    const { lane } = layoutLanes(xs, w, MARK - 4, Math.min(MAX_LANES, fullLanes), laneBuf);
    drawMarkers(w, xs, lane, items);
    drawCanvas(w, v, xs, lane, items);
    // The buttons follow once the view stops changing (or at once the first time / when idle).
    clearTimeout(domTimer);
    if (!domSynced || layer.children.length !== n) syncDom();
    else domTimer = setTimeout(syncDom, 120);
    // For the UI tests and the benchmark: the view that was drawn.
    root.dataset.viewStart = String(v.start);
    root.dataset.viewSpan = String(v.span);
    root.dataset.width = String(w);
    const ms = performance.now() - t0;
    drawTimes.push(ms);
    if (drawTimes.length > 240) drawTimes.shift();
    (window as any).__cvTimelineDrawMs = drawTimes;
  }

  const markersH = $derived(fullLanes * LANE_H + (compact ? 1 : 4));
  function drawMarkers(w: number, xs: Float64Array, lane: Int8Array, items: typeof list) {
    const dpr = window.devicePixelRatio || 1;
    // The canvas reaches 12 px past both ends (the timeline's padding) and 8 px above.
    const cw = w + 24;
    const ch = markersH + 8;
    const pw = Math.max(1, Math.round(cw * dpr));
    const ph = Math.max(1, Math.round(ch * dpr));
    if (mkcv.width !== pw) mkcv.width = pw;
    if (mkcv.height !== ph) mkcv.height = ph;
    const g = mkcv.getContext("2d")!;
    g.setTransform(1, 0, 0, 1, 0, 0);
    g.clearRect(0, 0, pw, ph);
    const hv = hover?.i ?? -1;
    for (let i = 0; i < xs.length; i++) {
      const l = lane[i];
      if (l < 0 || i === hv) continue;
      const e = items[i].e;
      const sp = sprite(e.kind, !!e.steal, false, dpr);
      const cx = 12 + xs[i];
      const cy = ch - l * LANE_H - MARK / 2;
      g.drawImage(sp, Math.round((cx - sp.width / dpr / 2) * dpr), Math.round((cy - sp.height / dpr / 2) * dpr));
    }
    if (hv >= 0 && hv < xs.length && lane[hv] >= 0) {
      const e = items[hv].e;
      const sp = sprite(e.kind, !!e.steal, true, dpr);
      const cx = 12 + xs[hv];
      const cy = ch - lane[hv] * LANE_H - MARK / 2;
      g.drawImage(sp, Math.round((cx - sp.width / dpr / 2) * dpr), Math.round((cy - sp.height / dpr / 2) * dpr));
    }
  }

  const trackH = $derived(zoomed ? (compact ? 28 : 34) : compact ? 16 : 22);
  // Rail band inside the track (the ruler sits above it when zoomed).
  const railTop = $derived(trackH - (compact ? 11 : 14));

  function drawCanvas(w: number, v: View, xs: Float64Array, lane: Int8Array, items: typeof list) {
    const dpr = window.devicePixelRatio || 1;
    const h = trackH;
    const cw = Math.max(1, Math.round(w * dpr));
    const ch = Math.max(1, Math.round(h * dpr));
    if (canvas.width !== cw) canvas.width = cw;
    if (canvas.height !== ch) canvas.height = ch;
    const g = canvas.getContext("2d")!;
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    g.clearRect(0, 0, w, h);
    const rt = railTop;
    // Marker ticks on the rail.
    let lastColor = "";
    for (let i = 0; i < xs.length; i++) {
      if (lane[i] < 0) continue;
      const c = color((KIND[items[i].e.kind] ?? KIND.manual_marker).color);
      if (c !== lastColor) {
        g.fillStyle = c;
        lastColor = c;
      }
      g.fillRect(Math.round(xs[i]) - 1, rt - 2, 2, 10);
    }
    if (!isZoomed(v, duration)) return;
    // Ruler: labels on round game-clock times, small ticks between, one tick per frame.
    const r = ruler(v, w, frames, offset);
    const fg = color("var(--text-2)");
    const mu = color("var(--muted)");
    g.font = `600 ${compact ? 9.5 : 10}px system-ui, sans-serif`;
    g.textBaseline = "top";
    g.fillStyle = mu;
    for (const t of r.minor) g.fillRect(Math.round(tToX(t, v, w)), rt - 6, 1, 3);
    const labelBottom = rt - 7;
    let lastLabelEnd = -1e9;
    for (const m of r.major) {
      const px = Math.round(tToX(m.t, v, w));
      g.fillStyle = fg;
      g.fillRect(px, rt - 8, 1, 6);
      const tw = g.measureText(m.label).width;
      if (px + 3 > lastLabelEnd + 6 && px + 3 + tw < w) {
        g.fillText(m.label, px + 3, Math.max(0, labelBottom - 10));
        lastLabelEnd = px + 3 + tw;
      }
    }
    if (frames && r.frameTo >= r.frameFrom) {
      g.fillStyle = mu;
      for (let i = r.frameFrom; i <= r.frameTo; i++) {
        const px = Math.round(tToX(frames[i], v, w));
        g.fillRect(px, rt + 8, 1, 4);
      }
      if (r.frameLabelEvery) {
        g.font = `${compact ? 8.5 : 9}px system-ui, sans-serif`;
        g.fillStyle = mu;
        const first = Math.ceil(r.frameFrom / r.frameLabelEvery) * r.frameLabelEvery;
        // Frame numbers go between the major labels' row and the rail, right of each tick.
        for (let i = first; i <= r.frameTo; i += r.frameLabelEvery) {
          const px = tToX(frames[i], v, w);
          const s = String(i);
          if (px + 2 + g.measureText(s).width < w && px > lastLabelEnd + 2) g.fillText(s, px + 2, Math.max(0, labelBottom - 10));
        }
      }
    }
  }

  // ---------- input ----------
  let dragging = $state<null | "seek" | "start" | "end" | "pan" | "window">(null);
  let panFrom = { x: 0, start: 0 };
  function down(e: PointerEvent, what: "seek" | "start" | "end") {
    if (e.button !== 0) return;
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
    if (dragging === "pan") {
      const dt = ((panFrom.x - e.clientX) / Math.max(width, 1)) * view.span;
      view = clampView({ start: panFrom.start + dt, span: view.span }, duration, minS);
      return;
    }
    if (dragging === "window") {
      const dt = ((e.clientX - panFrom.x) / Math.max(width, 1)) * duration;
      view = clampView({ start: panFrom.start + dt, span: view.span }, duration, minS);
      return;
    }
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
  // Drag the empty part of the marker row to pan (when zoomed).
  function panDown(e: PointerEvent) {
    if (!zoomed || e.button !== 0 || e.target !== e.currentTarget) return;
    e.preventDefault();
    dragging = "pan";
    panFrom = { x: e.clientX, start: view.start };
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }
  // The overview bar under a zoomed timeline: drag the window, or click to center there.
  function overviewDown(e: PointerEvent) {
    if (e.button !== 0) return;
    e.preventDefault();
    const el = e.currentTarget as HTMLElement;
    const r = el.getBoundingClientRect();
    const t = ((e.clientX - r.left) / r.width) * duration;
    if (t < view.start || t > view.start + view.span) view = clampView({ start: t - view.span / 2, span: view.span }, duration, minS);
    dragging = "window";
    panFrom = { x: e.clientX, start: view.start };
    el.setPointerCapture(e.pointerId);
  }

  function wheel(e: WheelEvent) {
    const unit = e.deltaMode === 1 ? 33 : e.deltaMode === 2 ? 400 : 1;
    const dy = e.deltaY * unit;
    const dx = e.deltaX * unit;
    if (e.ctrlKey || e.metaKey) {
      e.preventDefault();
      const r = track.getBoundingClientRect();
      const at = xToT(e.clientX - r.left, view, width);
      zoomBy(Math.exp(-dy * 0.0025), Math.min(duration, Math.max(0, at)));
    } else if (zoomed && (e.shiftKey || Math.abs(dx) > Math.abs(dy))) {
      e.preventDefault();
      const d = Math.abs(dx) > Math.abs(dy) ? dx : dy;
      view = pan(view, (d / Math.max(width, 1)) * view.span, duration, minS);
    }
  }
  function wheelAction(node: HTMLElement) {
    node.addEventListener("wheel", wheel, { passive: false });
    return { destroy: () => node.removeEventListener("wheel", wheel) };
  }

  const hoverTime = $derived(hoverX != null && width ? xToT(hoverX, view, width) : null);
  const hoverLabel = $derived.by(() => {
    if (hoverTime == null) return "";
    if (!zoomed) return clock(hoverTime - offset);
    const f = frames && frames.length ? ` · f${frameIndexAt(frames, hoverTime)}` : "";
    return preciseClock(hoverTime - offset) + f;
  });
  const clampX = (px: number) => Math.min(width + 2, Math.max(-2, px));
  const head = $derived(x(current));
</script>

<div class="timeline" class:compact class:zoomed use:wheelAction data-testid="timeline" bind:this={root}>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="markers" style="height:{markersH}px" onpointerdown={panDown} onpointermove={move} onpointerup={up} class:panning={dragging === "pan"}>
    <canvas class="mkcv" bind:this={mkcv} style="height:{markersH + 8}px" aria-hidden="true"></canvas>
    <div class="mklayer" bind:this={layer}>
      {#each list as p, i (p.e.id)}
        <button
          class="mk"
          class:steal={p.e.steal}
          style="display:none"
          onmouseenter={() => (hover = { i, x: xsBuf[i] ?? x(p.pos), lane: Math.max(0, laneBuf[i] ?? 0) })}
          onmouseleave={() => (hover = null)}
          onclick={() => onmarker(p.e, p.pos)}
          aria-label="{p.e.title} at {clock(p.e.game_time)}"
        >
        </button>
      {/each}
    </div>
    {#if hover && list[hover.i]}
      {@const he = list[hover.i].e}
      {@const m = KIND[he.kind] ?? KIND.manual_marker}
      <div class="tip" style="left:clamp(120px, {hover.x}px, calc(100% - 120px));bottom:{hover.lane * LANE_H + MARK + 8}px">
        <div class="tip-head">
          <span class="tip-ic" style="background:{m.color}"><Icon name={m.icon} size={12} stroke={2.4} /></span>
          <strong>{he.title}</strong>
          {#if he.steal}<span class="stealbadge">STEAL</span>{/if}
        </div>
        <div class="tip-sub">{m.label} · {clock(he.game_time)} game time</div>
        {#if he.details}<div class="tip-det">{he.details}</div>{/if}
        <div class="tip-hint">Click to jump to 5 s before</div>
      </div>
    {/if}
  </div>

  <div
    class="track"
    style="height:{trackH}px;--rail:{railTop}px"
    bind:this={track}
    use:measure
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
    <div class="played" style="width:{Math.max(0, clampX(head))}px"></div>
    {#each clips as c}
      {#if x(c.end) >= 0 && x(c.start) <= width}
        <div class="cliprange" title="Clip: {c.title}" style="left:{clampX(x(c.start))}px;width:{Math.max(0, clampX(x(c.end)) - clampX(x(c.start)))}px"></div>
      {/if}
    {/each}
    {#if offset > 0 && x(0) < width}
      <div class="loading" style="left:{clampX(x(0))}px;width:{Math.max(0, clampX(x(offset)) - clampX(x(0)))}px" title="Loading screen (before the game clock started)"></div>
    {/if}
    {#if range}
      <div class="sel" style="left:{clampX(x(range[0]))}px;width:{Math.max(0, clampX(x(range[1])) - clampX(x(range[0])))}px"></div>
      {#if x(range[0]) >= -5 && x(range[0]) <= width + 5}
        <div class="handle" style="left:{x(range[0])}px" role="slider" tabindex="0" aria-label="Clip start" aria-valuenow={range[0]} onpointerdown={(e) => down(e, "start")} onpointermove={move} onpointerup={up}></div>
      {/if}
      {#if x(range[1]) >= -5 && x(range[1]) <= width + 5}
        <div class="handle" style="left:{x(range[1])}px" role="slider" tabindex="0" aria-label="Clip end" aria-valuenow={range[1]} onpointerdown={(e) => down(e, "end")} onpointermove={move} onpointerup={up}></div>
      {/if}
    {/if}
    <canvas class="tcv" bind:this={canvas} style="height:{trackH}px" aria-hidden="true"></canvas>
    {#if head >= -2 && head <= width + 2}
      <div class="head" style="left:{head}px" data-testid="playhead"></div>
    {/if}
    {#if hoverTime != null && !dragging}
      <div class="hovertime" style="left:{hoverX}px">{hoverLabel}</div>
    {/if}
  </div>
  {#if zoomed}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="overview" onpointerdown={overviewDown} onpointermove={move} onpointerup={up} title="Drag to move along the game" data-testid="timeline-overview">
      <div class="ovhead" style="left:{(current / Math.max(duration, 1e-3)) * 100}%"></div>
      <div class="ovwin" style="left:{(view.start / Math.max(duration, 1e-3)) * 100}%;width:max(6px, {(view.span / Math.max(duration, 1e-3)) * 100}%)"></div>
    </div>
  {/if}
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
  .zoomed .markers {
    cursor: grab;
  }
  .markers.panning {
    cursor: grabbing;
  }
  .mklayer {
    position: absolute;
    inset: 0;
    overflow: hidden;
    /* Room for the hover scale and the steal badge. */
    margin: -6px -12px 0;
    padding: 6px 12px 0;
    pointer-events: none;
  }
  .mkcv {
    position: absolute;
    left: -12px;
    bottom: 0;
    width: calc(100% + 24px);
    pointer-events: none;
  }
  /* Invisible hit targets over the drawn markers (click, hover, focus, screen readers). */
  .mk {
    position: absolute;
    left: 12px;
    bottom: 0;
    width: 22px;
    height: 22px;
    padding: 0;
    border: none;
    border-radius: 50%;
    background: transparent;
    pointer-events: auto;
    z-index: 1;
  }
  .mk:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }
  .compact .markers {
    margin-bottom: 1px;
  }
  .compact .mk {
    width: 15px;
    height: 15px;
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
    color: var(--text);
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
  .tcv {
    position: absolute;
    left: 0;
    top: 0;
    width: 100%;
    pointer-events: none;
    z-index: 2;
  }
  .rail,
  .played,
  .loading,
  .cliprange,
  .sel {
    position: absolute;
    top: var(--rail);
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
    background: repeating-linear-gradient(135deg, color-mix(in srgb, var(--text) 10%, transparent) 0 4px, transparent 4px 8px);
  }
  .cliprange {
    top: calc(var(--rail) + 8px);
    height: 3px;
    background: var(--ev-clip);
    opacity: 0.8;
  }
  .sel {
    top: calc(var(--rail) - 4px);
    height: 14px;
    border-radius: 4px;
    background: color-mix(in srgb, var(--accent) 22%, transparent);
    border: 1px solid var(--accent);
  }
  .handle {
    position: absolute;
    top: calc(var(--rail) - 8px);
    width: 10px;
    height: 22px;
    margin-left: -5px;
    border-radius: 3px;
    background: var(--accent);
    cursor: ew-resize;
    z-index: 4;
  }
  .head {
    position: absolute;
    top: calc(var(--rail) - 6px);
    width: 3px;
    height: 18px;
    margin-left: -1.5px;
    border-radius: 2px;
    background: var(--text);
    box-shadow: 0 0 0 2px var(--surface);
    z-index: 3;
    pointer-events: none;
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
    z-index: 5;
  }
  .overview {
    position: relative;
    height: 6px;
    margin-top: 3px;
    border-radius: 3px;
    background: color-mix(in srgb, var(--text) 10%, transparent);
    cursor: pointer;
    touch-action: none;
  }
  .ovwin {
    position: absolute;
    top: 0;
    bottom: 0;
    border-radius: 3px;
    background: color-mix(in srgb, var(--accent) 55%, transparent);
    border: 1px solid var(--accent);
    box-sizing: border-box;
    cursor: grab;
  }
  .ovhead {
    position: absolute;
    top: -1px;
    bottom: -1px;
    width: 2px;
    margin-left: -1px;
    background: var(--text);
  }
  .compact .overview {
    height: 4px;
    margin-top: 2px;
  }
</style>
