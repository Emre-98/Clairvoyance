<script lang="ts">
  import { api, on } from "../lib/api";
  import { clock, bytes } from "../lib/format";
  import { toast } from "../lib/store.svelte";
  import Icon from "./Icon.svelte";

  let {
    sessionId,
    range = $bindable(),
    current,
    offset,
    duration,
    onclose,
    onpreview,
  }: {
    sessionId: string;
    range: [number, number];
    current: number;
    offset: number;
    duration: number;
    onclose: () => void;
    onpreview: () => void;
  } = $props();

  let title = $state("");
  let precise = $state(false);
  let busy = $state(false);
  let ff = $state<{ available: boolean } | null>(null);
  let dl = $state<{ done: number; total: number } | null>(null);

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
      await api.exportClip(sessionId, range[0], range[1], title, precise);
      toast("Clip saved", "ok");
      onclose();
    } catch (e) {
      toast(String(e), "error", 9000);
    } finally {
      busy = false;
    }
  }

  function setStart() {
    range = [Math.min(current, range[1] - 1), range[1]];
  }
  function setEnd() {
    range = [range[0], Math.min(duration, Math.max(current, range[0] + 1))];
  }
</script>

<div class="editor card fade-in">
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
    <label class="chk" title="Re-encodes on your GPU so the clip starts exactly at the handle. Off = instant copy (may start up to a couple of seconds early).">
      <input type="checkbox" bind:checked={precise} /> Exact cut
    </label>
    {#if ff && !ff.available}
      <button class="btn primary" onclick={download} disabled={dl != null}>
        <Icon name="download" size={15} />
        {#if dl}Downloading {dl.total ? Math.round((dl.done / dl.total) * 100) + "%" : bytes(dl.done)}{:else}Get ffmpeg to export{/if}
      </button>
    {:else}
      <button class="btn primary" onclick={exportClip} disabled={busy || !ff}>{busy ? "Saving…" : "Save clip"}</button>
    {/if}
  </div>
  {#if ff && !ff.available}
    <p class="muted note">Clip export uses the free ffmpeg tool (about 100 MB, downloaded once from github.com/BtbN/FFmpeg-Builds).</p>
  {/if}
</div>

<style>
  .editor {
    padding: 14px 16px;
    border-color: rgba(46, 230, 197, 0.35);
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
