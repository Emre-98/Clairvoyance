<script lang="ts" module>
  import type { StorageInfo as SI } from "../lib/types";
  let storageCache: SI | null = null;
</script>

<script lang="ts">
  import { keepScroll } from "../lib/scroll";
  import { app, go, reloadSettings, toast } from "../lib/store.svelte";
  import { api } from "../lib/api";
  import { clock, kda, bytes } from "../lib/format";
  import { KIND } from "../lib/eventmeta";
  import GameCard from "../components/GameCard.svelte";
  import ChampionIcon from "../components/ChampionIcon.svelte";
  import Icon from "../components/Icon.svelte";
  import type { StorageInfo } from "../lib/types";

  const st = $derived(app.status);
  const recent = $derived(app.sessions.slice(0, 8));

  // Numbers for the last 7 days.
  const week = $derived.by(() => {
    const cutoff = Date.now() - 7 * 86400000;
    const g = app.sessions.filter((s) => new Date(s.started_at).getTime() >= cutoff);
    const decided = g.filter((s) => s.result === "win" || s.result === "loss");
    const wins = decided.filter((s) => s.result === "win").length;
    return { games: g.length, winrate: decided.length ? Math.round((wins / decided.length) * 100) : null, clips: g.reduce((a, s) => a + s.clip_count, 0) };
  });

  let storage = $state<StorageInfo | null>(storageCache);
  $effect(() => {
    app.libraryVersion;
    api.storageInfo().then((s) => (storage = storageCache = s)).catch(() => {});
  });

  // Tick the game clock between status updates.
  let now = $state(Date.now());
  $effect(() => {
    const t = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(t);
  });
  let statusAt = $state(Date.now());
  $effect(() => {
    st;
    statusAt = Date.now();
  });
  const liveTime = $derived(st?.game_time != null ? st.game_time + (now - statusAt) / 1000 : null);

  let removingLegacy = $state(false);
  async function removeLegacy() {
    removingLegacy = true;
    try {
      await api.removeLegacyApp();
      await reloadSettings();
      toast("The old GameRecorder app was uninstalled. Your games are all here.", "ok");
    } catch (e) {
      toast(String(e), "error", 9000);
    } finally {
      removingLegacy = false;
    }
  }

  const phaseLabel: Record<string, string> = { waiting: "In client / loading", loading: "Loading screen", in_progress: "In game", ended: "Game over" };
</script>

<div class="page" use:keepScroll={"home"}>
  <div class="page-head">
    <div>
      <h1>Welcome back</h1>
      <p class="muted" style="margin:6px 0 0">Clairvoyance records automatically when a supported game starts. Just play.</p>
    </div>
  </div>

  {#if app.info?.legacy_install}
    <div class="card banner">
      <Icon name="info" size={20} />
      <div class="spacer">
        <strong>GameRecorder is now Clairvoyance</strong>
        <div class="muted">Your settings, games and clips were moved over. The old GameRecorder app is still installed; remove it so both don't record at the same time.</div>
      </div>
      <button class="btn primary" onclick={removeLegacy} disabled={removingLegacy}>{removingLegacy ? "Removing…" : "Uninstall old app"}</button>
    </div>
  {/if}

  <section class="hero card" class:rec={st?.state === "recording"} class:det={st?.state === "detected"}>
    {#if st && st.state !== "idle"}
      <div class="hero-left">
        <ChampionIcon id={st.player?.character_id} name={st.player?.character ?? st.game_name} size={64} />
        <div>
          <div class="state"><span class="dot"></span>{st.state === "recording" ? (st.mode_rule === "clips_only" ? "Clips only" : "Recording") : st.mode_rule === "off" ? "Not recorded (mode off)" : "Game detected, not recording"}</div>
          <h2>{st.player?.character ?? st.game_name}</h2>
          <div class="muted">{st.game_name} · {phaseLabel[st.phase] ?? st.phase}{(st.mode_name ?? st.player?.mode) ? ` · ${st.mode_name ?? st.player?.mode}` : ""}{st.mode_rule === "clips_only" ? " · clips only" : ""}</div>
        </div>
      </div>
      <div class="hero-stats">
        <div><span class="label">Game time</span><span class="big">{clock(liveTime)}</span></div>
        <div><span class="label">KDA</span><span class="big">{kda(st.stats)}</span></div>
        <div><span class="label">Events</span><span class="big">{st.event_count}</span></div>
      </div>
      <div class="hero-actions">
        {#if st.last_event}
          <span class="pill"><span class="evdot" style="background:{KIND[st.last_event.kind]?.color}"></span>{st.last_event.title}</span>
        {/if}
        <div class="row">
          <button class="btn" onclick={() => api.saveClipNow()} disabled={st.state !== "recording"}><Icon name="scissors" size={15} />Save clip <kbd>{app.settings?.hotkey_clip}</kbd></button>
          <button class="btn" onclick={() => api.addMarkerNow()}><Icon name="bookmark" size={15} />Marker <kbd>{app.settings?.hotkey_marker}</kbd></button>
          <button class="btn ghost" onclick={() => api.stopSession()} title="End this recording now">Stop</button>
        </div>
      </div>
      {#if st.message}<div class="msg"><Icon name="warn" size={15} />{st.message}</div>{/if}
    {:else}
      <div class="hero-left">
        <div class="idle-icon"><Icon name="gamepad" size={30} stroke={1.6} /></div>
        <div>
          <div class="state"><span class="dot"></span>Ready</div>
          <h2>Waiting for a game</h2>
          <div class="muted">
            Supported: {app.info?.games.map((g) => g.name).join(", ")}. Hotkeys in game: <kbd>{app.settings?.hotkey_clip}</kbd> save clip, <kbd>{app.settings?.hotkey_marker}</kbd> marker.
          </div>
        </div>
      </div>
      <div class="hero-stats">
        <div><span class="label">Games this week</span><span class="big">{week.games}</span></div>
        <div><span class="label">Win rate (7 days)</span><span class="big">{week.winrate == null ? "-" : `${week.winrate}%`}</span></div>
        <div><span class="label">Storage used</span><span class="big">{storage ? bytes(storage.used_bytes) : "-"}</span></div>
      </div>
    {/if}
  </section>

  <div class="section-head">
    <h2>Recent games</h2>
    {#if app.sessions.length > recent.length}<button class="btn ghost small" onclick={() => go({ page: "games" })}>See all {app.sessions.length}</button>{/if}
  </div>
  {#if !app.sessionsLoaded}
    <div class="grid">{#each Array(4) as _}<div class="card skel"></div>{/each}</div>
  {:else if recent.length === 0}
    <div class="empty">
      <Icon name="clips" size={34} stroke={1.5} />
      <strong>No games yet</strong>
      <span>Start a match and it will show up here with its timeline.</span>
    </div>
  {:else}
    <div class="grid">
      {#each recent as s (s.id)}<GameCard {s} />{/each}
    </div>
  {/if}
</div>

<style>
  .banner {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 14px 16px;
    margin-bottom: 16px;
    border-color: color-mix(in srgb, var(--accent) 35%, transparent);
    color: var(--accent-text);
  }
  .banner strong {
    color: var(--text);
  }
  .hero {
    position: relative;
    display: grid;
    grid-template-columns: minmax(280px, 1.3fr) minmax(300px, 1fr);
    gap: 20px 28px;
    padding: 22px 24px;
    margin-bottom: 30px;
    overflow: hidden;
    background:
      radial-gradient(600px 200px at 0% 0%, var(--glow-a), transparent 70%),
      radial-gradient(500px 220px at 100% 100%, var(--glow-b), transparent 70%),
      var(--surface);
  }
  .hero.rec {
    border-color: color-mix(in srgb, var(--danger) 35%, transparent);
    background:
      radial-gradient(600px 220px at 0% 0%, color-mix(in srgb, var(--danger) 10%, transparent), transparent 70%),
      var(--surface);
  }
  .hero-left {
    display: flex;
    align-items: center;
    gap: 18px;
  }
  .hero-left h2 {
    font-size: 22px;
    margin: 4px 0 4px;
  }
  .state {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--accent);
  }
  .hero.rec .state {
    color: var(--danger-text);
  }
  .hero.det .state {
    color: var(--warn);
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: currentColor;
  }
  .hero.rec .dot {
    animation: blink 1.2s infinite;
  }
  @keyframes blink {
    50% {
      opacity: 0.25;
    }
  }
  .idle-icon {
    width: 64px;
    height: 64px;
    border-radius: 16px;
    display: grid;
    place-items: center;
    background: var(--surface-2);
    color: var(--accent);
    border: 1px solid var(--border-2);
  }
  .hero-stats {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 12px;
    align-self: center;
  }
  .hero-stats > div {
    background: color-mix(in srgb, var(--surface-2) 60%, transparent);
    border: 1px solid var(--border);
    border-radius: 10px;
    padding: 12px 14px;
  }
  .label {
    display: block;
    font-size: 11.5px;
    color: var(--muted);
    margin-bottom: 4px;
  }
  .big {
    font-size: 21px;
    font-weight: 700;
    font-variant-numeric: tabular-nums;
    font-family: var(--font-display);
  }
  .hero-actions {
    grid-column: 1 / -1;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    flex-wrap: wrap;
  }
  .evdot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
  }
  .msg {
    grid-column: 1 / -1;
    display: flex;
    gap: 8px;
    align-items: center;
    color: var(--warn);
    font-size: 13px;
  }
  .section-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 14px;
  }
  .skel {
    aspect-ratio: 4 / 3;
    background: linear-gradient(90deg, var(--surface) 0%, var(--surface-2) 50%, var(--surface) 100%);
    background-size: 200% 100%;
    animation: sh 1.4s infinite;
  }
  @keyframes sh {
    to {
      background-position: -200% 0;
    }
  }
</style>
