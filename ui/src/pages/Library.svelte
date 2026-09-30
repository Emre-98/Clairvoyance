<script lang="ts">
  import { keepScroll } from "../lib/scroll";
  import { app } from "../lib/store.svelte";
  import GameCard from "../components/GameCard.svelte";
  import Icon from "../components/Icon.svelte";
  import { bytes } from "../lib/format";

  let q = $state("");
  let game = $state("all");
  let result = $state("all");
  let favOnly = $state(false);
  let sort = $state("new");

  const games = $derived([...new Set(app.sessions.map((s) => s.game_name))]);
  const list = $derived.by(() => {
    const needle = q.trim().toLowerCase();
    let l = app.sessions.filter(
      (s) =>
        (game === "all" || s.game_name === game) &&
        (result === "all" || s.result === result) &&
        (!favOnly || s.favorite) &&
        (!needle || `${s.player?.character ?? ""} ${s.player?.mode ?? ""} ${s.game_name}`.toLowerCase().includes(needle)),
    );
    if (sort === "old") l = [...l].reverse();
    if (sort === "long") l = [...l].sort((a, b) => (b.duration ?? 0) - (a.duration ?? 0));
    if (sort === "big") l = [...l].sort((a, b) => b.size_bytes - a.size_bytes);
    return l;
  });
  const total = $derived(list.reduce((a, s) => a + s.size_bytes, 0));
</script>

<div class="page" use:keepScroll={"games"}>
  <div class="page-head">
    <div>
      <h1>Games</h1>
      <p class="muted" style="margin:6px 0 0">{list.length} game{list.length === 1 ? "" : "s"} · {bytes(total)}</p>
    </div>
  </div>

  <div class="filters">
    <div class="search">
      <Icon name="search" size={16} />
      <input class="input" placeholder="Search champion or mode" bind:value={q} />
    </div>
    {#if games.length > 1}
      <select class="input" bind:value={game}>
        <option value="all">All games</option>
        {#each games as g}<option value={g}>{g}</option>{/each}
      </select>
    {/if}
    <div class="seg">
      {#each [["all", "All"], ["win", "Wins"], ["loss", "Losses"]] as [v, l]}
        <button class:on={result === v} onclick={() => (result = v)}>{l}</button>
      {/each}
    </div>
    <button class="btn" class:favon={favOnly} onclick={() => (favOnly = !favOnly)}><Icon name="star" size={15} fill={favOnly} />Favorites</button>
    <div class="spacer"></div>
    <select class="input" bind:value={sort}>
      <option value="new">Newest first</option>
      <option value="old">Oldest first</option>
      <option value="long">Longest</option>
      <option value="big">Largest files</option>
    </select>
  </div>

  {#if list.length === 0}
    <div class="empty">
      <Icon name="library" size={34} stroke={1.5} />
      <strong>{app.sessions.length ? "No games match these filters" : "No games yet"}</strong>
    </div>
  {:else}
    <div class="grid">
      {#each list as s (s.id)}<GameCard {s} />{/each}
    </div>
  {/if}
</div>

<style>
  .filters {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-bottom: 20px;
    flex-wrap: wrap;
  }
  .search {
    position: relative;
    display: flex;
    align-items: center;
    color: var(--muted);
  }
  .search :global(svg) {
    position: absolute;
    left: 10px;
  }
  .search input {
    padding-left: 34px;
    width: 240px;
  }
  .seg {
    display: flex;
    background: var(--bg-2);
    border: 1px solid var(--border-2);
    border-radius: var(--radius-sm);
    padding: 3px;
  }
  .seg button {
    border: none;
    background: transparent;
    color: var(--muted);
    padding: 5px 12px;
    border-radius: 6px;
    font-weight: 600;
    font-size: 13px;
  }
  .seg button.on {
    background: var(--surface-3);
    color: var(--text);
  }
  .favon {
    color: var(--fav);
    border-color: color-mix(in srgb, var(--fav) 40%, transparent);
  }
</style>
