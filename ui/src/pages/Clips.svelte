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
  import VirtualGrid from "../components/VirtualGrid.svelte";

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

  // Hover preview: the video element only exists for the card under the mouse (after a short
  // delay), so scrolling past hundreds of clips never loads any video.
  let previewKey = $state<string | null>(null);
  let hoverTimer = 0;
  const keyOf = (c: ClipEntry) => c.session_id + "/" + c.file;
  function hover(c: ClipEntry, on: boolean) {
    clearTimeout(hoverTimer);
    if (on) hoverTimer = window.setTimeout(() => (previewKey = keyOf(c)), 350);
    else if (previewKey === keyOf(c)) previewKey = null;
  }

  async function toggleKeep(c: ClipEntry) {
    const keep = !c.keep;
    c.keep = keep; // instant; confirmed by the library refresh
    try {
      await api.setClipKeep(c.session_id, c.file, keep);
      toast(keep ? "Clip kept: the storage clean-up will never delete it" : "Clip no longer kept", "ok");
    } catch (e) {
      c.keep = !keep;
      toast(String(e), "error");
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

  {#if !loaded}
    <div class="grid">{#each Array(6) as _}<div class="card"><div class="skeleton" style="aspect-ratio:16/9"></div><div style="padding:14px 12px"><span class="skeleton-line" style="width:70%"></span></div></div>{/each}</div>
  {:else if shown.length === 0}
    <div class="empty">
      <Icon name="scissors" size={34} stroke={1.5} />
      <strong>No clips yet</strong>
      <span>Press <kbd>{app.settings?.hotkey_clip}</kbd> in game to save the last {app.settings?.video.replay_buffer_secs ?? 30} seconds.</span>
    </div>
  {:else}
    <VirtualGrid items={shown} key={(c) => keyOf(c) + c.created_at} extraHeight={90}>
      {#snippet item(c)}
        <div class="card clip" onmouseenter={() => hover(c, true)} onmouseleave={() => hover(c, false)} role="group">
          <button class="thumb" onclick={() => (playing = c)} aria-label="Play {c.title}">
            {#if c.thumb_path}
              <img src={fileSrc(c.thumb_path)} alt="" loading="lazy" decoding="async" draggable="false" />
            {:else}
              <span class="ph"><Icon name="film" size={30} stroke={1.5} /></span>
            {/if}
            {#if previewKey === keyOf(c)}
              <video src={fileSrc(c.path)} muted autoplay loop playsinline preload="auto"></video>
            {/if}
            <span class="src">{sourceLabel[c.source] ?? c.source}</span>
            {#if c.keep}<span class="kept" title="Kept: never deleted by the storage clean-up"><Icon name="pin" size={12} />Kept</span>{/if}
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
            <button class="btn ghost small icon keepbtn" class:on={c.keep} title={c.keep ? "Kept (click to un-keep)" : "Keep: never auto-delete this clip"} aria-pressed={c.keep} onclick={() => toggleKeep(c)}><Icon name="pin" size={15} /></button>
            <button class="btn ghost small icon" title="Delete" onclick={() => remove(c)}><Icon name="trash" size={15} /></button>
          </div>
        </div>
      {/snippet}
    </VirtualGrid>
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
    flex: none;
    display: block;
    width: 100%;
    aspect-ratio: 16 / 9;
    padding: 0;
    border: none;
    background: var(--media-bg);
  }
  .clip {
    display: flex;
    flex-direction: column;
  }
  .thumb img,
  .thumb video {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }
  .thumb .ph {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    color: var(--on-media-2);
  }
  .kept {
    position: absolute;
    right: 8px;
    top: 8px;
    display: inline-flex;
    gap: 4px;
    align-items: center;
    font-size: 11px;
    font-weight: 700;
    padding: 2px 7px;
    border-radius: 5px;
    background: var(--media-overlay);
    color: var(--media-fav);
  }
  .keepbtn.on {
    color: var(--fav);
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
