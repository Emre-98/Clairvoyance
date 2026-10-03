<script lang="ts">
  // Settings > Game modes: which modes get recorded. The list comes from the app (League: the
  // client's queue list + Riot's list, cached), so new and rotating modes appear by themselves.
  // Every change is saved at once and applies to the next game.
  import { api } from "../lib/api";
  import { app, toast } from "../lib/store.svelte";
  import { on } from "../lib/api";
  import { relativeDate } from "../lib/format";
  import type { GameModesView, ModeEntry, ModeRule } from "../lib/types";
  import Icon from "./Icon.svelte";
  import RuleSwitch from "./RuleSwitch.svelte";

  let views = $state<GameModesView[] | null>(null);
  let refreshing = $state(false);
  let showGone = $state<Record<string, boolean>>({});

  function recount() {
    app.newModes = (views ?? []).reduce((n, g) => n + Object.values(g.modes.entries).filter((e) => e.is_new).length, 0);
  }
  async function load() {
    views = await api.modesGet();
    recount();
  }
  async function refresh(quiet = false) {
    refreshing = true;
    try {
      views = await api.modesRefresh();
      recount();
    } catch (e) {
      if (!quiet) toast(String(e), "error");
    } finally {
      refreshing = false;
    }
  }
  $effect(() => {
    load().then(() => refresh(true));
    let un: (() => void) | null = null;
    on("modes-changed", () => load()).then((u) => (un = u));
    return () => un?.();
  });

  async function set(game: string, what: "mode" | "group" | "unknown" | "preset" | "seen", key?: string | null, rule?: ModeRule | null) {
    try {
      views = await api.modesSet(game, what, key, rule);
      recount();
    } catch (e) {
      toast(String(e), "error");
    }
  }

  type Row = { key: string; e: ModeEntry };
  function rows(v: GameModesView, group: string): { now: Row[]; gone: Row[] } {
    const all = Object.entries(v.modes.entries)
      .filter(([, e]) => e.group === group)
      .map(([key, e]) => ({ key, e }))
      .sort((a, b) => Number(b.e.is_new) - Number(a.e.is_new) || a.e.name.localeCompare(b.e.name));
    return { now: all.filter((r) => r.e.available !== false), gone: all.filter((r) => r.e.available === false) };
  }
  function groupRule(v: GameModesView, group: string): { rule: ModeRule | null; mixed: boolean } {
    const rs = [...new Set(Object.values(v.modes.entries).filter((e) => e.group === group && e.available !== false).map((e) => e.rule))];
    return { rule: rs.length === 1 ? rs[0] : null, mixed: rs.length > 1 };
  }
  const newCount = (v: GameModesView) => Object.values(v.modes.entries).filter((e) => e.is_new).length;
  const inGame = $derived(app.status?.state !== "idle");
</script>

{#if !views}
  <div class="card box"><span class="skeleton-line" style="width:50%"></span></div>
{:else if views.length === 0}
  <p class="muted">None of the supported games has game modes to choose from.</p>
{:else}
  {#each views as v (v.game_id)}
    <div class="toolbar">
      <div class="presets">
        <span class="muted small">Quick presets</span>
        <button class="btn small" onclick={() => set(v.game_id, "preset", "everything")}>Everything</button>
        <button class="btn small" onclick={() => set(v.game_id, "preset", "ranked")}>Ranked only</button>
        <button class="btn small" onclick={() => set(v.game_id, "preset", "ranked_normal")}>Ranked + Normal</button>
      </div>
      <div class="spacer"></div>
      <span class="muted small">{v.modes.catalog_updated_at ? `List from the League client, ${relativeDate(v.modes.catalog_updated_at).toLowerCase()}` : "Start the League client to get the live list"}</span>
      <button class="btn small ghost" onclick={() => refresh()} disabled={refreshing || inGame} title={inGame ? "After the game" : "Ask the League client and Riot for the current modes"}>
        <Icon name="download" size={13} />{refreshing ? "Updating…" : "Update list"}
      </button>
    </div>

    {#if newCount(v) > 0}
      <div class="newbar">
        <span class="newbadge">New</span>
        <span>{newCount(v)} new mode{newCount(v) === 1 ? "" : "s"} detected. They follow the "Unknown / new modes" rule until you choose.</span>
        <div class="spacer"></div>
        <button class="btn small ghost" onclick={() => set(v.game_id, "seen")}>Mark as seen</button>
      </div>
    {/if}

    {#each v.groups as g (g.id)}
      {@const r = rows(v, g.id)}
      {@const gr = groupRule(v, g.id)}
      {#if r.now.length || r.gone.length}
        <div class="card group">
          <div class="ghead">
            <div>
              <strong>{g.label}</strong>
              <span class="muted small">{g.help}</span>
            </div>
            <div class="spacer"></div>
            <span class="muted small">All:</span>
            <RuleSwitch value={gr.rule} mixed={gr.mixed} label="All {g.label} modes" onchange={(rule) => set(v.game_id, "group", g.id, rule)} />
          </div>
          {#each r.now as { key, e } (key)}
            <div class="mode">
              <div class="mname">
                <span title={e.name}>{e.name}</span>
                {#if e.is_new}<span class="newbadge">New mode detected</span>{/if}
                {#if e.queue_id}<span class="qid" title="Queue id">#{e.queue_id}</span>{/if}
              </div>
              <RuleSwitch value={e.rule} label={e.name} onchange={(rule) => set(v.game_id, "mode", key, rule)} />
            </div>
          {/each}
          {#if r.gone.length}
            <button class="gone-toggle" onclick={() => (showGone[g.id] = !showGone[g.id])}>
              <Icon name={showGone[g.id] ? "back" : "next"} size={12} />{r.gone.length} not currently available
            </button>
            {#if showGone[g.id]}
              {#each r.gone as { key, e } (key)}
                <div class="mode gone">
                  <div class="mname">
                    <span title={e.name}>{e.name}</span>
                    <span class="gonetag">not currently available</span>
                    {#if e.queue_id}<span class="qid">#{e.queue_id}</span>{/if}
                  </div>
                  <RuleSwitch value={e.rule} label={e.name} onchange={(rule) => set(v.game_id, "mode", key, rule)} />
                </div>
              {/each}
            {/if}
          {/if}
        </div>
      {/if}
    {/each}

    <div class="card group unknown">
      <div class="ghead">
        <div>
          <strong>Unknown / new modes</strong>
          <span class="muted small">Modes Riot adds later, and games whose mode can't be detected. New ones appear above with a "New" badge.</span>
        </div>
        <div class="spacer"></div>
        <RuleSwitch value={v.modes.unknown_rule} label="Unknown and new modes" onchange={(rule) => set(v.game_id, "unknown", null, rule)} />
      </div>
    </div>
    <p class="muted small foot">
      Record = full video + timeline + clips. Clips only = the timeline, your hotkey clips and automatic clips of big moments, without the full video (much less disk).
      Off = not recorded at all (the tray shows why). The mode is read from the League client when the game starts, so a mode that's off is never recorded.
    </p>
  {/each}
{/if}

<style>
  .toolbar {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
    margin-bottom: 12px;
  }
  .presets {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
  }
  .small {
    font-size: 12px;
  }
  .newbar {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 9px 12px;
    margin-bottom: 12px;
    border-radius: var(--radius-sm);
    border: 1px solid color-mix(in srgb, var(--accent) 40%, var(--border));
    background: color-mix(in srgb, var(--accent) 7%, var(--surface));
    font-size: 13px;
  }
  .newbadge {
    font-size: 10.5px;
    font-weight: 800;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    padding: 2px 7px;
    border-radius: 5px;
    background: var(--accent-grad);
    color: var(--on-accent);
    white-space: nowrap;
  }
  .group {
    padding: 4px 16px 8px;
    margin-bottom: 12px;
  }
  .ghead {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 12px 0 10px;
  }
  .ghead > div:first-child {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .mode {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 7px 0;
    border-top: 1px solid var(--border);
  }
  .mname {
    flex: 1;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 2px 8px;
    min-width: 0;
    font-size: 13.5px;
  }
  /* A narrow window: the badges go under the name instead of shortening it. */
  .mname > span:first-child {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
    max-width: 100%;
  }
  .mname > span:not(:first-child) {
    flex: none;
  }
  .qid {
    font-size: 11px;
    color: var(--faint);
    font-variant-numeric: tabular-nums;
  }
  .gone {
    opacity: 0.55;
  }
  .gonetag {
    font-size: 11px;
    color: var(--muted);
    border: 1px solid var(--border-2);
    border-radius: 5px;
    padding: 1px 6px;
    white-space: nowrap;
  }
  .gone-toggle {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    border: none;
    background: transparent;
    color: var(--muted);
    font-size: 12px;
    padding: 8px 0 4px;
  }
  .gone-toggle:hover {
    color: var(--text-2);
  }
  .unknown {
    border-style: dashed;
  }
  .foot {
    line-height: 1.5;
    max-width: 760px;
  }
</style>
