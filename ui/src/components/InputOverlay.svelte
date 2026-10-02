<script lang="ts">
  // Canvas over the replay video that draws the recorded input (see lib/inputoverlay.ts).
  // Only exists while the overlay is on; the drawing loop runs only then, and redraws only
  // when the time, size or options changed (nothing to do while paused).
  import { Overlay, type OverlayOptions } from "../lib/inputoverlay";

  let {
    overlay,
    video,
    options,
    heatRange = null,
    showUnconfirmed = false,
  }: { overlay: Overlay; video: HTMLVideoElement | undefined; options: OverlayOptions; heatRange?: [number, number] | null; showUnconfirmed?: boolean } = $props();

  let canvas: HTMLCanvasElement;
  let size = { w: 0, h: 0, dpr: 1 };

  $effect(() => {
    const c = canvas;
    const ro = new ResizeObserver(() => {
      const r = c.getBoundingClientRect();
      const dpr = window.devicePixelRatio || 1;
      size = { w: r.width, h: r.height, dpr };
      c.width = Math.max(1, Math.round(r.width * dpr));
      c.height = Math.max(1, Math.round(r.height * dpr));
      last = "";
    });
    ro.observe(c);
    return () => ro.disconnect();
  });

  let last = "";
  // Last few draw times (ms), for the UI tests / Settings numbers.
  const times: number[] = [];
  $effect(() => {
    const ov = overlay;
    const v = video;
    const opts = $state.snapshot(options) as OverlayOptions;
    const hr = heatRange ? ([...heatRange] as [number, number]) : null;
    const unconf = showUnconfirmed;
    last = "";
    if (!v) return;
    const g = canvas.getContext("2d")!;
    let raf = 0;
    const frame = () => {
      raf = requestAnimationFrame(frame);
      const t = v.currentTime;
      const key = `${t}|${size.w}|${size.h}|${v.videoWidth}|${ov.version}`;
      if (key === last) return;
      last = key;
      const t0 = performance.now();
      ov.draw(g, size.w, size.h, size.dpr, t, v.videoWidth, v.videoHeight, opts, hr, unconf);
      times.push(performance.now() - t0);
      if (times.length > 240) times.shift();
      (window as any).__cvOverlayDrawMs = times;
      if (ov.bubbles) {
        (window as any).__cvBubbleStats = { ...ov.bubbles.stats, t };
        (window as any).__cvBubbles = ov.bubbles;
      }
    };
    raf = requestAnimationFrame(frame);
    return () => cancelAnimationFrame(raf);
  });
</script>

<canvas class="input-overlay" bind:this={canvas} aria-hidden="true" data-testid="input-overlay"></canvas>

<style>
  .input-overlay {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    pointer-events: none;
  }
</style>
