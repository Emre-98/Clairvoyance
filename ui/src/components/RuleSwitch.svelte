<script lang="ts">
  // Record full game / Clips only / Off, as a compact segmented control.
  import type { ModeRule } from "../lib/types";

  let { value, onchange, disabled = false, mixed = false, label = "Recording" }: { value: ModeRule | null; onchange: (r: ModeRule) => void; disabled?: boolean; mixed?: boolean; label?: string } = $props();
  const opts: { v: ModeRule; label: string; title: string }[] = [
    { v: "record", label: "Record", title: "Record the full game (events, clips, full video)" },
    { v: "clips_only", label: "Clips only", title: "Events + hotkey and event clips from the replay buffer, no full video" },
    { v: "off", label: "Off", title: "Don't record this mode" },
  ];
</script>

<div class="rs" class:mixed role="radiogroup" aria-label={label}>
  {#each opts as o}
    <button class="o {o.v}" class:on={!mixed && value === o.v} role="radio" aria-checked={!mixed && value === o.v} title={o.title} {disabled} onclick={() => onchange(o.v)}>{o.label}</button>
  {/each}
</div>

<style>
  .rs {
    display: inline-flex;
    background: var(--bg-2);
    border: 1px solid var(--border-2);
    border-radius: 8px;
    padding: 2px;
    flex: none;
  }
  .o {
    border: none;
    background: transparent;
    color: var(--muted);
    padding: 4px 10px;
    border-radius: 6px;
    font-size: 12px;
    font-weight: 600;
    white-space: nowrap;
    transition: background 0.15s var(--ease), color 0.15s var(--ease);
  }
  .o:hover:not(:disabled) {
    color: var(--text);
  }
  .o.on {
    background: var(--surface-3);
    color: var(--text);
  }
  .o.record.on {
    background: color-mix(in srgb, var(--ok) 18%, var(--surface));
    color: var(--ok-text);
  }
  .o.clips_only.on {
    background: color-mix(in srgb, var(--accent) 16%, var(--surface));
    color: var(--accent-text);
  }
  .o.off.on {
    background: var(--danger-soft);
    color: var(--danger-text);
  }
  .mixed {
    border-style: dashed;
  }
</style>
