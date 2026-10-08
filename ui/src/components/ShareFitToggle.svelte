<script lang="ts">
  // "Fit for Discord": the saved setting Share follows (same as in Settings > Events & clips).
  import { api } from "../lib/api";
  import { app, toast } from "../lib/store.svelte";

  const on = $derived(app.settings?.share_fit_discord ?? true);

  async function set(v: boolean) {
    if (!app.settings) return;
    app.settings.share_fit_discord = v; // instant; saved below
    try {
      await api.setShareFitDiscord(v);
    } catch (e) {
      app.settings.share_fit_discord = !v;
      toast(String(e), "error");
    }
  }
</script>

<label class="fitdiscord" title="Share makes a copy under 19.5 MB so it fits Discord's free upload limit. Off: the original clip is shared.">
  <input type="checkbox" checked={on} onchange={(e) => set(e.currentTarget.checked)} />
  Fit for Discord
</label>

<style>
  .fitdiscord {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 13px;
    font-weight: 600;
    color: var(--muted);
    white-space: nowrap;
    cursor: pointer;
  }
</style>
