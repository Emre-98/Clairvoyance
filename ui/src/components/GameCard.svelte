<script lang="ts">
  import type { SessionSummary } from "../lib/types";
  import { fileSrc, api } from "../lib/api";
  import { clock, kda, relativeDate } from "../lib/format";
  import { go, prefetchSession } from "../lib/store.svelte";
  import ChampionIcon from "./ChampionIcon.svelte";
  import Icon from "./Icon.svelte";

  let { s }: { s: SessionSummary } = $props();
  let favLocal = $state<boolean | null>(null);
  const fav = $derived(favLocal ?? s.favorite);

  const thumb = $derived(
    s.thumb_path
      ? fileSrc(s.thumb_path)
      : s.game_id === "league" && s.player?.character_id
        ? `https://ddragon.leagueoflegends.com/cdn/img/champion/splash/${s.player.character_id}_0.jpg`
        : null,
  );
  const resultLabel = $derived(s.result === "win" ? "Victory" : s.result === "loss" ? "Defeat" : s.result === "draw" ? "Draw" : null);

  async function toggleFav(e: MouseEvent) {
    e.stopPropagation();
    favLocal = !fav;
    await api.setFavorite(s.id, favLocal);
  }
</script>

<button class="card game" onpointerenter={() => prefetchSession(s.id)} onfocus={() => prefetchSession(s.id)} onclick={() => go({ page: "game", id: s.id })}>
  <div class="thumb" style={thumb ? `background-image:url('${thumb}')` : ""}>
    {#if !thumb}<div class="ph"><Icon name="gamepad" size={34} stroke={1.5} /></div>{/if}
    <div class="shade"></div>
    {#if resultLabel}<span class="badge {s.result}">{resultLabel}</span>{/if}
    <span class="fav" class:on={fav} role="button" tabindex="-1" title={fav ? "Favorite (never auto-deleted)" : "Add to favorites"} onclick={toggleFav} onkeydown={() => {}}>
      <Icon name="star" size={16} fill={fav} />
    </span>
    {#if s.duration}<span class="dur">{clock(s.duration)}</span>{/if}
    {#if !s.video_path}<span class="novideo">No video</span>{/if}
  </div>
  <div class="meta">
    <ChampionIcon id={s.player?.character_id} name={s.player?.character ?? s.game_name} size={38} />
    <div class="txt">
      <div class="title">{s.player?.character ?? s.game_name}</div>
      <div class="sub">{s.player?.mode ?? s.game_name}</div>
    </div>
    <div class="right">
      <div class="kda">{kda(s.stats)}</div>
      <div class="sub">{relativeDate(s.started_at)}</div>
    </div>
  </div>
</button>

<style>
  .game {
    padding: 0;
    overflow: hidden;
    text-align: left;
    transition: transform 0.15s, border-color 0.15s, box-shadow 0.15s;
  }
  .game:hover {
    transform: translateY(-2px);
    border-color: var(--border-2);
    box-shadow: var(--shadow);
  }
  .thumb {
    position: relative;
    aspect-ratio: 16 / 9;
    background: linear-gradient(135deg, #1f2638, #121622) center / cover no-repeat;
  }
  .ph {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    color: #39415a;
  }
  .shade {
    position: absolute;
    inset: 0;
    background: linear-gradient(180deg, rgba(0, 0, 0, 0.25) 0%, transparent 35%, transparent 60%, rgba(0, 0, 0, 0.55) 100%);
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
    backdrop-filter: blur(6px);
  }
  .badge.win {
    background: rgba(61, 220, 132, 0.2);
    color: #7cf0b0;
    border: 1px solid rgba(61, 220, 132, 0.4);
  }
  .badge.loss {
    background: rgba(255, 77, 109, 0.2);
    color: #ff9cb0;
    border: 1px solid rgba(255, 77, 109, 0.4);
  }
  .badge.draw {
    background: rgba(200, 200, 200, 0.2);
    color: #ddd;
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
    color: rgba(255, 255, 255, 0.75);
    background: rgba(0, 0, 0, 0.35);
    opacity: 0;
    transition: opacity 0.15s;
  }
  .game:hover .fav,
  .fav.on {
    opacity: 1;
  }
  .fav.on {
    color: #f5c542;
  }
  .dur,
  .novideo {
    position: absolute;
    right: 10px;
    bottom: 8px;
    font-size: 12px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    color: #fff;
    text-shadow: 0 1px 3px rgba(0, 0, 0, 0.6);
  }
  .novideo {
    left: 10px;
    right: auto;
    color: #ffcf7a;
  }
  .meta {
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
  .right {
    text-align: right;
  }
  .kda {
    font-weight: 700;
    font-variant-numeric: tabular-nums;
  }
</style>
