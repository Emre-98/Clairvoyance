<script lang="ts">
  import { app, saveSettings, toast } from "../lib/store.svelte";
  import { api, on, pickFolder } from "../lib/api";
  import { bytes } from "../lib/format";
  import { KIND, USER_KINDS } from "../lib/eventmeta";
  import type { EventKind, Settings, StorageInfo } from "../lib/types";
  import Icon from "../components/Icon.svelte";
  import HotkeyInput from "../components/HotkeyInput.svelte";
  import BuiltinRecorder from "../components/BuiltinRecorder.svelte";
  import PerfTest from "../components/PerfTest.svelte";

  let { section = "setup" }: { section?: string } = $props();

  const sections = [
    { id: "setup", label: "Recorder", icon: "rec" },
    { id: "recording", label: "Recording", icon: "clips" },
    { id: "hotkeys", label: "Hotkeys", icon: "keyboard" },
    { id: "events", label: "Events & clips", icon: "bolt" },
    { id: "games", label: "Games", icon: "gamepad" },
    { id: "storage", label: "Storage", icon: "folder" },
    { id: "general", label: "General", icon: "settings" },
    { id: "performance", label: "Performance test", icon: "cpu" },
    { id: "advanced", label: "Advanced", icon: "settings" },
  ];
  let active = $state("setup");
  $effect(() => {
    active = section;
  });

  // Copy of the settings with every game's defaults filled in.
  function makeDraft(): Settings {
    const d = structuredClone($state.snapshot(app.settings!)) as Settings;
    for (const g of app.info?.games ?? []) d.games[g.id] = { ...g.default_config, ...(d.games[g.id] ?? {}) };
    return d;
  }
  let baseline = $state(makeDraft());
  let draft = $state(makeDraft());
  const dirty = $derived(JSON.stringify(draft) !== JSON.stringify(baseline));
  let saving = $state(false);

  async function save() {
    saving = true;
    try {
      await saveSettings(draft);
      baseline = makeDraft();
      draft = makeDraft();
      toast("Settings saved", "ok");
    } catch (e) {
      toast(`Couldn't save: ${e}`, "error");
    } finally {
      saving = false;
    }
  }

  function toggleKind(list: EventKind[], k: EventKind) {
    const i = list.indexOf(k);
    if (i >= 0) list.splice(i, 1);
    else list.push(k);
  }

  let storage = $state<StorageInfo | null>(null);
  let ff = $state<{ available: boolean; path?: string | null } | null>(null);
  $effect(() => {
    if (active === "storage") api.storageInfo().then((s) => (storage = s));
    if (active === "events") api.ffmpegStatus().then((s) => (ff = s));
  });

  let ffdl = $state<{ done: number; total: number } | null>(null);
  async function getFfmpeg() {
    const un = await on("ffmpeg-progress", (p) => (ffdl = p));
    try {
      ffdl = { done: 0, total: 0 };
      await api.ffmpegDownload();
      ff = await api.ffmpegStatus();
      toast("ffmpeg installed", "ok");
    } catch (e) {
      toast(String(e), "error", 9000);
    } finally {
      un();
      ffdl = null;
    }
  }

  async function chooseFolder() {
    const f = await pickFolder();
    if (f) draft.save_dir = f;
  }

  async function cleanNow() {
    if (dirty) await save();
    const removed = await api.applyRetentionNow();
    toast(removed.length ? `Deleted ${removed.length} old game${removed.length === 1 ? "" : "s"}` : "Nothing to delete", "ok");
    storage = await api.storageInfo();
  }

  let simSpeed = $state(1);
  let simLength = $state(180);
  async function simulate() {
    try {
      await api.simulateGame(simSpeed, simLength);
      toast("Simulated League game started. It records your desktop.", "ok", 7000);
    } catch (e) {
      toast(String(e), "error");
    }
  }
</script>

<div class="settings">
  <nav class="subnav">
    <h1>Settings</h1>
    {#each sections as s}
      <button class:on={active === s.id} onclick={() => (active = s.id)}><Icon name={s.icon} size={16} />{s.label}</button>
    {/each}
  </nav>

  <div class="content page">
    {#if active === "setup"}
      <h2>Recorder</h2>
      <p class="lead">Records by itself on your graphics card, so your game stays smooth. Nothing else to install: game audio only, with an optional mic track.</p>
      <div class="card box"><BuiltinRecorder /></div>
    {:else if active === "recording"}
      <h2>Recording</h2>
      <div class="card box form">
        <label class="check wide"><input type="checkbox" bind:checked={draft.auto_record} />Record games automatically</label>
        <label>Encoder
          <select class="input" bind:value={draft.video.encoder}>
            <option value="auto">Automatic ({app.info?.gpu?.vendor ?? "detect"})</option>
            <option value="nvenc">NVIDIA NVENC</option>
            <option value="amd">AMD AMF</option>
            <option value="qsv">Intel Quick Sync</option>
          </select>
          <small>Always a hardware (GPU) encoder, never CPU.</small>
        </label>
        <label>Quality
          <select class="input" bind:value={draft.video.quality}>
            <option value="standard">Standard (smaller files)</option>
            <option value="high">High (bigger files)</option>
          </select>
          <small>About 1.5–3 GB/hour at 1080p60 on Standard.</small>
        </label>
        <label>Resolution
          <select class="input" bind:value={draft.video.height}>
            <option value={0}>Same as screen</option>
            <option value={1440}>1440p</option>
            <option value={1080}>1080p</option>
            <option value={720}>720p</option>
          </select>
        </label>
        <label>Frame rate
          <select class="input" bind:value={draft.video.fps}>
            <option value={30}>30 fps</option>
            <option value={60}>60 fps</option>
            <option value={120}>120 fps</option>
          </select>
        </label>
        <label>Replay buffer (clip length)
          <select class="input" bind:value={draft.video.replay_buffer_secs}>
            {#each [15, 20, 30, 45, 60, 90, 120] as s}<option value={s}>{s} seconds</option>{/each}
          </select>
          <small>What the clip hotkey saves.</small>
        </label>
        <label class="check wide"><input type="checkbox" bind:checked={draft.video.record_mic} />Record my microphone <small>(its own audio track; the game's sound is always recorded on its own)</small></label>
        <label class="check wide"><input type="checkbox" bind:checked={draft.video.display_capture} />Capture the whole screen instead of the game window <small>(only if recordings come out black)</small></label>
      </div>
    {:else if active === "hotkeys"}
      <h2>Hotkeys</h2>
      <p class="lead">Work while a game is running, without stealing the key from the game.</p>
      <div class="card box list">
        <div class="item"><div><strong>Save clip</strong><span class="muted">Saves the last {draft.video.replay_buffer_secs} seconds</span></div><HotkeyInput bind:value={draft.hotkey_clip} /></div>
        <div class="item"><div><strong>Add marker</strong><span class="muted">Puts a marker on the timeline to find the moment later</span></div><HotkeyInput bind:value={draft.hotkey_marker} /></div>
      </div>
    {:else if active === "events"}
      <h2>Events & clips</h2>
      <h3 class="sub">Automatic clips</h3>
      <p class="lead">After a game, short clips are cut from the recording around these moments (a quick copy, no re-encoding).</p>
      <div class="kinds">
        {#each USER_KINDS as k}
          <button class="kind" class:on={draft.events.clip_kinds.includes(k)} style="--c:{KIND[k].color}" onclick={() => toggleKind(draft.events.clip_kinds, k)}>
            <span class="ki"><Icon name={KIND[k].icon} size={11} stroke={2.6} /></span>{KIND[k].label}
          </button>
        {/each}
      </div>
      <div class="card box form" style="margin-top:14px">
        <label>Seconds before<input class="input" type="number" min="1" max="60" bind:value={draft.events.clip_before_secs} /></label>
        <label>Seconds after<input class="input" type="number" min="1" max="60" bind:value={draft.events.clip_after_secs} /></label>
        <div class="wide ffrow">
          {#if ff?.available}
            <span class="ok"><Icon name="check" size={14} /> ffmpeg ready</span><span class="muted path">{ff.path}</span>
          {:else}
            <span class="warn">Clips need the free ffmpeg tool.</span>
            <button class="btn small primary" onclick={getFfmpeg} disabled={ffdl != null}><Icon name="download" size={13} />{ffdl ? `Downloading ${ffdl.total ? Math.round((ffdl.done / ffdl.total) * 100) + "%" : bytes(ffdl.done)}` : "Download ffmpeg (~100 MB)"}</button>
            <span class="muted">or set its path:</span>
          {/if}
          <input class="input grow" bind:value={draft.ffmpeg_path} placeholder="Path to ffmpeg.exe (optional)" />
        </div>
      </div>

      <h3 class="sub">Voice callouts</h3>
      <div class="card box form">
        <label class="check wide"><input type="checkbox" bind:checked={draft.events.tts_enabled} />Speak events out loud while playing (Windows text-to-speech)</label>
        <label>Volume<input type="range" min="0" max="100" bind:value={draft.events.tts_volume} /></label>
      </div>
      <div class="kinds" class:dim={!draft.events.tts_enabled} style="margin-top:12px">
        {#each USER_KINDS as k}
          <button class="kind" class:on={draft.events.tts_kinds.includes(k)} style="--c:{KIND[k].color}" onclick={() => toggleKind(draft.events.tts_kinds, k)}>
            <span class="ki"><Icon name={KIND[k].icon} size={11} stroke={2.6} /></span>{KIND[k].label}
          </button>
        {/each}
      </div>
    {:else if active === "games"}
      <h2>Games</h2>
      {#each app.info?.games ?? [] as g}
        {@const cfg = draft.games[g.id]}
        <div class="card box">
          <div class="row gh">
            <Icon name="gamepad" size={18} />
            <strong>{g.name}</strong>
            <span class="pill">{g.supports_events ? "Events + recording" : "Recording + manual clips"}</span>
            <div class="spacer"></div>
            <label class="check"><input type="checkbox" checked={!draft.disabled_games.includes(g.id)} onchange={(e) => { const on = (e.currentTarget as HTMLInputElement).checked; draft.disabled_games = on ? draft.disabled_games.filter((x) => x !== g.id) : [...draft.disabled_games, g.id]; }} />Enabled</label>
          </div>
          <div class="form">
            {#each g.config_fields as f}
              <label class:wide={f.kind !== "key"}>
                {f.label}
                {#if f.kind === "key"}
                  <HotkeyInput single bind:value={cfg[f.key] as string} />
                {:else if f.kind === "bool"}
                  <input type="checkbox" bind:checked={cfg[f.key] as boolean} />
                {:else}
                  <input class="input" bind:value={cfg[f.key] as string} />
                {/if}
                <small>{f.help}</small>
              </label>
            {/each}
          </div>
        </div>
      {/each}
    {:else if active === "storage"}
      <h2>Storage</h2>
      <div class="card box form">
        <label class="wide">Save folder
          <div class="row">
            <input class="input grow" bind:value={draft.save_dir} placeholder={app.info?.default_save_dir} />
            <button class="btn small" onclick={chooseFolder}>Browse</button>
            <button class="btn small ghost" onclick={() => api.reveal(storage?.save_dir ?? app.info?.save_dir ?? "")}>Open</button>
          </div>
          <small>One folder per game: the video, its timeline (session.json), and its clips.</small>
        </label>
        <label>Delete games older than
          <select class="input" bind:value={draft.auto_delete_days}>
            <option value={0}>Never</option>
            {#each [3, 7, 14, 30, 60, 90] as d}<option value={d}>{d} days</option>{/each}
          </select>
        </label>
        <label>Maximum space
          <select class="input" bind:value={draft.max_disk_gb}>
            <option value={0}>No limit</option>
            {#each [20, 50, 100, 200, 500, 1000] as g}<option value={g}>{g} GB</option>{/each}
          </select>
          <small>Oldest games go first. Favorites are never deleted.</small>
        </label>
      </div>
      {#if storage}
        <div class="card box stor">
          <div><span class="muted">Used by {storage.games} games</span><strong>{bytes(storage.used_bytes)}</strong></div>
          <div><span class="muted">Free on this drive</span><strong>{bytes(storage.free_bytes)}</strong></div>
          <button class="btn" onclick={cleanNow}>Apply clean-up now</button>
        </div>
      {/if}
    {:else if active === "general"}
      <h2>General</h2>
      <div class="card box form">
        <label class="check wide"><input type="checkbox" bind:checked={draft.start_with_windows} />Start with Windows</label>
        <label class="check wide"><input type="checkbox" bind:checked={draft.start_minimized} />Start minimized to the tray</label>
        <label class="check wide"><input type="checkbox" bind:checked={draft.keep_ui_loaded} />Keep Clairvoyance ready in the background <small>(opens instantly; while hidden in the tray it's paused and uses no CPU, but keeps some memory)</small></label>
        <label class="check wide"><input type="checkbox" bind:checked={draft.close_ui_in_game} />Close this window when a game starts <small>(off: stays open so you can alt-tab to it during the loading screen)</small></label>
        <label class="check wide"><input type="checkbox" bind:checked={draft.show_perf} />Show Clairvoyance's CPU/RAM use in the sidebar</label>
      </div>
      <p class="muted small">Closing the window keeps Clairvoyance in the tray (bottom-right, next to the clock). Use Quit there to exit.</p>
      <button class="btn danger" onclick={() => api.quit()}>Quit Clairvoyance</button>
    {:else if active === "performance"}
      <h2>Performance test</h2>
      <div class="card box"><PerfTest /></div>
    {:else if active === "advanced"}
      <h2>Advanced</h2>
      <h3 class="sub">Try it without playing</h3>
      <div class="card box">
        <p class="lead" style="margin-top:0">Plays a scripted League match against a fake game API (kills, a triple kill, a stolen Herald, towers, Baron…) and records your screen, so you can see the whole flow: detection, recording, events and the timeline.</p>
        <div class="row">
          <label class="inline">Length
            <select class="input" bind:value={simLength}>
              <option value={90}>1.5 min</option>
              <option value={180}>3 min</option>
              <option value={600}>10 min</option>
            </select>
          </label>
          <button class="btn primary" onclick={simulate} disabled={app.status?.state !== "idle"}><Icon name="play" size={14} fill />Simulate a League game</button>
        </div>
      </div>
      <h3 class="sub">Files</h3>
      <div class="card box list">
        <div class="item"><div><strong>Log file</strong><span class="muted path">{app.info?.log_file}</span></div><button class="btn small" onclick={() => api.reveal(app.info?.log_file ?? "")}>Show</button></div>
        <div class="item"><div><strong>Settings file</strong><span class="muted path">{app.info?.config_file}</span></div><button class="btn small" onclick={() => api.reveal(app.info?.config_file ?? "")}>Show</button></div>
      </div>
      <p class="muted small">Clairvoyance {app.info?.version}. No accounts, no cloud, no telemetry.</p>
    {/if}
  </div>

  {#if dirty}
    <div class="savebar fade-in">
      <span>You have unsaved changes</span>
      <button class="btn ghost" onclick={() => (draft = makeDraft())}>Discard</button>
      <button class="btn primary" onclick={save} disabled={saving}>{saving ? "Saving…" : "Save changes"}</button>
    </div>
  {/if}
</div>

<style>
  .settings {
    display: flex;
    height: 100%;
    position: relative;
  }
  .subnav {
    width: 220px;
    flex: none;
    padding: 26px 12px;
    border-right: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .subnav h1 {
    padding: 0 10px 16px;
  }
  .subnav button {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 9px 10px;
    border: none;
    border-radius: 8px;
    background: transparent;
    color: var(--muted);
    font-weight: 600;
    text-align: left;
  }
  .subnav button:hover {
    color: var(--text-2);
    background: var(--surface);
  }
  .subnav button.on {
    background: var(--surface-2);
    color: var(--text);
  }
  .content {
    flex: 1;
    max-width: 900px;
    padding-bottom: 100px;
  }
  .content h2 {
    font-size: 22px;
    margin-bottom: 8px;
  }
  .sub {
    margin: 26px 0 10px;
  }
  .lead {
    color: var(--text-2);
    line-height: 1.55;
    margin: 0 0 16px;
    max-width: 720px;
  }
  .box {
    padding: 16px 18px;
    margin-bottom: 12px;
  }
  .form {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 16px 20px;
  }
  .form label {
    display: flex;
    flex-direction: column;
    gap: 6px;
    font-weight: 600;
    font-size: 13px;
  }
  .form label.wide {
    grid-column: 1 / -1;
  }
  .form label.check,
  label.check {
    flex-direction: row;
    align-items: center;
    gap: 10px;
    font-weight: 500;
  }
  input[type="checkbox"],
  input[type="range"] {
    accent-color: var(--accent);
  }
  input[type="checkbox"] {
    width: 16px;
    height: 16px;
  }
  small {
    font-weight: 400;
    color: var(--muted);
    font-size: 12px;
  }
  .grow {
    flex: 1;
  }
  .list .item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 10px 0;
  }
  .list .item + .item {
    border-top: 1px solid var(--border);
  }
  .list .item > div {
    display: flex;
    flex-direction: column;
    gap: 3px;
    min-width: 0;
  }
  .path {
    font-size: 12px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 520px;
    user-select: text;
  }
  .kinds {
    display: flex;
    flex-wrap: wrap;
    gap: 7px;
  }
  .kinds.dim {
    opacity: 0.45;
  }
  .kind {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    padding: 5px 11px 5px 5px;
    border-radius: 999px;
    border: 1px solid var(--border-2);
    background: transparent;
    color: var(--muted);
    font-size: 12.5px;
    font-weight: 600;
  }
  .kind .ki {
    width: 20px;
    height: 20px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    background: var(--surface-3);
    color: var(--muted);
  }
  .kind.on {
    color: var(--text);
    background: var(--surface-2);
    border-color: color-mix(in srgb, var(--c) 50%, transparent);
  }
  .kind.on .ki {
    background: var(--c);
    color: #0b0d14;
  }
  .ffrow {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    align-items: center;
    font-size: 13px;
  }
  .ok {
    color: var(--ok);
    display: inline-flex;
    gap: 5px;
    align-items: center;
  }
  .warn {
    color: var(--warn);
  }
  .gh {
    margin-bottom: 14px;
  }
  .stor {
    display: flex;
    align-items: center;
    gap: 30px;
  }
  .stor > div {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .stor strong {
    font-size: 20px;
  }
  .stor .btn {
    margin-left: auto;
  }
  .small {
    font-size: 12.5px;
  }
  .inline {
    display: flex;
    align-items: center;
    gap: 8px;
    font-weight: 600;
    font-size: 13px;
  }
  .savebar {
    position: absolute;
    left: 50%;
    bottom: 22px;
    transform: translateX(-50%);
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 12px 10px 18px;
    background: #0a0c12;
    border: 1px solid var(--border-2);
    border-radius: 12px;
    box-shadow: var(--shadow);
    z-index: 20;
  }
</style>
