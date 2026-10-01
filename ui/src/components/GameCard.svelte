<script lang="ts">
  import type { SessionSummary } from "../lib/types";
  import { fileSrc, api } from "../lib/api";
  import { clock, kda, relativeDate } from "../lib/format";
  import { go, prefetchSession } from "../lib/store.svelte";
  import { warmVideo, cancelWarm, videoUrl } from "../lib/videopool";
  import ChampionIcon from "./ChampionIcon.svelte";
  import Icon from "./Icon.svelte";

  let { s }: { s: SessionSummary } = $props();
  // Optimistic: the star flips at once; the library refresh confirms it.
  let favLocal = $state<boolean | null>(null);
  const fav = $derived(favLocal ?? s.favorite);
  $effect(() => {
    s.favorite;
    favLocal = null;
  });
  let loaded = $state(false);
  let failed = $state(false);

  const thumb = $derived(
    s.thumb_path
      ? fileSrc(s.thumb_path)
      : s.game_id === "league" && s.player?.character_id
        ? `https://ddragon.leagueoflegends.com/cdn/img/champion/splash/${s.player.character_id}_0.jpg`
        : null,
  );
  const resultLabel = $derived(s.result === "win" ? "Victory" : s.result === "loss" ? "Defeat" : s.result === "draw" ? "Draw" : null);

  // Hovering a card loads the game's data and starts loading its video (index + first frame),
  // so the page is ready when the click lands.
  function warm() {
    prefetchSession(s.id);
    if (s.video_path) warmVideo(videoUrl(s.video_path, s.video_bytes));
  }

  async function toggleFav(e: MouseEvent) {
    e.stopPropagation();
    favLocal = !fav;
    await api.setFavorite(s.id, favLocal);
  }
</script>

<button class="card game" onpointerenter={warm} onpointerleave={cancelWarm} onfocus={warm} onclick={() => go({ page: "game", id: s.id })}>
  <div class="thumb">
    {#if thumb && !failed}
      {#if !loaded}<div class="skeleton ph-skel"></div>{/if}
      <img src={thumb} alt="" loading="lazy" decoding="async" draggable="false" class:shown={loaded} onload={() => (loaded = true)} onerror={() => (failed = true)} />
      <div class="shade"></div>
    {:else}
      <div class="ph"><Icon name="gamepad" size={34} stroke={1.5} /></div>
    {/if}
    {#if resultLabel}<span class="badge {s.result}">{resultLabel}</span>{/if}
    <span class="fav" class:on={fav} role="button" tabindex="-1" title={fav ? "Favorite (never auto-deleted)" : "Add to favorites"} onclick={toggleFav} onkeydown={() => {}}>
      <Icon name="star" size={16} fill={fav} />
    </span>
    {#if s.duration}<span class="dur">{clock(s.duration)}</span>{/if}
    {#if !s.video_path}<span class="novideo">{s.record_mode === "clips_only" ? "Clips only" : s.video_removed ? "Video removed" : "No video"}</span>{/if}
  </div>
  <div class="meta">
    <ChampionIcon id={s.player?.character_id} name={s.player?.character ?? s.game_name} size={38} />
    <div class="txt">
      <div class="title">{s.player?.character ?? s.game_name}</div>
      <div class="sub">
        {#if s.mode_name ?? s.player?.mode}<span class="modetag" title={s.queue_id ? `Queue ${s.queue_id}` : ""}>{s.mode_name ?? s.player?.mode}</span>{:else}{s.game_name}{/if}
      </div>
    </div>
    <div class="right">
      <div class="kda">{kda(s.stats)}</div>
      <div class="sub">{relativeDate(s.started_at)}</div>
    </div>
  </div>
</button>

<style>
  .game {
    display: flex;
    flex-direction: column;
    width: 100%;
    padding: 0;
    overflow: hidden;
    text-align: left;
    transition: transform 0.16s var(--ease), border-color 0.16s var(--ease), box-shadow 0.16s var(--ease);
  }
  .game:hover {
    transform: translateY(-2px);
    border-color: var(--border-2);
    box-shadow: var(--shadow);
  }
  .game:active {
    transform: translateY(0) scale(0.985);
    transition-duration: 0.06s;
  }
  img {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
    opacity: 0;
    transition: opacity 0.2s var(--ease);
  }
  img.shown {
    opacity: 1;
  }
  .ph-skel {
    position: absolute;
    inset: 0;
  }
  .thumb {
    position: relative;
    flex: none;
    aspect-ratio: 16 / 9;
    background: var(--media-placeholder);
  }
  .ph {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    color: var(--faint);
  }
  .shade {
    position: absolute;
    inset: 0;
    background: var(--media-shade);
  }
  .badge {
    position: absolute;
    left: 10px;
    top: 10px;
    padding: 3px 9px;
    border-radius: 6px;
    font-size: 11.5px;
    font-weight: 700;
    letter-spacing: 0.03em;
    text-transform: uppercase;
  }
  .badge.win {
    background: var(--media-win-bg);
    color: var(--media-win-text);
    border: 1px solid var(--media-win-border);
  }
  .badge.loss {
    background: var(--media-loss-bg);
    color: var(--media-loss-text);
    border: 1px solid var(--media-loss-border);
  }
  .badge.draw {
    background: var(--media-overlay);
    color: var(--on-media-2);
  }
  .fav {
    position: absolute;
    right: 8px;
    top: 8px;
    width: 30px;
    height: 30px;
    border-radius: 8px;
    display: grid;
    place-items: center;
    color: var(--on-media-2);
    background: var(--media-overlay);
    opacity: 0;
    transition: opacity 0.15s;
  }
  .game:hover .fav,
  .fav.on {
    opacity: 1;
  }
  .fav.on {
    color: var(--media-fav);
  }
  .dur,
  .novideo {
    position: absolute;
    right: 10px;
    bottom: 8px;
    font-size: 12px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    color: var(--on-media);
    text-shadow: var(--media-text-shadow);
  }
  .novideo {
    left: 10px;
    right: auto;
    color: var(--media-warn);
  }
  .meta {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 11px;
    padding: 11px 12px 12px;
  }
  .txt {
    flex: 1;
    min-width: 0;
  }
  .title {
    font-weight: 650;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .sub {
    font-size: 12px;
    color: var(--muted);
    margin-top: 2px;
    white-space: nowrap;
  }
  .modetag {
    display: inline-block;
    max-width: 100%;
    padding: 1px 7px;
    border-radius: 5px;
    background: var(--surface-2);
    border: 1px solid var(--border);
    color: var(--text-2);
    font-size: 11.5px;
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    vertical-align: middle;
  }
  .right {
    text-align: right;
  }
  .kda {
    font-weight: 700;
    font-variant-numeric: tabular-nums;
  }
</style>
