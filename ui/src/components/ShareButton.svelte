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
      if (r.copied) toast(`Copied! Paste it in Discord with Ctrl+V (${what}).`, "ok", 6000);
      // Not on the clipboard: the file can still be dragged into Discord from its folder.
      else toast(`Couldn't copy to the clipboard (${what}). Drag the file into Discord instead.`, "error", 10000, { label: "Show file", run: () => api.reveal(r.path).catch((e) => toast(String(e), "error")) });
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
