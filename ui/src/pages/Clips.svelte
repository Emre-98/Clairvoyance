<script lang="ts" module>
  import type { ClipEntry as CE } from "../lib/types";
  // Kept between visits, so the page shows at once and refreshes in the background.
  let cache: CE[] | null = null;
</script>

<script lang="ts">
  import { keepScroll } from "../lib/scroll";
  import { app, go, toast } from "../lib/store.svelte";
  import { api, fileSrc, confirmDialog } from "../lib/api";
  import { bytes, relativeDate } from "../lib/format";
  import type { ClipEntry } from "../lib/types";
  import Icon from "../components/Icon.svelte";
  import ChampionIcon from "../components/ChampionIcon.svelte";

  let clips = $state<ClipEntry[]>(cache ?? []);
  let loaded = $state(cache != null);
  let playing = $state<ClipEntry | null>(null);
  let source = $state("all");

  $effect(() => {
    app.libraryVersion;
    api.listClips().then((c) => {
      clips = c;
      cache = c;
      loaded = true;
    });
  });

  const shown = $derived(clips.filter((c) => source === "all" || c.source === source));
  const sourceLabel: Record<string, string> = { replay: "Hotkey", event: "Auto", editor: "Edited" };

  async function remove(c: ClipEntry) {
    if (!(await confirmDialog(`Delete the clip "${c.title}"? This can't be undone.`))) return;
    try {
      await api.deleteClip(c.session_id, c.path.split(/[\\/]/).pop()!);
      toast("Clip deleted", "ok");
      if (playing === c) playing = null;
    } catch (e) {
      toast(String(e), "error");
    }
  }

  function hover(e: MouseEvent, play: boolean) {
    const v = (e.currentTarget as HTMLElement).querySelector("video");
    if (!v) return;
    if (play) v.play().catch(() => {});
    else {
      v.pause();
      v.currentTime = 0;
    }
  }
</script>

<div class="page" use:keepScroll={"clips"}>
  <div class="page-head">
    <div>
      <h1>Clips</h1>
      <p class="muted" style="margin:6px 0 0">Saved with <kbd>{app.settings?.hotkey_clip}</kbd>, made automatically from big moments, or cut in the clip editor.</p>
    </div>
    <div class="seg">
      {#each [["all", "All"], ["replay", "Hotkey"], ["event", "Auto"], ["editor", "Edited"]] as [v, l]}
        <button class:on={source === v} onclick={() => (source = v)}>{l}</button>
      {/each}
    </div>
  </div>

  {#if loaded && shown.length === 0}
    <div class="empty">
      <Icon name="scissors" size={34} stroke={1.5} />
      <strong>No clips yet</strong>
      <span>Press <kbd>{app.settings?.hotkey_clip}</kbd> in game to save the last {app.settings?.video.replay_buffer_secs ?? 30} seconds.</span>
    </div>
  {:else}
    <div class="grid">
      {#each shown as c (c.path + c.title + c.created_at)}
        <div class="card clip fade-in" onmouseenter={(e) => hover(e, true)} onmouseleave={(e) => hover(e, false)} role="group">
          <button class="thumb" onclick={() => (playing = c)} aria-label="Play {c.title}">
            <video src={fileSrc(c.path) + "#t=0.5"} muted preload="metadata" loop playsinline></video>
            <span class="src">{sourceLabel[c.source] ?? c.source}</span>
            <span class="playbtn"><Icon name="play" size={22} fill /></span>
          </button>
          <div class="meta">
            <ChampionIcon id={c.character_id} name={c.character ?? c.game_name} size={32} />
            <div class="txt">
              <div class="title" title={c.title}>{c.title}</div>
              <div class="sub">{relativeDate(c.created_at)} · {bytes(c.size_bytes)}</div>
            </div>
          </div>
          <div class="actions">
            <button class="btn ghost small" onclick={() => go({ page: "game", id: c.session_id })}>Game</button>
            <button class="btn ghost small" onclick={() => api.reveal(c.path)}><Icon name="folder" size={14} />Show</button>
            <div class="spacer"></div>
            <button class="btn ghost small icon" title="Delete" onclick={() => remove(c)}><Icon name="trash" size={15} /></button>
          </div>
        </div>
      {/each}
    </div>
  {/if}
</div>

{#if playing}
  <div class="modal" role="dialog" tabindex="-1" onclick={() => (playing = null)} onkeydown={(e) => e.key === "Escape" && (playing = null)}>
    <div class="modal-body" role="presentation" onclick={(e) => e.stopPropagation()}>
      <div class="row" style="margin-bottom:10px">
        <h3>{playing.title}</h3>
        <div class="spacer"></div>
        <button class="btn small" onclick={() => api.openPath(playing!.path)}><Icon name="external" size={14} />Open in player</button>
        <button class="btn small icon" onclick={() => (playing = null)}><Icon name="x" size={15} /></button>
      </div>
      <!-- svelte-ignore a11y_media_has_caption -->
      <video src={fileSrc(playing.path)} controls autoplay></video>
    </div>
  </div>
{/if}

<style>
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
  .clip {
    overflow: hidden;
  }
  .thumb {
    position: relative;
    display: block;
    width: 100%;
    aspect-ratio: 16 / 9;
    padding: 0;
    border: none;
    background: var(--media-bg);
  }
  .thumb video {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }
  .src {
    position: absolute;
    left: 8px;
    top: 8px;
    font-size: 11px;
    font-weight: 700;
    padding: 2px 7px;
    border-radius: 5px;
    background: var(--media-overlay);
    color: var(--on-media);
  }
  .playbtn {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    color: var(--on-media);
    opacity: 0;
    transition: opacity 0.15s;
    background: color-mix(in srgb, var(--media-bg) 25%, transparent);
  }
  .clip:hover .playbtn {
    opacity: 1;
  }
  .meta {
    display: flex;
    gap: 10px;
    align-items: center;
    padding: 10px 12px 4px;
  }
  .txt {
    min-width: 0;
    flex: 1;
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
  }
  .actions {
    display: flex;
    gap: 2px;
    padding: 4px 8px 8px;
  }
  .modal {
    position: fixed;
    inset: 0;
    background: var(--scrim);
    display: grid;
    place-items: center;
    z-index: 50;
    backdrop-filter: blur(4px);
  }
  .modal-body {
    width: min(1100px, 90vw);
    background: var(--surface);
    border: 1px solid var(--border-2);
    border-radius: var(--radius);
    padding: 14px;
  }
  .modal-body video {
    width: 100%;
    max-height: 72vh;
    background: var(--media-bg);
    border-radius: 8px;
  }
</style>
