<script lang="ts">
  // An item / summoner spell / champion picture from Riot's public Data Dragon CDN (only while the
  // window is open). The event keeps the Data Dragon version of its game, so items removed
  // from League later still show; without one the newest version is used.
  import type { EventIcon } from "../lib/types";
  import { fetchVersion } from "./ChampionIcon.svelte";
  import Icon from "./Icon.svelte";

  let {
    icon,
    size = 24,
    title = "",
    round = false,
    fallback = null,
  }: {
    icon: EventIcon;
    size?: number;
    title?: string;
    round?: boolean;
    /** Shown when the picture can't load (offline): an icon name and its colour. */
    fallback?: { icon: string; color: string } | null;
  } = $props();

  let latest = $state<string | null>(null);
  let failed = $state(false);
  $effect(() => {
    if (!icon.version && !latest) fetchVersion().then((v) => (latest = v));
  });
  const dir = $derived(icon.kind === "item" ? "item" : icon.kind === "spell" ? "spell" : "champion");
  const version = $derived(icon.version || latest);
  const src = $derived(version ? `https://ddragon.leagueoflegends.com/cdn/${version}/img/${dir}/${encodeURIComponent(icon.id)}.png` : null);
  $effect(() => {
    void src;
    failed = false;
  });
</script>

<span class="gi" class:round style="width:{size}px;height:{size}px" {title} data-src={src}>
  {#if src && !failed}
    <img {src} alt={title} onerror={() => (failed = true)} draggable="false" loading="lazy" />
  {:else if fallback}
    <span class="fb" style="background:{fallback.color}"><Icon name={fallback.icon} size={Math.round(size * 0.55)} stroke={2.4} /></span>
  {/if}
</span>

<style>
  .gi {
    display: inline-block;
    flex: none;
    border-radius: 5px;
    overflow: hidden;
    background: var(--surface-3);
    box-shadow: inset 0 0 0 1px var(--border-2);
    vertical-align: middle;
  }
  .gi.round {
    border-radius: 50%;
  }
  .fb {
    width: 100%;
    height: 100%;
    display: grid;
    place-items: center;
    color: var(--on-ev);
  }
  img {
    width: 100%;
    height: 100%;
    display: block;
  }
</style>
