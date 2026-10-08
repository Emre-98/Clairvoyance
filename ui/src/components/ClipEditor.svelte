<script lang="ts">
  import { api, on } from "../lib/api";
  import { clock, bytes } from "../lib/format";
  import { toast } from "../lib/store.svelte";
  import { Overlay, parse as parseInput, loadOptions } from "../lib/inputoverlay";
  import Icon from "./Icon.svelte";
  import ShareFitToggle from "./ShareFitToggle.svelte";

  let {
    sessionId,
    range = $bindable(),
    current,
    offset,
    duration,
    onclose,
    onpreview,
    inputId = null,
    showUnconfirmed = false,
    heatRange = null,
  }: {
    sessionId: string;
    range: [number, number];
    current: number;
    offset: number;
    duration: number;
    onclose: () => void;
    onpreview: () => void;
    /** The game has an input recording: the clip can carry the input overlay. */
    inputId?: string | null;
    /** The timeline shows unconfirmed presses (their faded bubbles are drawn then, as in the player). */
    showUnconfirmed?: boolean;
    /** The Mechanics range, for the overlay's "selected range" heatmap. */
    heatRange?: [number, number] | null;
  } = $props();

  let title = $state("");
  let precise = $state(false);
  let busy = $state(false);
  let ff = $state<{ available: boolean } | null>(null);
  let dl = $state<{ done: number; total: number } | null>(null);
  /** Burn the input overlay (as set in the player's overlay options) into the clip. */
  let withOverlay = $state(false);
  /** Overlay export progress 0..1 while rendering. */
  let progress = $state<number | null>(null);
  let cancelled = false;
  let job: number | null = null;

  $effect(() => {
    api.ffmpegStatus().then((s) => (ff = s));
  });

  const len = $derived(range[1] - range[0]);

  async function download() {
    const un = await on("ffmpeg-progress", (p) => (dl = p));
    try {
      dl = { done: 0, total: 0 };
      await api.ffmpegDownload();
      ff = { available: true };
      toast("ffmpeg installed", "ok");
    } catch (e) {
      toast(String(e), "error", 9000);
    } finally {
      un();
      dl = null;
    }
  }

  async function exportClip() {
    busy = true;
    try {
      if (withOverlay && inputId) await exportWithOverlay(inputId);
      else await api.exportClip(sessionId, range[0], range[1], title, precise);
      toast("Clip saved", "ok");
      onclose();
    } catch (e) {
      if (!cancelled) toast(String(e), "error", 9000);
    } finally {
      busy = false;
      progress = null;
      job = null;
    }
  }

  function toPng(c: HTMLCanvasElement): Promise<Uint8Array> {
    return new Promise((res, rej) =>
      c.toBlob((b) => (b ? b.arrayBuffer().then((a) => res(new Uint8Array(a)), rej) : rej(new Error("Couldn't draw the overlay"))), "image/png"),
    );
  }

  /** The overlay is drawn with the player's own code, one frame at a time at the moments the app
   *  asks for (the source frame each output frame shows), and burned in by ffmpeg. */
  async function exportWithOverlay(id: string) {
    cancelled = false;
    progress = 0;
    const [buf, actions] = await Promise.all([api.inputLoad(id), api.inputActions(id).catch(() => null)]);
    const ov = new Overlay(parseInput(buf));
    ov.setBubbles(actions);
    const opts = loadOptions();
    const plan = await api.overlayExportBegin(sessionId, range[0], range[1], title);
    job = plan.job;
    const canvas = document.createElement("canvas");
    canvas.width = plan.width;
    canvas.height = plan.height;
    const g = canvas.getContext("2d")!;
    const hr = heatRange ? ([...heatRange] as [number, number]) : null;
    // One frame is written while the next one is drawn.
    let writing: Promise<void> | null = null;
    try {
      for (let k = 0; k < plan.times.length; k++) {
        if (cancelled) throw new Error("cancelled");
        ov.draw(g, plan.width, plan.height, 1, plan.times[k], plan.width, plan.height, opts, hr, showUnconfirmed, "contain", 0);
        const png = await toPng(canvas);
        if (writing) await writing;
        writing = api.overlayExportFrame(plan.job, png);
        progress = (k + 1) / plan.times.length;
      }
      await writing;
      if (cancelled) throw new Error("cancelled");
      await api.overlayExportEnd(plan.job);
    } catch (e) {
      await writing?.catch(() => {});
      await api.overlayExportCancel(plan.job).catch(() => {});
      throw e;
    }
  }

  function cancelExport() {
    cancelled = true;
  }

  const opts = loadOptions();
  const drawsNothing = !(opts.trail || opts.clicks || opts.dot || opts.keys || opts.heat || opts.bubbles);

  let root: HTMLDivElement;
  /** Scrolls only as far as needed for the whole editor to be visible (not at all if it is). */
  export function reveal() {
    const reduce = window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;
    root?.scrollIntoView({ block: "nearest", behavior: reduce ? "auto" : "smooth" });
  }

  function setStart() {
    range = [Math.min(current, range[1] - 1), range[1]];
  }
  function setEnd() {
    range = [range[0], Math.min(duration, Math.max(current, range[0] + 1))];
  }
</script>

<div class="editor card fade-in" bind:this={root}>
  <div class="row head">
    <Icon name="scissors" size={17} />
    <h3>Clip editor</h3>
    <span class="muted">Drag the handles on the timeline, or use the buttons.</span>
    <div class="spacer"></div>
    <button class="btn small icon ghost" onclick={onclose} aria-label="Close editor"><Icon name="x" size={15} /></button>
  </div>
  <div class="row wrap">
    <div class="rng">
      <button class="btn small" onclick={setStart} title="Set start to the current position">Start {clock(range[0] - offset)}</button>
      <span class="muted">→</span>
      <button class="btn small" onclick={setEnd} title="Set end to the current position">End {clock(range[1] - offset)}</button>
      <span class="pill">{len.toFixed(1)} s</span>
      <button class="btn small ghost" onclick={onpreview}><Icon name="play" size={13} fill />Preview</button>
    </div>
    <input class="input title" placeholder="Clip name (optional)" bind:value={title} maxlength="80" />
    <label class="chk" title="Re-encodes on your graphics card so the clip starts exactly at the handle (a few seconds slower). Off: instant, but may start up to ~2 s early.">
      <input type="checkbox" checked={precise || withOverlay} disabled={withOverlay} onchange={(e) => (precise = e.currentTarget.checked)} data-testid="export-exact" /> Start exactly at the handle
    </label>
    {#if inputId}
      <label
        class="chk"
        title={drawsNothing
          ? "Your overlay options draw nothing: turn on the trail, clicks or ability bubbles in the player's overlay options first."
          : "Your cursor trail, clicks, keys and ability bubbles burned into the clip, exactly as the player's overlay options show them. Re-encodes the clip."}
      >
        <input type="checkbox" bind:checked={withOverlay} disabled={busy} data-testid="export-overlay" /> Input overlay
      </label>
    {/if}
    <!-- Not used to make the clip: it's what the Share button on the saved clip does. -->
    <ShareFitToggle variant="chk" label="Share: fit for Discord" title="When you share this clip, a copy under 18 MB is made for Discord. Your saved clip stays full quality." />
    {#if ff && !ff.available}
      <button class="btn primary" onclick={download} disabled={dl != null}>
        <Icon name="download" size={15} />
        {#if dl}Downloading {dl.total ? Math.round((dl.done / dl.total) * 100) + "%" : bytes(dl.done)}{:else}Get ffmpeg to export{/if}
      </button>
    {:else}
      <button class="btn primary" onclick={exportClip} disabled={busy || !ff}>
        {#if progress != null}Rendering {Math.round(progress * 100)}%{:else}{busy ? "Saving…" : "Save clip"}{/if}
      </button>
      {#if progress != null}
        <button class="btn small ghost" onclick={cancelExport} data-testid="export-cancel">Cancel</button>
      {/if}
    {/if}
  </div>
  {#if withOverlay && drawsNothing}
    <p class="muted note">Your overlay options draw nothing: turn on the trail, clicks or ability bubbles in the player's overlay options (next to "Input overlay").</p>
  {/if}
  {#if ff && !ff.available}
    <p class="muted note">Clip export uses the free ffmpeg tool (about 100 MB, downloaded once from github.com/BtbN/FFmpeg-Builds).</p>
  {/if}
</div>

<style>
  .editor {
    padding: 14px 16px;
    border-color: color-mix(in srgb, var(--accent) 35%, transparent);
    margin-top: 14px;
  }
  .head {
    margin-bottom: 12px;
    color: var(--accent);
  }
  .head h3 {
    color: var(--text);
  }
  .head .muted {
    font-size: 12.5px;
  }
  .wrap {
    flex-wrap: wrap;
  }
  .rng {
    display: flex;
    align-items: center;
    gap: 8px;
    font-variant-numeric: tabular-nums;
  }
  .title {
    flex: 1;
    min-width: 180px;
  }
  .chk {
    display: flex;
    gap: 6px;
    align-items: center;
    font-size: 13px;
    color: var(--text-2);
  }
  .chk input {
    accent-color: var(--accent);
  }
  .note {
    font-size: 12px;
    margin: 10px 0 0;
  }
</style>
