<script lang="ts">
  import { untrack } from "svelte";
  import { api, fileSrc, confirmDialog } from "../lib/api";
  import { videoUrl } from "../lib/videopool";
  import { app, go, toast, cachedSession, fetchSession } from "../lib/store.svelte";
  import { clock, kda, kdaRatio, num, relativeDate } from "../lib/format";
  import { KIND, type Group } from "../lib/eventmeta";
  import { loadHidden, saveHidden } from "../lib/timelinefilters";
  import type { GameEvent, SessionView } from "../lib/types";
  import Player from "../components/Player.svelte";
  import GameIcon from "../components/GameIcon.svelte";
  import Scoreboard from "../components/Scoreboard.svelte";
  import ClipEditor from "../components/ClipEditor.svelte";
  import LineChart from "../components/LineChart.svelte";
  import Mechanics from "../components/Mechanics.svelte";
  import ChampionIcon from "../components/ChampionIcon.svelte";
  import Icon from "../components/Icon.svelte";

  let { id, t = 0 }: { id: string; t?: number } = $props();

  // Re-created per game ({#key} in App), so reading `id` once is intended.
  let view = $state<SessionView | null>(untrack(() => cachedSession(id)));
  // The library already knows the video, thumbnail and header facts: the page, the player and
  // the video loading start right away, while the full game data (events, stats) loads.
  const sum = $derived(app.sessions.find((x) => x.id === id) ?? null);
  let error = $state<string | null>(null);
  let player = $state<ReturnType<typeof Player>>();
  let current = $state(0);
  let hidden = $state(loadHidden());
  $effect(() => saveHidden(hidden));
  let range = $state<[number, number] | null>(null);
  let listEl = $state<HTMLDivElement>();
  /** Mechanics range (game seconds): range stats + the overlay's "selected range" heatmap. */
  let mechRange = $state<[number, number] | null>(null);

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
  const hd = $derived((s ?? sum) as any);
  const videoPath = $derived(view ? (view.video_path ?? null) : (sum?.video_path ?? null));
  const videoSrc = $derived(videoPath ? videoUrl(videoPath, view ? view.video_bytes : sum?.video_bytes) : null);
  const poster = $derived(fileSrc(view?.thumb_path ?? sum?.thumb_path) || null);
  const offset = $derived(s?.video_offset ?? 0);
  // The game has an input recording (from the library summary at once, then the full view).
  const hasInput = $derived((view ? (view.input_bytes ?? 0) : (sum?.input_bytes ?? 0)) > 0);
  // Sub-toggles of the overlay's ability bubbles for this game (League: abilities, summoners...).
  const bubbleCats = $derived(app.info?.games.find((g) => g.id === (s?.game_id ?? sum?.game_id))?.action_categories ?? []);
  const heatRange = $derived<[number, number] | null>(mechRange ? [mechRange[0] + offset, mechRange[1] + offset] : range);
  const events = $derived([...(s?.events ?? [])].sort((a, b) => a.game_time - b.game_time));
  /** Full text of an event row (its lines are cut to one line each). */
  function rowTip(e: GameEvent): string | undefined {
    const facts = (e.facts ?? []).filter(([k]) => k !== "Slot").map(([k, v]) => `${k}: ${v}`);
    const lines = [...(facts.length ? [facts.join(" · ")] : []), ...(e.details ? [e.details] : [])];
    return lines.length ? `${e.title}\n${lines.join("\n")}` : undefined;
  }
  const listEvents = $derived(events.filter((e) => !hidden.has((KIND[e.kind] ?? KIND.manual_marker).group)));
  const gameLen = $derived(s?.game_duration ?? (s?.video_duration ? s.video_duration - offset : 0));
  const videoLen = $derived(s?.video_duration ?? (s ? gameLen + offset : (sum?.duration ?? 0)));
  const clipRanges = $derived(
    (view?.clips ?? []).filter((c) => c.video_start != null && c.video_end != null).map((c) => ({ start: c.video_start!, end: c.video_end!, title: c.title })),
  );
  const resultLabel = $derived(hd?.result === "win" ? "Victory" : hd?.result === "loss" ? "Defeat" : hd?.result === "draw" ? "Draw" : "No result");
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

  async function toggleKeep(file: string, keep: boolean) {
    const c = view?.clips.find((x) => x.file === file);
    if (c) c.keep = keep; // instant feedback; the library refresh confirms it
    await api.setClipKeep(id, file, keep).catch((e) => toast(String(e), "error"));
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
  {:else if hd}
    <div class="head">
      <button class="btn ghost icon" onclick={() => go({ page: "games" })} aria-label="Back"><Icon name="back" size={18} /></button>
      <ChampionIcon id={hd.player?.character_id} name={hd.player?.character ?? hd.game_name} size={48} />
      <div class="htxt">
        <div class="row">
          <h1>{hd.player?.character ?? hd.game_name}</h1>
          <span class="result {hd.result ?? ''}">{resultLabel}</span>
        </div>
        <div class="muted">{hd.game_name}{(hd.mode_name ?? hd.player?.mode) ? ` · ${hd.mode_name ?? hd.player?.mode}` : ""}{hd.record_mode === "clips_only" ? " · clips only" : ""} · {relativeDate(hd.started_at)}{s ? ` · ${clock(gameLen)}` : ""}</div>
      </div>
      <!-- In a narrow window the buttons go on their own line (the title keeps its width). -->
      <div class="hact">
      <button class="btn" class:favon={hd.favorite} onclick={toggleFav} disabled={!s} title="Favorites are never deleted by the storage clean-up" aria-pressed={!!hd.favorite}><Icon name="star" size={15} fill={hd.favorite} />{hd.favorite ? "Favorite" : "Add to favorites"}</button>
      <button class="btn" onclick={startClip} disabled={!view?.video_path}><Icon name="scissors" size={15} />Create clip</button>
      <button class="btn" onclick={() => view && api.reveal(view.video_path ?? view.dir)} disabled={!view}><Icon name="folder" size={15} />Folder</button>
      <button class="btn ghost icon danger" onclick={del} disabled={!s} title="Delete game" aria-label="Delete game"><Icon name="trash" size={16} /></button>
      </div>
    </div>

    <div class="main">
      <div class="left">
        <Player
          bind:this={player}
          src={videoSrc}
          path={videoPath}
          {poster}
          {events}
          {offset}
          knownDuration={videoLen}
          clips={clipRanges}
          bind:hidden
          bind:range
          bind:current
          startAt={t}
          removed={!!(s?.video_removed_at ?? sum?.video_removed)}
          inputId={hasInput ? id : null}
          {bubbleCats}
          {heatRange}
          apm={s?.mechanics?.apm_bins ?? null}
          scoreboard={s?.scoreboard ?? null}
          sbPlayer={s?.player ?? null}
          sbStats={s?.stats ?? null}
        />
        {#if range && s}
          <ClipEditor
            sessionId={s.id}
            bind:range
            {current}
            {offset}
            duration={videoLen}
            onclose={() => (range = null)}
            onpreview={() => player?.seek(range![0], true)}
            inputId={hasInput ? id : null}
            showUnconfirmed={!hidden.has("unconfirmed")}
            {heatRange}
          />
        {/if}
      </div>

      <div class="events card">
        <div class="ev-head">
          <h3>Events</h3>
          <span class="muted">{s ? listEvents.length : ""}</span>
        </div>
        {#if s?.verification}
          {@const v = s.verification}
          <div class="verify muted" title="Ult key presses are checked against the ability bar in the recording after the game.">
            {#if v.status === "verified"}
              <Icon name="check" size={12} /> Ult casts checked against the recording{v.unconfirmed ? ` · ${v.unconfirmed} press${v.unconfirmed === 1 ? "" : "es"} without a cast (Unconfirmed presses)` : ""}
            {:else}
              <Icon name="info" size={12} /> Ult presses not checked: {v.reason ?? v.status}
            {/if}
          </div>
        {/if}
        <div class="ev-list" bind:this={listEl}>
          {#if !s}
            {#each [70, 55, 80, 60, 75] as w}<div class="ev"><span class="skeleton-line" style="width:{w}%"></span></div>{/each}
          {/if}
          {#each listEvents as e (e.id)}
            {@const m = KIND[e.kind] ?? KIND.manual_marker}
            <button class="ev" class:active={e.id === activeId} data-id={e.id} onclick={() => player?.jumpTo(e)} title={rowTip(e)}>
              {#if e.icon}
                <GameIcon icon={e.icon} size={22} title={e.title} fallback={m} />
              {:else}
                <span class="ev-ic" style="background:{m.color}"><Icon name={m.icon} size={12} stroke={2.4} /></span>
              {/if}
              <span class="ev-txt">
                <span class="ev-title">{e.title}{#if e.steal}<span class="steal">STEAL</span>{/if}</span>
                {#if e.facts?.length}<span class="ev-det">{e.facts.filter(([k]) => k !== "Slot").map(([k, v]) => `${k}: ${v}`).join(" · ")}</span>{/if}
                {#if e.details}<span class="ev-det">{e.details}</span>{/if}
              </span>
              <span class="ev-time">{clock(e.game_time)}</span>
            </button>
          {:else}
            {#if s}<div class="muted none">No events{events.length ? " (all filtered out)" : ""}.</div>{/if}
          {/each}
        </div>
      </div>
    </div>

    {#if s && (s.scoreboard || s.player)}
      <div class="card sbcard">
        <Scoreboard sb={s.scoreboard ?? null} t={current - offset} player={s.player} stats={s.stats} />
        <div class="sbhint muted">Press <kbd>O</kbd> to show it over the video (or hold <kbd>Tab</kbd> in fullscreen).</div>
      </div>
    {/if}

    {#if s && view}
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

    {#if s.mechanics}
      <Mechanics whole={s.mechanics} {id} {offset} canRange={hasInput} bind:range={mechRange} />
    {:else if s.input_file && hasInput}
      <div class="card mech-wait muted">Mechanics stats are computed shortly after the game (while no game is running).</div>
    {/if}

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
            <button class="btn small ghost keepbtn" class:on={c.keep} onclick={() => toggleKeep(c.file, !c.keep)} title={c.keep ? "Kept: never deleted by the storage clean-up" : "Keep this clip (never auto-deleted)"} aria-pressed={!!c.keep}><Icon name="pin" size={13} />{c.keep ? "Kept" : "Keep"}</button>
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
    {/if}
  {:else}
    <div class="head" aria-busy="true">
      <span class="skeleton" style="width:48px;height:48px;border-radius:10px"></span>
      <div class="htxt"><span class="skeleton-line" style="width:180px;height:22px"></span><br /><span class="skeleton-line" style="width:260px"></span></div>
    </div>
    <div class="main">
      <div class="skeleton" style="aspect-ratio:16/9;border-radius:var(--radius)"></div>
      <div class="skeleton" style="height:420px;border-radius:var(--radius)"></div>
    </div>
  {/if}
</div>

<style>
  .mech-wait {
    padding: 14px 16px;
    margin-top: 16px;
    font-size: 13px;
  }
  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 10px 14px;
    margin-bottom: 20px;
  }
  .htxt {
    flex: 1 1 280px;
    min-width: 0;
  }
  .hact {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: flex-end;
    gap: 8px 14px;
    margin-left: auto;
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
    color: var(--ok-text);
    background: var(--ok-soft);
  }
  .result.loss {
    color: var(--danger-text);
    background: var(--danger-soft);
  }
  .favon {
    color: var(--fav);
    border-color: color-mix(in srgb, var(--fav) 40%, transparent);
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
    color: var(--on-ev);
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
    background: var(--fav);
    color: var(--on-fav);
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
  .keepbtn.on {
    color: var(--fav);
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
    border-color: color-mix(in srgb, var(--warn) 35%, transparent);
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
  .verify {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 11.5px;
    padding: 0 14px 8px;
  }
  .sbcard {
    margin-top: 16px;
    padding: 14px 16px;
  }
  .sbhint {
    font-size: 11.5px;
    margin-top: 8px;
  }
</style>
