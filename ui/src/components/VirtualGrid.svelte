<script lang="ts" generics="T">
  // A responsive card grid that only renders the rows on screen (plus a few above and below),
  // so a library with hundreds of games scrolls at full frame rate. It scrolls with the page:
  // it listens to its nearest scrolling ancestor.
  import type { Snippet } from "svelte";

  let {
    items,
    key,
    minWidth = 250,
    gap = 16,
    extraHeight = 62,
    aspect = 9 / 16,
    overscan = 3,
    item,
  }: {
    items: T[];
    key: (it: T) => string;
    /** Minimum card width (same rule as CSS auto-fill/minmax). */
    minWidth?: number;
    gap?: number;
    /** Card height below its 16:9 picture. */
    extraHeight?: number;
    /** Picture height / width. */
    aspect?: number;
    /** Extra rows rendered above and below the visible ones. */
    overscan?: number;
    item: Snippet<[T, number]>;
  } = $props();

  let el = $state<HTMLDivElement>();
  let width = $state(0);
  let scrollTop = $state(0);
  let viewH = $state(800);
  let offset = $state(0); // grid top inside the scroller

  const cols = $derived(Math.max(1, Math.floor((width + gap) / (minWidth + gap))));
  const colW = $derived(cols > 0 ? (width - gap * (cols - 1)) / cols : minWidth);
  const rowH = $derived(Math.round(colW * aspect + extraHeight));
  const rows = $derived(Math.ceil(items.length / cols));
  const total = $derived(rows > 0 ? rows * rowH + (rows - 1) * gap : 0);
  const first = $derived(Math.max(0, Math.floor((scrollTop - offset) / (rowH + gap)) - overscan));
  const last = $derived(Math.min(rows - 1, Math.ceil((scrollTop - offset + viewH) / (rowH + gap)) + overscan));
  const visible = $derived.by(() => {
    const out: { it: T; i: number; x: number; y: number }[] = [];
    for (let r = first; r <= last; r++) {
      for (let c = 0; c < cols; c++) {
        const i = r * cols + c;
        if (i >= items.length) break;
        out.push({ it: items[i], i, x: c * (colW + gap), y: r * (rowH + gap) });
      }
    }
    return out;
  });

  function scrollParent(node: HTMLElement): HTMLElement {
    let p = node.parentElement;
    while (p) {
      const oy = getComputedStyle(p).overflowY;
      if (oy === "auto" || oy === "scroll") return p;
      p = p.parentElement;
    }
    return document.documentElement;
  }

  $effect(() => {
    if (!el) return;
    const sc = scrollParent(el);
    let raf = 0;
    const measure = () => {
      raf = 0;
      scrollTop = sc.scrollTop;
      viewH = sc.clientHeight;
      offset = el!.getBoundingClientRect().top - sc.getBoundingClientRect().top + sc.scrollTop;
    };
    // At most one update per frame, and only when the window of visible rows changes.
    const onScroll = () => {
      if (!raf) raf = requestAnimationFrame(measure);
    };
    measure();
    sc.addEventListener("scroll", onScroll, { passive: true });
    const ro = new ResizeObserver(onScroll);
    ro.observe(sc);
    return () => {
      sc.removeEventListener("scroll", onScroll);
      ro.disconnect();
      if (raf) cancelAnimationFrame(raf);
    };
  });
</script>

<div class="vgrid" bind:this={el} bind:clientWidth={width} style="height:{total}px">
  {#each visible as v (key(v.it))}
    <div class="cell" style="width:{colW}px;height:{rowH}px;transform:translate({v.x}px,{v.y}px)">
      {@render item(v.it, v.i)}
    </div>
  {/each}
</div>

<style>
  .vgrid {
    position: relative;
    width: 100%;
    contain: layout;
  }
  .cell {
    position: absolute;
    left: 0;
    top: 0;
    /* Fixed size: the browser can lay out and paint each card on its own. */
    contain: strict;
    will-change: transform;
  }
  .cell > :global(*) {
    width: 100%;
    height: 100%;
  }
</style>
