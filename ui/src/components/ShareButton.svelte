<script lang="ts">
  // Share: puts the clip (with "Fit for Discord" on, a copy under Discord's 19.5 MB) on the
  // clipboard as a file, ready to paste in Discord.
  import { api } from "../lib/api";
  import { toast } from "../lib/store.svelte";
  import { bytes } from "../lib/format";
  import Icon from "./Icon.svelte";

  let { id, file, disabled = false, iconSize = 14, extraClass = "ghost" }: { id: string; file: string; disabled?: boolean; iconSize?: number; extraClass?: string } = $props();
  let busy = $state(false);

  async function share() {
    if (busy) return;
    busy = true;
    try {
      const r = await api.shareClip(id, file);
      const what = `${bytes(r.bytes)}, ${r.height}p ${r.fps} fps`;
      if (r.copied) toast(`Copied (${what}). Paste it in Discord with Ctrl+V.`, "ok", 6000);
      else {
        toast(`Ready (${what}), but it couldn't be put on the clipboard: here it is in its folder.`, "warn", 7000);
        api.reveal(r.path).catch(() => {});
      }
    } catch (e) {
      toast(String(e), "error", 7000);
    } finally {
      busy = false;
    }
  }
</script>

<button class="btn small {extraClass} sharebtn" onclick={share} disabled={disabled || busy} aria-busy={busy} title="Copy this clip so you can paste it in Discord">
  <Icon name="share" size={iconSize} />{busy ? "Preparing…" : "Share"}
</button>
