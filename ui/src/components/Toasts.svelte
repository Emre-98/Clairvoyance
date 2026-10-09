<script lang="ts">
  import { app } from "../lib/store.svelte";
  import Icon from "./Icon.svelte";
  const icon = { info: "info", ok: "check", warn: "warn", error: "warn" } as const;
</script>

<div class="toasts" aria-live="polite">
  {#each app.toasts as t (t.id)}
    <div class="toast {t.level} fade-in"><Icon name={icon[t.level]} size={16} /><span>{t.text}</span>{#if t.action}<button class="btn small tact" onclick={t.action.run}>{t.action.label}</button>{/if}</div>
  {/each}
</div>

<style>
  .toasts {
    position: fixed;
    right: 20px;
    bottom: 20px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    z-index: 100;
    max-width: 420px;
  }
  .toast {
    display: flex;
    gap: 10px;
    align-items: flex-start;
    padding: 11px 14px;
    border-radius: 10px;
    background: var(--popover);
    border: 1px solid var(--border-2);
    box-shadow: var(--shadow);
    font-size: 13px;
    line-height: 1.4;
    user-select: text;
  }
  .tact {
    flex: none;
    margin: -3px 0 -3px 4px;
  }
  .toast :global(svg) {
    flex: none;
    margin-top: 1px;
  }
  .ok :global(svg) {
    color: var(--ok);
  }
  .warn :global(svg) {
    color: var(--warn);
  }
  .error {
    border-color: color-mix(in srgb, var(--danger) 45%, transparent);
  }
  .error :global(svg) {
    color: var(--danger);
  }
  .info :global(svg) {
    color: var(--accent);
  }
</style>
