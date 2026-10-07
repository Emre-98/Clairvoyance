<script lang="ts">
  // The scoreboard at a moment of the game: all 10 players with champion, level, KDA, CS,
  // summoner spells and items, following the playback time (scrubbing included). Recordings made
  // before v1.8 have no snapshots: they show the own player's final numbers only.
  import type { PlayerInfo, PlayerStats, Scoreboard } from "../lib/types";
  import { build, frameAt, shortName } from "../lib/scoreboard";
  import { clock } from "../lib/format";
  import GameIcon from "./GameIcon.svelte";
  import ChampionIcon from "./ChampionIcon.svelte";

  let {
    sb = null,
    t,
    player = null,
    stats = null,
    overlay = false,
  }: {
    sb?: Scoreboard | null;
    /** Game time (seconds) to show. */
    t: number;
    /** Old recordings: the own player's final numbers. */
    player?: PlayerInfo | null;
    stats?: PlayerStats | null;
    /** Drawn over the video (see-through, no card frame). */
    overlay?: boolean;
  } = $props();

  const tl = $derived(sb && sb.frames.length ? build(sb) : null);
  const idx = $derived(tl ? frameAt(tl, t) : -1);
  const states = $derived(tl && idx >= 0 ? tl.states[idx] : null);
  const teams = $derived.by(() => {
    if (!sb || !states) return [];
    const order = [...new Set(sb.players.map((p) => p.team))];
    return order.map((team) => {
      const rows = sb.players.map((p, i) => ({ p, s: states[i], i })).filter((r) => r.p.team === team);
      const kills = rows.reduce((n, r) => n + r.s.kills, 0);
      return { team, rows, kills, label: team === "ORDER" ? "Blue team" : team === "CHAOS" ? "Red team" : team };
    });
  });
  /** 6 item slots + the trinket. */
  const slots = (items: number[]) => Array.from({ length: 7 }, (_, k) => items[k] ?? 0);
  const name = (id: string | number) => sb?.names[String(id)] ?? "";
  const ver = $derived(sb?.version ?? "");
</script>

<div class="sb" class:overlay data-testid="scoreboard" data-frame={idx}>
  {#if teams.length}
    <div class="head">
      <strong>Scoreboard</strong>
      <span class="muted">at {clock(Math.max(0, t))} · follows the video{tl && idx >= 0 ? ` · read at ${clock(tl.times[idx])}` : ""}</span>
    </div>
    <div class="teams">
      {#each teams as tm (tm.team)}
        <div class="team" class:red={tm.team === "CHAOS"}>
          <div class="thead"><span class="tname">{tm.label}</span><span class="tk">{tm.kills} kills</span></div>
          {#each tm.rows as r (r.i)}
            <div class="row" class:me={r.p.me} data-testid="sb-row">
              <span class="champ" title={r.p.character}>
                <ChampionIcon id={r.p.character_id} name={r.p.character} size={28} />
                <span class="lv" title="Level">{r.s.level}</span>
              </span>
              <span class="who">
                <span class="pn" title={r.p.name}>{shortName(r.p.name)}</span>
                <span class="cn muted" title={r.p.character}>{r.p.character}</span>
              </span>
              <span class="spells">
                {#each r.s.spells as sp, k (k)}
                  {#if sp}<GameIcon icon={{ kind: "spell", id: sp, version: ver }} size={18} title={name(sp) || sp} />{:else}<span class="slot sm"></span>{/if}
                {/each}
              </span>
              <span class="kda" title="Kills / deaths / assists">{r.s.kills}/{r.s.deaths}/{r.s.assists}</span>
              <span class="cs" title="Creep score">{r.s.cs}</span>
              <span class="items">
                {#each slots(r.s.items) as it, k (k)}
                  {#if it}
                    <GameIcon icon={{ kind: "item", id: String(it), version: ver }} size={k === 6 ? 18 : 20} title={name(it) || `Item ${it}`} />
                  {:else}
                    <span class="slot" class:trinket={k === 6}></span>
                  {/if}
                {/each}
              </span>
            </div>
          {/each}
        </div>
      {/each}
    </div>
  {:else if player || stats}
    <div class="head"><strong>Final scoreboard</strong><span class="muted">recorded before v1.8: only your own final numbers were saved</span></div>
    <div class="row me single" data-testid="sb-row">
      <span class="champ"><ChampionIcon id={player?.character_id} name={player?.character} size={28} />{#if stats?.level}<span class="lv">{stats.level}</span>{/if}</span>
      <span class="who"><span class="pn">{shortName(player?.name ?? "You")}</span><span class="cn muted">{player?.character ?? ""}</span></span>
      <span class="spells"></span>
      <span class="kda">{stats ? `${stats.kills}/${stats.deaths}/${stats.assists}` : "-"}</span>
      <span class="cs">{stats?.cs ?? "-"}</span>
      <span class="items muted">{stats?.gold != null ? `${stats.gold.toLocaleString()} gold` : ""}</span>
    </div>
  {/if}
</div>

<style>
  .sb {
    container-type: inline-size;
    font-size: 12.5px;
    color: var(--text);
  }
  .head {
    display: flex;
    align-items: baseline;
    gap: 10px;
    margin-bottom: 8px;
    flex-wrap: wrap;
  }
  .head strong {
    font-size: 14px;
  }
  .teams {
    display: grid;
    gap: 12px;
  }
  @container (min-width: 900px) {
    .teams {
      grid-template-columns: 1fr 1fr;
    }
  }
  .team {
    display: grid;
    gap: 2px;
    border-left: 3px solid var(--ev-assist);
    padding-left: 8px;
    min-width: 0;
  }
  .team.red {
    border-left-color: var(--ev-death);
  }
  .thead {
    display: flex;
    justify-content: space-between;
    font-weight: 700;
    font-size: 11px;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--muted);
    padding: 0 4px 4px;
  }
  .row {
    display: grid;
    grid-template-columns: 30px minmax(60px, 1fr) 40px 62px 36px auto;
    align-items: center;
    gap: 8px;
    padding: 3px 4px;
    border-radius: 6px;
    min-width: 0;
  }
  .row.me {
    background: color-mix(in srgb, var(--accent) 14%, transparent);
  }
  .champ {
    position: relative;
    width: 28px;
    height: 28px;
  }
  .lv {
    position: absolute;
    right: -4px;
    bottom: -3px;
    min-width: 14px;
    padding: 0 2px;
    border-radius: 7px;
    font-size: 9.5px;
    font-weight: 700;
    line-height: 13px;
    text-align: center;
    background: var(--surface);
    color: var(--text);
    box-shadow: 0 0 0 1px var(--border-2);
  }
  .who {
    display: grid;
    min-width: 0;
    line-height: 1.2;
  }
  .pn,
  .cn {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .pn {
    font-weight: 600;
  }
  .cn {
    font-size: 11px;
  }
  .spells,
  .items {
    display: flex;
    gap: 2px;
    align-items: center;
  }
  .kda,
  .cs {
    font-variant-numeric: tabular-nums;
    text-align: right;
  }
  .slot {
    width: 20px;
    height: 20px;
    border-radius: 5px;
    background: color-mix(in srgb, var(--text) 7%, transparent);
  }
  .slot.trinket,
  .slot.sm {
    width: 18px;
    height: 18px;
  }
  .slot.trinket {
    border-radius: 50%;
  }
  .overlay {
    background: color-mix(in srgb, var(--bg) 82%, transparent);
    backdrop-filter: blur(6px);
    border: 1px solid var(--border-2);
    border-radius: 12px;
    padding: 12px 14px;
    box-shadow: var(--shadow);
  }
</style>
