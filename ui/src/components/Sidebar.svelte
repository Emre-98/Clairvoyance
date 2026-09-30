<script lang="ts">
  import Icon from "./Icon.svelte";
  import UpdateNotice from "./UpdateNotice.svelte";
  import { app, go } from "../lib/store.svelte";
  import { api } from "../lib/api";
  import { clock } from "../lib/format";

  const nav = [
    { page: "home", label: "Home", icon: "home" },
    { page: "games", label: "Games", icon: "library" },
    { page: "clips", label: "Clips", icon: "clips" },
    { page: "settings", label: "Settings", icon: "settings" },
  ] as const;

  let perf = $state<{ cpu: number; ram_mb: number } | null>(null);
  $effect(() => {
    if (!app.settings?.show_perf) return;
    let alive = true;
    const tick = async () => {
      if (!alive || document.hidden) return;
      try {
        perf = await api.perfNow();
      } catch {}
    };
    tick();
    const t = setInterval(tick, 3000);
    return () => {
      alive = false;
      clearInterval(t);
    };
  });

  const st = $derived(app.status);
  const active = (p: string) => app.route.page === p || (p === "games" && app.route.page === "game");
</script>

<aside class="sidebar">
  <nav>
    {#each nav as n}
      <button class="nav" class:active={active(n.page)} onclick={() => go({ page: n.page } as any)}>
        <Icon name={n.icon} size={19} />
        <span>{n.label}</span>
      </button>
    {/each}
  </nav>

  <div class="spacer"></div>

  <UpdateNotice />

  {#if st}
    <div class="live" class:rec={st.state === "recording"} class:det={st.state === "detected"}>
      <div class="row">
        <span class="dot"></span>
        <strong>
          {#if st.state === "recording"}Recording{:else if st.state === "detected"}Game detected{:else}Waiting for a game{/if}
        </strong>
      </div>
      {#if st.state !== "idle"}
        <div class="sub">
          {st.game_name}{#if st.game_time != null}&nbsp;· {clock(st.game_time)}{/if}
        </div>
        {#if st.state === "recording"}
          <div class="row btns">
            <button class="btn small" onclick={() => api.saveClipNow()}><Icon name="scissors" size={14} />Clip</button>
            <button class="btn small" onclick={() => api.addMarkerNow()}><Icon name="bookmark" size={14} />Mark</button>
          </div>
        {/if}
      {:else}
        <div class="sub">
          Recorder: <span class="ok">ready</span>
        </div>
      {/if}
    </div>
  {/if}

  {#if app.settings?.show_perf && perf}
    <div class="perf" title="CPU and memory used by Clairvoyance itself (this window adds a little while it's open)">
      <Icon name="cpu" size={14} />
      <span>{perf.cpu.toFixed(1)}% CPU</span>
      <span>{Math.round(perf.ram_mb)} MB</span>
    </div>
  {/if}
</aside>

<style>
  .nav:active {
    transform: scale(0.98);
  }
  .sidebar {
    width: 212px;
    flex: none;
    background: var(--bg-2);
    border-right: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    padding: 14px 10px;
    gap: 10px;
  }
  nav {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .nav {
    transition: background 0.15s var(--ease), color 0.15s var(--ease), transform 0.1s var(--ease);
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 12px;
    border: none;
    border-radius: 9px;
    background: transparent;
    color: var(--muted);
    font-weight: 600;
    text-align: left;
    position: relative;
  }
  .nav:hover {
    background: var(--surface);
    color: var(--text-2);
  }
  .nav.active {
    background: var(--surface-2);
    color: var(--text);
  }
  .nav.active::before {
    content: "";
    position: absolute;
    left: -10px;
    top: 9px;
    bottom: 9px;
    width: 3px;
    border-radius: 0 3px 3px 0;
    background: var(--accent-grad);
  }
  .live {
    border: 1px solid var(--border);
    background: var(--surface);
    border-radius: 10px;
    padding: 11px 12px;
    font-size: 12.5px;
  }
  .live .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--muted);
    flex: none;
  }
  .live.det .dot {
    background: var(--warn);
  }
  .live.rec .dot {
    background: var(--danger);
    box-shadow: 0 0 0 0 color-mix(in srgb, var(--danger) 60%, transparent);
    animation: pulse 1.6s infinite;
  }
  .live.rec {
    border-color: color-mix(in srgb, var(--danger) 35%, transparent);
  }
  @keyframes pulse {
    70% {
      box-shadow: 0 0 0 7px transparent;
    }
    100% {
      box-shadow: 0 0 0 0 transparent;
    }
  }
  .live .row {
    gap: 8px;
  }
  .sub {
    color: var(--muted);
    margin-top: 5px;
    line-height: 1.4;
  }
  .ok {
    color: var(--ok);
  }
  .btns {
    margin-top: 9px;
    gap: 6px;
  }
  .perf {
    display: flex;
    align-items: center;
    gap: 7px;
    font-size: 11.5px;
    color: var(--muted);
    padding: 0 6px;
    font-variant-numeric: tabular-nums;
  }
</style>
