<script lang="ts">
  // "Update available: vX.Y.Z" card in the sidebar, with the release notes and "Update now".
  import { app, toast } from "../lib/store.svelte";
  import { api } from "../lib/api";
  import { bytes } from "../lib/format";
  import Icon from "./Icon.svelte";

  let open = $state(false);
  let dismissedFor = $state<string | null>(null);
  const u = $derived(app.update);
  const busy = $derived(u?.state === "downloading" || u?.state === "installing");
  const show = $derived(!!u && (busy || (u.state === "available" && dismissedFor !== u.version)));
  const pct = $derived(u?.total ? Math.min(100, (u.downloaded / u.total) * 100) : 0);
  const inGame = $derived(app.status?.state !== "idle");

  async function updateNow() {
    try {
      await api.updateInstall();
    } catch (e) {
      toast(String(e), "error", 8000);
    }
  }
</script>

{#if show && u}
  <div class="upd fade-in" class:busy>
    <div class="row head">
      <Icon name="download" size={15} />
      <strong>{busy ? (u.state === "installing" ? "Installing…" : "Downloading update") : `Update available: v${u.version}`}</strong>
    </div>
    {#if u.state === "downloading"}
      <div class="bar"><span style="transform:scaleX({pct / 100})"></span></div>
      <div class="sub">{bytes(u.downloaded)}{u.total ? ` of ${bytes(u.total)}` : ""}</div>
    {:else if u.state === "installing"}
      <div class="sub">Clairvoyance restarts in a moment.</div>
    {:else}
      {#if open && u.notes}<div class="notes selectable">{u.notes}</div>{/if}
      <div class="row btns">
        <button class="btn small primary" onclick={updateNow} disabled={inGame} title={inGame ? "Available after the game" : ""}>Update now</button>
        {#if u.notes}<button class="btn small ghost" onclick={() => (open = !open)}>{open ? "Hide" : "What's new"}</button>{/if}
        <button class="btn small ghost icon" aria-label="Later" title="Later" onclick={() => (dismissedFor = u.version ?? null)}><Icon name="x" size={13} /></button>
      </div>
    {/if}
  </div>
{/if}

<style>
  .upd {
    border: 1px solid color-mix(in srgb, var(--accent) 40%, var(--border));
    background: color-mix(in srgb, var(--accent) 7%, var(--surface));
    border-radius: 10px;
    padding: 10px 11px;
    font-size: 12.5px;
  }
  .head {
    gap: 7px;
    color: var(--accent-text);
  }
  .head strong {
    color: var(--text);
  }
  .btns {
    margin-top: 9px;
    gap: 4px;
  }
  .notes {
    margin-top: 8px;
    max-height: 160px;
    overflow-y: auto;
    white-space: pre-wrap;
    color: var(--text-2);
    line-height: 1.45;
  }
  .sub {
    color: var(--muted);
    margin-top: 6px;
    font-variant-numeric: tabular-nums;
  }
  .bar {
    margin-top: 8px;
    height: 5px;
    border-radius: 3px;
    background: var(--surface-3);
    overflow: hidden;
  }
  .bar span {
    display: block;
    height: 100%;
    background: var(--accent-grad);
    transform-origin: left;
    transition: transform 0.15s linear;
  }
</style>
