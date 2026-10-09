<script lang="ts">
  import { api } from "../lib/api";
  import { bytes } from "../lib/format";
  import type { SelfTestResult } from "../lib/types";
  import Icon from "./Icon.svelte";
  import { app } from "../lib/store.svelte";

  let enc = $state<{ gpu: string; encoders: string[] } | null>(null);
  let encErr = $state<string | null>(null);
  let testing = $state(false);
  let result = $state<SelfTestResult | null>(null);
  let testErr = $state<string | null>(null);

  $effect(() => {
    api.builtinEncoders().then((e) => (enc = e)).catch((e) => (encErr = String(e)));
  });

  async function test() {
    testing = true;
    testErr = null;
    result = null;
    try {
      result = await api.recorderSelftest(5);
    } catch (e) {
      testErr = String(e);
    } finally {
      testing = false;
    }
  }
</script>

<div class="br">
  <div class="line">
    <span class="ic ok"><Icon name="check" size={14} stroke={3} /></span>
    <div>
      <strong>Built-in recorder</strong>
      <div class="muted">Captures the game window with Windows' own screen capture (nothing touches the game, safe with Vanguard), encodes on your graphics card and records only the game's sound.</div>
    </div>
  </div>
  {#if enc}
    <div class="line">
      <span class="ic"><Icon name="cpu" size={14} /></span>
      <div>
        <strong>{enc.gpu}</strong>
        {#if enc.encoders.length}
          <div class="muted">Hardware encoder: {enc.encoders[0]}{enc.encoders.length > 1 ? ` (+${enc.encoders.length - 1} more)` : ""}</div>
        {:else}
          <div class="warn">No hardware video encoder found. Update your graphics driver.</div>
        {/if}
      </div>
    </div>
  {:else if encErr}
    <div class="warn">{encErr}</div>
  {/if}
  {#if app.settings?.dev_tools}
  <div class="row test">
    <button class="btn" onclick={test} disabled={testing}>{testing ? "Recording 5 s of your screen…" : "Test the recorder"}</button>
    {#if result}
      <span class="ok-text">
        Works: {result.fps.toFixed(0)} fps, {result.dropped} dropped, {bytes(result.bytes)}, {result.cpu_percent.toFixed(2)}% CPU
      </span>
      <button class="btn small ghost" onclick={() => api.reveal(result!.path)}>Show file</button>
    {/if}
    {#if testErr}<span class="warn">{testErr}</span>{/if}
  </div>
  {/if}
</div>

<style>
  .br {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .line {
    display: flex;
    gap: 12px;
    align-items: flex-start;
  }
  .line .muted {
    font-size: 13px;
    margin-top: 3px;
    line-height: 1.45;
  }
  .ic {
    width: 26px;
    height: 26px;
    flex: none;
    border-radius: 50%;
    display: grid;
    place-items: center;
    background: var(--surface-3);
    color: var(--text-2);
  }
  .ic.ok {
    background: var(--ok-soft);
    color: var(--ok);
  }
  .warn {
    color: var(--warn);
    font-size: 13px;
  }
  .ok-text {
    color: var(--ok);
    font-size: 13px;
  }
  .test {
    flex-wrap: wrap;
    margin-left: 38px;
  }
</style>
