<script lang="ts">
  // "Fit for Discord": the saved setting Share follows (same as in Settings > Events & clips).
  // Every tick reads and writes app.settings, so ticking one (clips list, clip editor) updates all.
  import { api } from "../lib/api";
  import { app, toast } from "../lib/store.svelte";

  let {
    label = "Fit for Discord",
    title = "Share makes a copy under 19.5 MB so it fits Discord's free upload limit. Off: the original clip is shared.",
    variant = "head",
  }: { label?: string; title?: string; /** "head": next to a heading; "chk": like the clip editor's options. */ variant?: "head" | "chk" } = $props();

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

<label class="fitdiscord {variant}" {title}>
  <input type="checkbox" checked={on} onchange={(e) => set(e.currentTarget.checked)} />
  {label}
</label>

<style>
  .fitdiscord {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 13px;
    white-space: nowrap;
    cursor: pointer;
  }
  .head {
    font-weight: 600;
    color: var(--muted);
  }
  .chk {
    display: flex;
    color: var(--text-2);
  }
  .chk input {
    accent-color: var(--accent);
  }
</style>
