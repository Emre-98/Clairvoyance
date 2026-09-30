<script lang="ts">
  import { api } from "../lib/api";
  import { app, reloadSettings, toast } from "../lib/store.svelte";
  import type { ObsInfo } from "../lib/types";
  import Icon from "./Icon.svelte";

  let { compact = false, onconnected }: { compact?: boolean; onconnected?: () => void } = $props();

  let info = $state<ObsInfo | null>(null);
  let testing = $state(false);
  let connected = $state<string | null>(null);
  let testError = $state<string | null>(null);
  let enabling = $state(false);

  async function refresh() {
    info = await api.obsDetect();
  }
  $effect(() => {
    refresh();
    const t = setInterval(refresh, 4000);
    return () => clearInterval(t);
  });

  async function enable() {
    enabling = true;
    try {
      await api.obsEnableWebsocket();
      await reloadSettings();
      toast("OBS WebSocket turned on. Starting OBS to test it…", "ok");
      await api.obsLaunch();
      setTimeout(test, 6000);
    } catch (e) {
      toast(String(e), "error", 8000);
    } finally {
      enabling = false;
      refresh();
    }
  }

  async function test() {
    testing = true;
    testError = null;
    try {
      connected = await api.obsTest();
      onconnected?.();
    } catch (e) {
      connected = null;
      testError = String(e);
    } finally {
      testing = false;
    }
  }

  const step = (ok: boolean) => (ok ? "ok" : "todo");
</script>

<div class="obs" class:compact>
  <div class="step {step(!!info?.installed)}">
    <span class="num">{#if info?.installed}<Icon name="check" size={14} stroke={3} />{:else}1{/if}</span>
    <div class="body">
      <strong>Install OBS Studio</strong>
      {#if info?.installed}
        <span class="muted path" title={info.exe_path ?? ""}>Found: {info.exe_path}</span>
      {:else}
        <span class="muted">Free, open-source recorder. GameRecorder controls it for you; you don't need to learn it.</span>
        <div class="row acts">
          <button class="btn primary small" onclick={() => api.openUrl("https://obsproject.com/download")}><Icon name="external" size={13} />Download OBS</button>
          <span class="muted">Install it, then come back. This page updates by itself.</span>
        </div>
      {/if}
    </div>
  </div>

  <div class="step {step(!!info?.websocket_enabled && !!app.settings?.obs.password)}">
    <span class="num">{#if info?.websocket_enabled && app.settings?.obs.password}<Icon name="check" size={14} stroke={3} />{:else}2{/if}</span>
    <div class="body">
      <strong>Let GameRecorder talk to OBS</strong>
      {#if info?.websocket_enabled && app.settings?.obs.password}
        <span class="muted">OBS WebSocket is on (port {info.websocket_port}, password saved).</span>
      {:else}
        <span class="muted">Turns on OBS's built-in WebSocket server with a random password. {info?.running ? "Close OBS first." : ""}</span>
        <div class="row acts">
          <button class="btn primary small" onclick={enable} disabled={!info?.installed || info?.running || enabling}>{enabling ? "Setting up…" : "Turn it on"}</button>
          {#if info?.running}<span class="warn">OBS is open: close it (File > Exit), then click again.</span>{/if}
        </div>
      {/if}
    </div>
  </div>

  <div class="step {step(!!connected)}">
    <span class="num">{#if connected}<Icon name="check" size={14} stroke={3} />{:else}3{/if}</span>
    <div class="body">
      <strong>Test the connection</strong>
      {#if connected}
        <span class="ok">Connected to OBS {connected}.</span>
      {:else}
        <span class="muted">GameRecorder starts OBS minimized when a game begins, so it doesn't need to stay open.</span>
      {/if}
      {#if testError}<span class="warn">{testError}</span>{/if}
      <div class="row acts">
        <button class="btn small" onclick={() => api.obsLaunch().then(() => toast("Starting OBS…")).catch((e) => toast(String(e), "error"))} disabled={!info?.installed || info?.running}>Start OBS</button>
        <button class="btn small" onclick={test} disabled={testing}>{testing ? "Testing…" : "Test connection"}</button>
      </div>
    </div>
  </div>

  {#if app.info?.gpu}
    <div class="gpu">
      <Icon name="cpu" size={15} />
      <span>Graphics card: <strong>{app.info.gpu.name}</strong>. Recording uses its hardware encoder ({app.info.gpu.encoder === "nvenc" ? "NVENC" : app.info.gpu.encoder === "amd" ? "AMD AMF" : "Intel Quick Sync"}), never the CPU.</span>
    </div>
  {/if}
</div>

<style>
  .obs {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .step {
    display: flex;
    gap: 14px;
    padding: 12px 4px;
  }
  .num {
    width: 26px;
    height: 26px;
    flex: none;
    border-radius: 50%;
    display: grid;
    place-items: center;
    font-weight: 700;
    font-size: 13px;
    background: var(--surface-3);
    color: var(--text-2);
  }
  .step.ok .num {
    background: rgba(61, 220, 132, 0.18);
    color: var(--ok);
  }
  .body {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }
  .path {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 560px;
  }
  .acts {
    margin-top: 6px;
    flex-wrap: wrap;
  }
  .warn {
    color: var(--warn);
    font-size: 13px;
  }
  .ok {
    color: var(--ok);
  }
  .gpu {
    display: flex;
    gap: 10px;
    align-items: center;
    color: var(--text-2);
    font-size: 13px;
    padding: 10px 12px;
    background: var(--bg-2);
    border-radius: var(--radius-sm);
    margin-top: 6px;
  }
</style>
