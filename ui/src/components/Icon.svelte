<script lang="ts">
  import { ICONS } from "../lib/eventmeta";
  let { name, size = 18, fill = false, stroke = 2, class: cls = "" }: { name: string; size?: number; fill?: boolean; stroke?: number; class?: string } = $props();
</script>

<svg
  class={cls}
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill={fill ? "currentColor" : "none"}
  stroke={fill ? "none" : "currentColor"}
  stroke-width={stroke}
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"><path d={ICONS[name] ?? ""} /></svg>
