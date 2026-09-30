<script lang="ts">
  import { windowAction } from "../lib/api";
</script>

<header class="titlebar" data-tauri-drag-region>
  <div class="brand" data-tauri-drag-region>
    <img src="/logo.svg" alt="" width="18" height="18" data-tauri-drag-region />
    <span data-tauri-drag-region>Clairvoyance</span>
  </div>
  <div class="controls">
    <button aria-label="Minimize" onclick={() => windowAction("minimize")}>
      <svg width="10" height="10" viewBox="0 0 10 10"><path d="M1 5h8" stroke="currentColor" stroke-width="1.2" /></svg>
    </button>
    <button aria-label="Maximize" onclick={() => windowAction("maximize")}>
      <svg width="10" height="10" viewBox="0 0 10 10"><rect x="1.5" y="1.5" width="7" height="7" fill="none" stroke="currentColor" stroke-width="1.2" /></svg>
    </button>
    <button class="close" aria-label="Close to tray" title="Close (keeps recording in the tray)" onclick={() => windowAction("close")}>
      <svg width="10" height="10" viewBox="0 0 10 10"><path d="M1.5 1.5l7 7M8.5 1.5l-7 7" stroke="currentColor" stroke-width="1.2" /></svg>
    </button>
  </div>
</header>

<style>
  .titlebar {
    height: 34px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    background: var(--bg);
    border-bottom: 1px solid var(--border);
    flex: none;
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 8px;
    padding-left: 14px;
    font-size: 12px;
    font-weight: 600;
    color: var(--text-2);
    letter-spacing: 0.02em;
    height: 100%;
    flex: 1;
  }
  .controls {
    display: flex;
    height: 100%;
  }
  .controls button {
    width: 46px;
    height: 100%;
    border: none;
    background: transparent;
    color: var(--text-2);
    display: grid;
    place-items: center;
  }
  .controls button:hover {
    background: var(--surface-2);
    color: var(--text);
  }
  .controls .close:hover {
    background: var(--win-close);
    color: var(--on-media);
  }
</style>
