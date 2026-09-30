<script lang="ts">
  import { api } from "../lib/api";
  import { app, toast } from "../lib/store.svelte";
  import type { PerfTestStatus, PhaseResult } from "../lib/types";
  import Icon from "./Icon.svelte";

  let status = $state<PerfTestStatus | null>(null);
  let secs = $state(60);

  $effect(() => {
    let alive = true;
    const tick = async () => {
      if (!alive) return;
      try {
        status = await api.perfTestStatus();
      } catch {}
    };
    tick();
    const t = setInterval(tick, 1000);
    return () => {
      alive = false;
      clearInterval(t);
    };
  });

  const busy = $derived(["preparing", "waiting_for_game", "running", "waiting_for_exit", "analyzing"].includes(status?.state ?? ""));

  async function start() {
    try {
      await api.perfTestStart(secs);
      toast("Performance test armed. Accept the Windows prompt (it lets PresentMon measure FPS), then start a Practice Tool game.", "info", 9000);
    } catch (e) {
      toast(String(e), "error");
    }
  }

  const base = $derived(status?.report?.phases[0]);
  function delta(p: PhaseResult, key: "fps_avg" | "fps_1_low") {
    const a = base?.[key];
    const b = p[key];
    if (a == null || b == null || p === base) return "";
    const d = ((b - a) / a) * 100;
    return `${d >= 0 ? "+" : ""}${d.toFixed(1)}%`;
  }
  const f = (v: number | null | undefined, d = 1) => (v == null ? "-" : v.toFixed(d));
</script>

<div class="pt">
  <p class="lead">
    Measures what recording costs in a real game: League FPS (average and 1% lows), CPU and GPU, first without recording, then with the built-in recorder. About {Math.round((secs * 2) / 60 + 1)} minutes of play.
  </p>
  <ol class="steps">
    <li>Click <strong>Start test</strong> and accept the Windows prompt (PresentMon, Intel's free frame-time tool, needs it to read FPS; it never touches the game).</li>
    <li>Start a <strong>Practice Tool</strong> game and play normally (walk around, fight minions). You'll hear each phase start.</li>
    <li><strong>Stay in the game until you hear “Performance test finished”</strong> (about {Math.round((secs * 2) / 60 + 1)} minutes). Leaving earlier stops the test. Then leave the game: the results appear here.</li>
    <li>For a fair comparison, do the same thing in every phase (for example, stand in lane and last-hit). Big fights in one phase and not the other change the FPS more than recording does.</li>
  </ol>
  <div class="row controls">
    <label class="inline">Phase length
      <select class="input" bind:value={secs} disabled={busy}>
        <option value={45}>45 s</option>
        <option value={60}>60 s</option>
        <option value={90}>90 s</option>
        <option value={120}>2 min</option>
      </select>
    </label>
    <div class="spacer"></div>
    {#if busy}
      <button class="btn danger" onclick={() => api.perfTestCancel()}>Cancel</button>
    {:else}
      <button class="btn primary" onclick={start}><Icon name="play" size={13} fill />Start test</button>
    {/if}
  </div>

  {#if busy}
    <div class="state">
      <span class="dot"></span>
      {#if status?.state === "running"}
        Measuring: <strong>{status.phase}</strong> ({Math.ceil(status.phase_ends_in ?? 0)} s left)
      {:else}
        {status?.message ?? status?.state}
      {/if}
    </div>
  {:else if status?.state === "error"}
    <div class="warn"><Icon name="warn" size={14} /> {status.message}</div>
  {/if}

  {#if status?.report}
    {@const r = status.report}
    <div class="report">
      <div class="muted small">{r.started_at} · {r.gpu} · {r.encoder || "encoder n/a"} · FPS from {r.fps_source}</div>
      <table>
        <thead>
          <tr><th>Phase</th><th>Avg FPS</th><th>1% low</th><th>League CPU</th><th>PC CPU</th><th>Clairvoyance</th><th>GPU 3D</th><th>GPU encode</th></tr>
        </thead>
        <tbody>
          {#each r.phases as p}
            <tr>
              <td>{p.name}</td>
              <td>{f(p.fps_avg, 0)} <span class="d">{delta(p, "fps_avg")}</span></td>
              <td>{f(p.fps_1_low, 0)} <span class="d">{delta(p, "fps_1_low")}</span></td>
              <td>{f(p.game_cpu)}%</td>
              <td>{f(p.total_cpu)}%</td>
              <td>{f(p.app_cpu, 2)}% · {Math.round(p.app_ram_mb)} MB</td>
              <td>{f(p.gpu_3d)}%</td>
              <td>{f(p.gpu_encode)}%</td>
            </tr>
          {/each}
        </tbody>
      </table>
      {#each r.notes as n}<div class="warn small">{n}</div>{/each}
      {#if status.report_path}<button class="btn small ghost" onclick={() => api.reveal(status!.report_path!)}><Icon name="folder" size={13} />Show report file</button>{/if}
    </div>
  {/if}
</div>

<style>
  .lead {
    color: var(--text-2);
    line-height: 1.55;
    margin: 0 0 10px;
  }
  .steps {
    margin: 0 0 14px;
    padding-left: 20px;
    color: var(--text-2);
    line-height: 1.6;
    font-size: 13.5px;
  }
  .controls {
    flex-wrap: wrap;
  }
  .inline {
    display: flex;
    align-items: center;
    gap: 8px;
    font-weight: 600;
    font-size: 13px;
  }
  .state {
    margin-top: 14px;
    display: flex;
    align-items: center;
    gap: 10px;
    color: var(--text-2);
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--accent);
    animation: blink 1.2s infinite;
  }
  @keyframes blink {
    50% {
      opacity: 0.3;
    }
  }
  .warn {
    color: var(--warn);
    margin-top: 10px;
    display: flex;
    gap: 6px;
    align-items: center;
  }
  .small {
    font-size: 12.5px;
  }
  .report {
    margin-top: 16px;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    margin: 10px 0;
    font-size: 13px;
    font-variant-numeric: tabular-nums;
  }
  th,
  td {
    text-align: left;
    padding: 8px 10px;
    border-bottom: 1px solid var(--border);
  }
  th {
    color: var(--muted);
    font-weight: 600;
    font-size: 12px;
  }
  .d {
    color: var(--muted);
    font-size: 11.5px;
  }
</style>
