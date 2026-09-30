<script lang="ts">
  import { untrack } from "svelte";
  import { api, fileSrc, confirmDialog } from "../lib/api";
  import { app, go, toast, cachedSession, fetchSession } from "../lib/store.svelte";
  import { clock, kda, kdaRatio, num, relativeDate } from "../lib/format";
  import { KIND, type Group } from "../lib/eventmeta";
  import type { GameEvent, SessionView } from "../lib/types";
  import Player from "../components/Player.svelte";
  import ClipEditor from "../components/ClipEditor.svelte";
  import LineChart from "../components/LineChart.svelte";
  import ChampionIcon from "../components/ChampionIcon.svelte";
  import Icon from "../components/Icon.svelte";

  let { id, t = 0 }: { id: string; t?: number } = $props();

  // Re-created per game ({#key} in App), so reading `id` once is intended.
  let view = $state<SessionView | null>(untrack(() => cachedSession(id)));
  let error = $state<string | null>(null);
  let player = $state<ReturnType<typeof Player>>();
  let current = $state(0);
  let hidden = $state(new Set<Group>(["game"]));
  let range = $state<[number, number] | null>(null);
  let listEl = $state<HTMLDivElement>();

  async function load() {
    try {
      view = await fetchSession(id);
      error = null;
    } catch (e) {
      error = String(e);
    }
  }
  $effect(() => {
    id;
    app.libraryVersion;
    load();
  });

  const s = $derived(view?.session);
  const offset = $derived(s?.video_offset ?? 0);
  const events = $derived([...(s?.events ?? [])].sort((a, b) => a.game_time - b.game_time));
  const listEvents = $derived(events.filter((e) => !hidden.has((KIND[e.kind] ?? KIND.manual_marker).group)));
  const gameLen = $derived(s?.game_duration ?? (s?.video_duration ? s.video_duration - offset : 0));
  const videoLen = $derived(s?.video_duration ?? gameLen + offset);
  const clipRanges = $derived(
    (view?.clips ?? []).filter((c) => c.video_start != null && c.video_end != null).map((c) => ({ start: c.video_start!, end: c.video_end!, title: c.title })),
  );
  const resultLabel = $derived(s?.result === "win" ? "Victory" : s?.result === "loss" ? "Defeat" : s?.result === "draw" ? "Draw" : "No result");
  const deaths = $derived(events.filter((e) => e.kind === "death"));
  const csPerMin = $derived(s?.stats?.cs != null && gameLen > 60 ? s.stats.cs / (gameLen / 60) : null);

  const series = $derived.by(() => {
    const tl = [...(s?.timeline ?? [])];
    if (s?.stats && gameLen > 0) tl.push({ t: gameLen, kills: s.stats.kills, deaths: s.stats.deaths, assists: s.stats.assists, cs: s.stats.cs, gold: s.stats.gold });
    return {
      gold: tl.filter((p) => p.gold != null).map((p) => ({ x: p.t, y: p.gold! })),
      cs: tl.filter((p) => p.cs != null).map((p) => ({ x: p.t, y: p.cs! })),
    };
  });

  // Keep the active event visible in the list.
  const activeId = $derived.by(() => {
    let a: string | null = null;
    for (const e of listEvents) if (e.game_time + offset <= current + 5.5) a = e.id;
    return a;
  });
  $effect(() => {
    if (!activeId || !listEl) return;
    const el = listEl.querySelector(`[data-id="${CSS.escape(activeId)}"]`);
    el?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  });

  function startClip() {
    const c = current;
    range = [Math.max(0, c - 10), Math.min(videoLen, c + 5)];
  }

  async function toggleFav() {
    if (!s) return;
    await api.setFavorite(s.id, !s.favorite);
  }

  async function del() {
    if (!s) return;
    if (!(await confirmDialog("Delete this game, its video and all its clips? This can't be undone."))) return;
    try {
      await api.deleteSession(s.id);
      toast("Game deleted", "ok");
      go({ page: "games" });
    } catch (e) {
      toast(String(e), "error");
    }
  }

  async function delClip(file: string) {
    if (!(await confirmDialog("Delete this clip?"))) return;
    await api.deleteClip(id, file).catch((e) => toast(String(e), "error"));
  }
</script>

<div class="page">
  {#if error}
    <button class="btn ghost" onclick={() => go({ page: "games" })}><Icon name="back" size={16} />Back</button>
    <div class="empty" style="margin-top:20px">{error}</div>
  {:else if s && view}
    <div class="head">
      <button class="btn ghost icon" onclick={() => go({ page: "games" })} aria-label="Back"><Icon name="back" size={18} /></button>
      <ChampionIcon id={s.player?.character_id} name={s.player?.character ?? s.game_name} size={48} />
      <div class="htxt">
        <div class="row">
          <h1>{s.player?.character ?? s.game_name}</h1>
          <span class="result {s.result ?? ''}">{resultLabel}</span>
        </div>
        <div class="muted">{s.game_name}{s.player?.mode ? ` · ${s.player.mode}` : ""} · {relativeDate(s.started_at)} · {clock(gameLen)}</div>
      </div>
      <div class="spacer"></div>
      <button class="btn" class:favon={s.favorite} onclick={toggleFav}><Icon name="star" size={15} fill={s.favorite} />{s.favorite ? "Favorite" : "Keep"}</button>
      <button class="btn" onclick={startClip} disabled={!view.video_path}><Icon name="scissors" size={15} />Create clip</button>
      <button class="btn" onclick={() => api.reveal(view!.video_path ?? view!.dir)}><Icon name="folder" size={15} />Folder</button>
      <button class="btn ghost icon danger" onclick={del} title="Delete game"><Icon name="trash" size={16} /></button>
    </div>

    <div class="main">
      <div class="left">
        <Player
          bind:this={player}
          src={view.video_path ? fileSrc(view.video_path) : null}
          {events}
          {offset}
          knownDuration={videoLen}
          clips={clipRanges}
          bind:hidden
          bind:range
          bind:current
          startAt={t}
        />
        {#if range}
          <ClipEditor sessionId={s.id} bind:range {current} {offset} duration={videoLen} onclose={() => (range = null)} onpreview={() => player?.seek(range![0], true)} />
        {/if}
      </div>

      <div class="events card">
        <div class="ev-head">
          <h3>Events</h3>
          <span class="muted">{listEvents.length}</span>
        </div>
        <div class="ev-list" bind:this={listEl}>
          {#each listEvents as e (e.id)}
            {@const m = KIND[e.kind] ?? KIND.manual_marker}
            <button class="ev" class:active={e.id === activeId} data-id={e.id} onclick={() => player?.jumpTo(e)}>
              <span class="ev-ic" style="background:{m.color}"><Icon name={m.icon} size={12} stroke={2.4} /></span>
              <span class="ev-txt">
                <span class="ev-title">{e.title}{#if e.steal}<span class="steal">STEAL</span>{/if}</span>
                {#if e.details}<span class="ev-det">{e.details}</span>{/if}
              </span>
              <span class="ev-time">{clock(e.game_time)}</span>
            </button>
          {:else}
            <div class="muted none">No events{events.length ? " (all filtered out)" : ""}.</div>
          {/each}
        </div>
      </div>
    </div>

    <h2 class="sec">Summary</h2>
    <div class="tiles">
      <div class="tile"><span class="label">KDA</span><span class="big">{kda(s.stats)}</span><span class="sub">{kdaRatio(s.stats)} ratio</span></div>
      {#if s.stats?.cs != null}
        <div class="tile"><span class="label">CS</span><span class="big">{num(s.stats.cs)}</span><span class="sub">{csPerMin != null ? csPerMin.toFixed(1) : "-"} per min</span></div>
      {/if}
      {#if s.stats?.gold != null}
        <div class="tile"><span class="label">Gold (items + bank)</span><span class="big">{num(s.stats.gold)}</span><span class="sub">{gameLen > 60 ? Math.round(s.stats.gold / (gameLen / 60)) : "-"} per min</span></div>
      {/if}
      {#if s.stats?.vision_score != null}
        <div class="tile"><span class="label">Vision score</span><span class="big">{num(s.stats.vision_score)}</span><span class="sub">Level {s.stats.level ?? "-"}</span></div>
      {/if}
      <div class="tile"><span class="label">Duration</span><span class="big">{clock(gameLen)}</span><span class="sub">{s.events.length} events</span></div>
      {#each s.stats?.extra ?? [] as [k, v]}
        <div class="tile"><span class="label">{k}</span><span class="big">{v}</span></div>
      {/each}
    </div>

    {#if deaths.length && gameLen > 0}
      <div class="card deaths">
        <div class="row"><h3>Deaths timeline</h3><span class="muted">{deaths.length} death{deaths.length === 1 ? "" : "s"}</span></div>
        <div class="dbar">
          <div class="drail"></div>
          {#each deaths as d (d.id)}
            <button class="dmark" style="left:{(d.game_time / gameLen) * 100}%" title="{d.title} · {clock(d.game_time)}" onclick={() => player?.jumpTo(d)}>
              <Icon name="skull" size={13} stroke={2.2} />
              <span>{clock(d.game_time)}</span>
            </button>
          {/each}
        </div>
      </div>
    {/if}

    {#if series.gold.length > 1 || series.cs.length > 1}
      <div class="charts">
        {#if series.gold.length > 1}
          <div class="card chartcard">
            <h3>Gold over time</h3>
            <LineChart label="Gold over time" points={series.gold} color="var(--ev-epic)" xFormat={(v) => clock(v)} yFormat={(v) => (v >= 1000 ? (v / 1000).toFixed(v % 1000 ? 1 : 0) + "k" : String(Math.round(v)))} />
          </div>
        {/if}
        {#if series.cs.length > 1}
          <div class="card chartcard">
            <h3>CS over time</h3>
            <LineChart label="Creep score over time" points={series.cs} color="var(--accent)" xFormat={(v) => clock(v)} />
          </div>
        {/if}
      </div>
    {/if}

    {#if view.clips.length}
      <h2 class="sec">Clips from this game</h2>
      <div class="cliplist card">
        {#each view.clips as c (c.file)}
          <div class="clip">
            <Icon name="film" size={16} />
            <div class="ctxt">
              <strong>{c.title}</strong>
              <span class="muted">{c.video_start != null ? `${clock(c.video_start - offset)} – ${clock((c.video_end ?? 0) - offset)}` : ""} · {c.source === "replay" ? "hotkey" : c.source === "event" ? "auto" : "edited"}{c.exists ? "" : " · file missing"}</span>
            </div>
            {#if c.video_start != null}<button class="btn small ghost" onclick={() => player?.seek(c.video_start!, true)}>Watch here</button>{/if}
            <button class="btn small ghost" onclick={() => api.openPath(c.path)} disabled={!c.exists}><Icon name="external" size={13} />Open</button>
            <button class="btn small ghost" onclick={() => api.reveal(c.path)} disabled={!c.exists}><Icon name="folder" size={13} /></button>
            <button class="btn small ghost" onclick={() => delClip(c.file)}><Icon name="trash" size={13} /></button>
          </div>
        {/each}
      </div>
    {/if}

    <div class="foot">
      {#if s.perf.samples > 0}
        <div class="pill" title="Measured every 5 seconds while the game ran">
          <Icon name="cpu" size={13} />Clairvoyance during this game: {s.perf.cpu_avg.toFixed(2)}% CPU avg ({s.perf.cpu_max.toFixed(1)}% max), {Math.round(s.perf.ram_avg_mb)} MB RAM ({Math.round(s.perf.ram_max_mb)} max)
        </div>
      {/if}
      <div class="pill" title="Video position when the game clock was 0:00">Video offset {offset.toFixed(2)} s</div>
      {#each s.warnings as w}<div class="pill warnpill"><Icon name="warn" size={13} />{w}</div>{/each}
    </div>
  {:else}
    <div class="muted">Loading…</div>
  {/if}
</div>

<style>
  .head {
    display: flex;
    align-items: center;
    gap: 14px;
    margin-bottom: 20px;
  }
  .htxt h1 {
    font-size: 24px;
  }
  .htxt .muted {
    margin-top: 3px;
  }
  .result {
    font-size: 12px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    padding: 3px 9px;
    border-radius: 6px;
    background: var(--surface-2);
    color: var(--muted);
  }
  .result.win {
    color: #7cf0b0;
    background: rgba(61, 220, 132, 0.14);
  }
  .result.loss {
    color: #ff9cb0;
    background: rgba(255, 77, 109, 0.14);
  }
  .favon {
    color: #f5c542;
    border-color: rgba(245, 197, 66, 0.4);
  }
  .danger:hover {
    color: var(--danger);
  }
  .main {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 320px;
    gap: 16px;
    align-items: start;
  }
  .events {
    display: flex;
    flex-direction: column;
    max-height: calc(100vh - 200px);
    min-height: 300px;
    position: sticky;
    top: 0;
  }
  .ev-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 14px 16px 10px;
    border-bottom: 1px solid var(--border);
  }
  .ev-list {
    overflow-y: auto;
    padding: 6px;
    flex: 1;
  }
  .ev {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 10px;
    border: none;
    border-radius: 8px;
    background: transparent;
    text-align: left;
  }
  .ev:hover {
    background: var(--surface-2);
  }
  .ev.active {
    background: var(--surface-3);
  }
  .ev-ic {
    width: 22px;
    height: 22px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    color: #0b0d14;
    flex: none;
  }
  .ev-txt {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .ev-title {
    font-weight: 600;
    font-size: 13px;
  }
  .ev-det {
    font-size: 11.5px;
    color: var(--muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .ev-time {
    font-variant-numeric: tabular-nums;
    color: var(--text-2);
    font-size: 12.5px;
  }
  .steal {
    font-size: 9.5px;
    font-weight: 800;
    margin-left: 6px;
    padding: 1px 5px;
    border-radius: 4px;
    background: #f5c542;
    color: #1a1400;
    vertical-align: 1px;
  }
  .none {
    padding: 16px;
  }
  .sec {
    margin: 30px 0 14px;
  }
  .tiles {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(170px, 1fr));
    gap: 12px;
  }
  .tile {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 14px 16px;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .label {
    font-size: 12px;
    color: var(--muted);
  }
  .big {
    font-size: 24px;
    font-weight: 700;
    font-family: var(--font-display);
    font-variant-numeric: tabular-nums;
  }
  .sub {
    font-size: 12px;
    color: var(--text-2);
  }
  .deaths {
    margin-top: 14px;
    padding: 14px 18px 18px;
  }
  .deaths .row {
    justify-content: space-between;
  }
  .dbar {
    position: relative;
    height: 44px;
    margin: 12px 20px 0;
  }
  .drail {
    position: absolute;
    left: 0;
    right: 0;
    top: 12px;
    height: 4px;
    border-radius: 2px;
    background: var(--surface-3);
  }
  .dmark {
    position: absolute;
    top: 0;
    transform: translateX(-50%);
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 3px;
    border: none;
    background: none;
    padding: 0;
    color: var(--ev-death);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
  }
  .dmark span {
    color: var(--text-2);
  }
  .dmark :global(svg) {
    background: var(--surface);
    border-radius: 50%;
    padding: 2px;
    box-sizing: content-box;
    border: 2px solid var(--ev-death);
  }
  .charts {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 14px;
    margin-top: 14px;
  }
  .chartcard {
    padding: 14px 16px 10px;
  }
  .chartcard h3 {
    margin-bottom: 14px;
  }
  .cliplist {
    padding: 6px;
  }
  .clip {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 10px;
    border-radius: 8px;
    color: var(--accent);
  }
  .clip:hover {
    background: var(--surface-2);
  }
  .ctxt {
    flex: 1;
    display: flex;
    flex-direction: column;
    color: var(--text);
  }
  .ctxt .muted {
    font-size: 12px;
  }
  .foot {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-top: 26px;
  }
  .warnpill {
    color: var(--warn);
    border-color: rgba(245, 184, 61, 0.35);
  }
  @media (max-width: 1150px) {
    .main {
      grid-template-columns: 1fr;
    }
    .events {
      position: static;
      max-height: 360px;
    }
    .charts {
      grid-template-columns: 1fr;
    }
  }
</style>
