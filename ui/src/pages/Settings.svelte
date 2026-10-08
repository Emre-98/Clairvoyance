<script lang="ts">
  import { app, saveSettings, toast } from "../lib/store.svelte";
  import { api, on, pickFolder } from "../lib/api";
  import { bytes, relativeDate } from "../lib/format";
  import { KIND, USER_KINDS } from "../lib/eventmeta";
  import type { EventKind, Settings, StorageInfo } from "../lib/types";
  import { setTheme, type Theme } from "../lib/theme";
  import { summary as perfSummary } from "../lib/perfmarks";
  import Icon from "../components/Icon.svelte";
  import HotkeyInput from "../components/HotkeyInput.svelte";
  import BuiltinRecorder from "../components/BuiltinRecorder.svelte";
  import PerfTest from "../components/PerfTest.svelte";
  import GameModes from "../components/GameModes.svelte";

  let { section = "setup" }: { section?: string } = $props();

  const sections = [
    { id: "setup", label: "Recorder", icon: "rec" },
    { id: "recording", label: "Recording", icon: "clips" },
    { id: "modes", label: "Game modes", icon: "flag" },
    { id: "hotkeys", label: "Hotkeys", icon: "keyboard" },
    { id: "events", label: "Events & clips", icon: "bolt" },
    { id: "games", label: "Games", icon: "gamepad" },
    { id: "storage", label: "Storage", icon: "folder" },
    { id: "appearance", label: "Appearance", icon: "palette" },
    { id: "general", label: "General & updates", icon: "settings" },
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

  // Theme changes apply and save at once (no "Save changes" needed).
  const themes: { id: Theme; label: string; icon: string; desc: string }[] = [
    { id: "system", label: "Match Windows", icon: "monitor", desc: "Follows your Windows light/dark setting" },
    { id: "dark", label: "Dark", icon: "moon", desc: "The classic Clairvoyance look" },
    { id: "light", label: "Light", icon: "sun", desc: "Bright and easy on daylight" },
  ];
  function chooseTheme(t: Theme) {
    setTheme(t);
    if (app.settings) app.settings.theme = t;
    draft.theme = t;
    baseline.theme = t;
  }

  let timings = $state<{ startup_ms: number | null; page_ms: number | null } | null>(null);
  let marks = $state(perfSummary());
  $effect(() => {
    if (active === "advanced") {
      marks = perfSummary();
      api.uiTimings().then((t) => (timings = t)).catch(() => {});
    }
  });
  const ms = (v: number | null | undefined) => (v == null ? "-" : `${Math.round(v)} ms`);

  async function checkUpdates() {
    try {
      const u = await api.updateCheck();
      app.update = u;
      if (u.state === "up_to_date") toast("You're on the latest version.", "ok");
    } catch (e) {
      toast(`Couldn't check for updates: ${e}`, "error", 8000);
    }
  }

  let storage = $state<StorageInfo | null>(null);
  let ff = $state<{ available: boolean; path?: string | null } | null>(null);
  $effect(() => {
    if (active === "storage") {
      app.libraryVersion;
      api.storageInfo().then((s) => (storage = s));
    }
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

  let cleaning = $state(false);
  async function cleanNow() {
    cleaning = true;
    try {
      if (dirty) await save();
      const r = await api.cleanupNow();
      if (r.removed.length) toast(`Removed ${r.removed.length} old recording${r.removed.length === 1 ? "" : "s"} (${bytes(r.freed_bytes)})`, "ok");
      else if (r.still_over) toast("Still over the limit, but only favorites and kept clips are left.", "warn", 7000);
      else toast("Nothing to remove: you're under the limit.", "ok");
      storage = await api.storageInfo();
    } catch (e) {
      toast(String(e), "error");
    } finally {
      cleaning = false;
    }
  }

  // Storage limit slider: fine steps at the low end, coarse ones above.
  const GB_STEPS = [10, 20, 30, 40, 50, 60, 75, 100, 125, 150, 200, 250, 300, 400, 500, 750, 1000, 1500, 2000];
  const stepIndex = $derived(GB_STEPS.reduce((best, v, i) => (Math.abs(v - draft.max_disk_gb) < Math.abs(GB_STEPS[best] - draft.max_disk_gb) ? i : best), 0));
  const usedPct = $derived(storage && draft.max_disk_gb > 0 ? Math.min(100, (storage.used_bytes / (draft.max_disk_gb * 1024 ** 3)) * 100) : 0);
  const overLimit = $derived(!!storage && draft.max_disk_gb > 0 && storage.used_bytes > draft.max_disk_gb * 1024 ** 3);

  let simSpeed = $state(1);
  let simLength = $state(180);
  let simQueue = $state(400);
  let simWatch = $state("");
  let reporting = $state(false);
  async function saveReport() {
    reporting = true;
    try {
      const path = await api.testReport({ ...perfSummary(), ui_timings: timings });
      toast("Test report saved.", "ok", 6000);
      api.reveal(path);
    } catch (e) {
      toast(`Couldn't save the test report: ${e}`, "error");
    } finally {
      reporting = false;
    }
  }

  async function simulate() {
    try {
      await api.simulateGame(simSpeed, simLength, simQueue, simWatch || null);
      toast(simWatch ? "Simulated League replay / spectating started. It shouldn't be recorded." : "Simulated League game started. It records your desktop.", "ok", 7000);
    } catch (e) {
      toast(String(e), "error");
    }
  }
</script>

<div class="settings">
  <nav class="subnav">
    <h1>Settings</h1>
    {#each sections as s}
      <button class:on={active === s.id} onclick={() => (active = s.id)}><Icon name={s.icon} size={16} />{s.label}{#if s.id === "modes" && app.newModes > 0}<span class="navnew" title="New modes detected">{app.newModes}</span>{/if}</button>
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
        <label>Video codec
          <select class="input" bind:value={draft.video.codec} data-testid="codec-select">
            <option value="h264">H.264 (plays everywhere)</option>
            {#each [["hevc", "HEVC / H.265 (smaller files)"], ["av1", "AV1 (smallest, newest GPUs)"]] as [c, label]}
              <option value={c} disabled={!(draft.video.playable_codecs ?? []).includes(c) && draft.video.codec !== c}>{label}{(draft.video.playable_codecs ?? []).includes(c) ? "" : " – not playable in this app here"}</option>
            {/each}
          </select>
          <small>HEVC and AV1 need a GPU that encodes them and Windows' free / store decoder (HEVC Video Extensions, AV1 Video Extension); otherwise games are recorded in H.264. Clips you export keep the codec; "Exact cut" makes H.264 for sharing.</small>
        </label>
        <label>Bitrate
          <select class="input" bind:value={draft.video.rate_control} data-testid="ratecontrol-select">
            <option value="bitrate">Fixed average (steady file size)</option>
            <option value="quality">Quality-based (smaller in calm moments, more for fights)</option>
          </select>
          <small>Quality-based sizes vary with the game; try the 5 s recorder test after changing it.</small>
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
    {:else if active === "modes"}
      <h2>Game modes</h2>
      <p class="lead">Choose per mode whether it's recorded. The list comes from the League client and Riot, so rotating and new modes show up by themselves.</p>
      <GameModes />
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
            <span class="ok"><Icon name="check" size={14} /> ffmpeg ready</span><span class="muted path" title={ff.path}>{ff.path}</span>
          {:else}
            <span class="warn">Clips need the free ffmpeg tool.</span>
            <button class="btn small primary" onclick={getFfmpeg} disabled={ffdl != null}><Icon name="download" size={13} />{ffdl ? `Downloading ${ffdl.total ? Math.round((ffdl.done / ffdl.total) * 100) + "%" : bytes(ffdl.done)}` : "Download ffmpeg (~100 MB)"}</button>
            <span class="muted">or set its path:</span>
          {/if}
          <input class="input grow" bind:value={draft.ffmpeg_path} placeholder="Path to ffmpeg.exe (optional)" />
        </div>
      </div>

      <h3 class="sub">Sharing</h3>
      <div class="card box form">
        <label class="check wide"><input type="checkbox" bind:checked={draft.share_fit_discord} />Fit shared clips for Discord <small>(Share makes a copy under 19.5 MB, Discord's free upload limit; off: the original clip is shared)</small></label>
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
                {:else if f.kind === "select"}
                  <select class="input" bind:value={cfg[f.key] as string}>
                    {#each f.options ?? [] as [v, label]}<option value={v}>{label}</option>{/each}
                  </select>
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
      <div class="card box">
        <div class="limit-head">
          <div>
            <strong>Max storage for recordings</strong>
            <div class="muted small">When your recordings take more than this, the oldest are deleted first (with their clips, timeline and thumbnail).</div>
          </div>
          <div class="gb-input">
            <input class="input" type="number" min="5" max="10000" bind:value={draft.max_disk_gb} aria-label="Max storage in GB" />
            <span>GB</span>
          </div>
        </div>
        <input class="gb-slider" type="range" min="0" max={GB_STEPS.length - 1} step="1" value={stepIndex} oninput={(e) => (draft.max_disk_gb = GB_STEPS[Number((e.currentTarget as HTMLInputElement).value)])} aria-label="Max storage" />
        <div class="usage" class:over={overLimit}>
          <div class="usage-bar"><span style="transform:scaleX({usedPct / 100})"></span></div>
          <div class="usage-txt">
            {#if storage}
              <span><strong>{bytes(storage.used_bytes)}</strong> used of {draft.max_disk_gb} GB · {storage.games} game{storage.games === 1 ? "" : "s"}</span>
              <span class="muted">{bytes(storage.free_bytes)} free on this drive</span>
            {:else}
              <span class="skeleton-line" style="width:220px"></span>
            {/if}
          </div>
        </div>
        <label class="check toggle-row"><input type="checkbox" bind:checked={draft.auto_cleanup} />Automatically delete the oldest recordings to stay under the limit <small>(after each game and when Clairvoyance starts, never during a game)</small></label>
        <div class="row cl-actions">
          <label class="inline">Also delete games older than
            <select class="input" bind:value={draft.auto_delete_days}>
              <option value={0}>Never</option>
              {#each [7, 14, 30, 60, 90, 180] as d}<option value={d}>{d} days</option>{/each}
            </select>
          </label>
          <div class="spacer"></div>
          <button class="btn" onclick={cleanNow} disabled={cleaning || app.status?.state !== "idle"}>{cleaning ? "Cleaning…" : "Clean up now"}</button>
        </div>
      </div>
      {#if storage?.stuck_over_limit}
        <div class="card box warnbox"><Icon name="warn" size={18} /><div><strong>Over the limit, but everything left is protected</strong><div class="muted small">Only favorites and games with clips marked "keep" remain ({bytes(storage.protected_bytes)}). Unstar some favorites, un-keep clips, or raise the limit.</div></div></div>
      {/if}
      <div class="card box protect">
        <Icon name="star" size={16} fill />
        <div><strong>Never deleted automatically</strong><div class="muted small">Games you star as favorite (★ on a game) and clips you mark "keep" (the pin on a clip). If an old game has kept clips, only its full video is removed and the clips stay.</div></div>
      </div>
      <h3 class="sub">Save folder</h3>
      <div class="card box form">
        <label class="wide">
          <div class="row">
            <input class="input grow" bind:value={draft.save_dir} placeholder={app.info?.default_save_dir} />
            <button class="btn small" onclick={chooseFolder}>Browse</button>
            <button class="btn small ghost" onclick={() => api.reveal(storage?.save_dir ?? app.info?.save_dir ?? "")}>Open</button>
          </div>
          <small>One folder per game: the video, its timeline (session.json) and its clips. Thumbnails are kept separately in the app's data folder.</small>
        </label>
      </div>
      <h3 class="sub">Recently removed by the clean-up</h3>
      {#if storage?.recent_cleanups.length}
        <div class="card box list cleanlog">
          {#each storage.recent_cleanups as c (c.at + c.id)}
            <div class="item">
              <div><strong>{c.title}</strong><span class="muted">{c.action === "delete_video" ? "Full video removed, kept clips stay" : "Recording, clips and timeline removed"} · {c.reason} · {relativeDate(c.at)}</span></div>
              <span class="muted">{bytes(c.bytes)}</span>
            </div>
          {/each}
        </div>
      {:else}
        <p class="muted small">Nothing yet.</p>
      {/if}
    {:else if active === "appearance"}
      <h2>Appearance</h2>
      <p class="lead">Changes apply right away.</p>
      <div class="themes" role="radiogroup" aria-label="Theme">
        {#each themes as t}
          <button class="theme-opt" class:on={draft.theme === t.id} role="radio" aria-checked={draft.theme === t.id} onclick={() => chooseTheme(t.id)}>
            <span class="preview" aria-hidden="true">
              {#each t.id === "system" ? ["light", "dark"] : [t.id] as scheme}
                <span class="pv {scheme}" class:half={t.id === "system" && scheme === "dark"}>
                  <span class="pv-side"></span>
                  <span class="pv-main"><span class="pv-bar"></span><span class="pv-row"><span></span><span></span></span><span class="pv-dots"><i class="k"></i><i class="d"></i><i class="e"></i></span></span>
                </span>
              {/each}
            </span>
            <span class="theme-label"><Icon name={t.icon} size={15} />{t.label}</span>
            <span class="muted theme-desc">{t.desc}</span>
          </button>
        {/each}
      </div>
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

      <h3 class="sub">Updates</h3>
      <div class="card box about">
        <img src="/logo.svg" alt="" width="44" height="44" />
        <div class="grow">
          <strong>Clairvoyance {app.info?.version}</strong>
          <div class="muted small">
            {#if app.update?.state === "checking"}Checking for updates…
            {:else if app.update?.state === "available"}Version {app.update.version} is available (see the card in the sidebar).
            {:else if app.update?.state === "downloading" || app.update?.state === "installing"}Updating…
            {:else if app.update?.state === "error"}Couldn't check: {app.update.error}
            {:else if app.update?.state === "up_to_date"}You're up to date{app.update.checked_at ? ` (checked ${relativeDate(app.update.checked_at)})` : ""}.
            {:else}Updates come from GitHub Releases.{/if}
          </div>
        </div>
        <button class="btn" onclick={checkUpdates} disabled={app.update?.state === "checking" || app.update?.state === "downloading" || app.update?.state === "installing"}>Check now</button>
      </div>
      <div class="card box form">
        <label class="check wide"><input type="checkbox" bind:checked={draft.auto_update_check} />Check for updates automatically <small>(when Clairvoyance starts and every few hours, never during a game; you always choose when to install)</small></label>
      </div>
    {:else if active === "performance"}
      <h2>Performance test</h2>
      <div class="card box"><PerfTest /></div>
    {:else if active === "advanced"}
      <h2>Advanced</h2>
      <h3 class="sub">Responsiveness (measured in this window)</h3>
      <div class="card box list">
        <div class="item"><div><strong>Window ready after launch</strong><span class="muted">Start of the app until the library is on screen (target under 1 s)</span></div><span class="metric">{ms(timings?.startup_ms)}</span></div>
        <div class="item"><div><strong>Page switch</strong><span class="muted">Click until the new page is painted, median / slowest of the last {marks.page_switch.n} (target under 100 ms)</span></div><span class="metric">{ms(marks.page_switch.median)} / {ms(marks.page_switch.max)}</span></div>
        <div class="item"><div><strong>Opening a replay</strong><span class="muted">Click on a game until its first video frame is on screen, median / slowest of the last {marks.replay_frame.n} (target under 500 ms)</span></div><span class="metric">{ms(marks.replay_frame.median)} / {ms(marks.replay_frame.max)}</span></div>
        <div class="item"><div><strong>Input overlay on</strong><span class="muted">First switch-on of a replay's input overlay until it's drawn (loads the recording), median / slowest of the last {marks.overlay_on.n} (target under 100 ms)</span></div><span class="metric">{ms(marks.overlay_on.median)} / {ms(marks.overlay_on.max)}</span></div>
        <div class="item"><div><strong>Timeline jump</strong><span class="muted">Marker click until the video shows the moment, median / slowest of the last {marks.seek.n} (target under 200 ms)</span></div><span class="metric">{ms(marks.seek.median)} / {ms(marks.seek.max)}</span></div>
      </div>
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
          <label class="inline" title="The mode the fake League client reports: try your Game modes rules">Mode
            <select class="input" bind:value={simQueue}>
              <option value={400}>Draft Pick</option>
              <option value={420}>Ranked Solo/Duo</option>
              <option value={450}>ARAM</option>
              <option value={1700}>Arena</option>
              <option value={-1}>Practice Tool</option>
              <option value={9999}>A brand-new mode</option>
            </select>
          </label>
          <label class="inline" title="A match you play, or League's spectator mode (never recorded)">What
            <select class="input" bind:value={simWatch}>
              <option value="">A match</option>
              <option value="replay">A replay</option>
              <option value="spectate">Spectating</option>
              <option value="spectate-live">Spectating a friend</option>
              <option value="replay-late">A replay found late</option>
            </select>
          </label>
          <button class="btn primary" onclick={simulate} disabled={app.status?.state !== "idle"}><Icon name="play" size={14} fill />Simulate a League game</button>
        </div>
      </div>
      <h3 class="sub">Test report</h3>
      <div class="card box">
        <p class="lead" style="margin-top:0">Something went wrong in a game? Save a test report: one small zip with the log, the latest game's timeline data and the numbers above (no videos, no Riot ID). It's saved in your recordings folder under "test-reports".</p>
        <button class="btn" onclick={saveReport} disabled={reporting}><Icon name="download" size={14} />{reporting ? "Saving…" : "Save test report"}</button>
      </div>
      <h3 class="sub">Files</h3>
      <div class="card box list">
        <div class="item"><div><strong>Log file</strong><span class="muted path" title={app.info?.log_file}>{app.info?.log_file}</span></div><button class="btn small" onclick={() => api.reveal(app.info?.log_file ?? "")}>Show</button></div>
        <div class="item"><div><strong>Settings file</strong><span class="muted path" title={app.info?.config_file}>{app.info?.config_file}</span></div><button class="btn small" onclick={() => api.reveal(app.info?.config_file ?? "")}>Show</button></div>
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
  .subnav button:active {
    transform: scale(0.98);
  }
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
    transition: background 0.15s var(--ease), color 0.15s var(--ease), transform 0.1s var(--ease);
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
    color: var(--on-ev);
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
  .themes {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 14px;
    max-width: 760px;
  }
  .theme-opt {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 6px;
    padding: 12px 12px 14px;
    border-radius: var(--radius);
    border: 1px solid var(--border);
    background: var(--surface);
    text-align: left;
    transition: border-color 0.15s var(--ease), transform 0.15s var(--ease), box-shadow 0.15s var(--ease);
  }
  .theme-opt:hover {
    border-color: var(--border-hover);
    transform: translateY(-1px);
  }
  .theme-opt:active {
    transform: translateY(0) scale(0.99);
  }
  .theme-opt.on {
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--accent-soft);
  }
  .theme-label {
    display: flex;
    align-items: center;
    gap: 7px;
    font-weight: 650;
    margin-top: 4px;
  }
  .theme-desc {
    font-size: 12px;
  }
  /* Mini previews: each renders the real theme tokens under its own color-scheme, so it shows
     that theme whatever the current one is. */
  .preview {
    position: relative;
    width: 100%;
    aspect-ratio: 16 / 10;
    border-radius: 8px;
    overflow: hidden;
    border: 1px solid var(--border);
  }
  .pv {
    position: absolute;
    inset: 0;
    display: flex;
    background: var(--bg);
  }
  .pv.dark {
    color-scheme: dark;
  }
  .pv.light {
    color-scheme: light;
  }
  .pv.half {
    clip-path: polygon(100% 0, 100% 100%, 0 100%);
  }
  .pv-side {
    width: 22%;
    background: var(--bg-2);
    border-right: 1px solid var(--border);
  }
  .pv-main {
    flex: 1;
    padding: 9% 8%;
    display: flex;
    flex-direction: column;
    gap: 9%;
  }
  .pv-bar {
    height: 12%;
    width: 55%;
    border-radius: 3px;
    background: var(--surface-3);
  }
  .pv-row {
    display: flex;
    gap: 8%;
    flex: 1;
  }
  .pv-row span {
    flex: 1;
    border-radius: 4px;
    background: var(--surface);
    border: 1px solid var(--border-2);
  }
  .pv-dots {
    display: flex;
    gap: 6%;
  }
  .pv-dots i {
    width: 9px;
    height: 9px;
    border-radius: 50%;
  }
  .pv-dots .k {
    background: var(--ev-kill);
  }
  .pv-dots .d {
    background: var(--ev-death);
  }
  .pv-dots .e {
    background: var(--ev-epic);
  }
  .about {
    display: flex;
    align-items: center;
    gap: 14px;
  }
  .navnew {
    margin-left: auto;
    min-width: 18px;
    height: 18px;
    padding: 0 5px;
    border-radius: 9px;
    display: grid;
    place-items: center;
    font-size: 11px;
    font-weight: 800;
    background: var(--accent-grad);
    color: var(--on-accent);
  }
  .metric {
    font-weight: 700;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .limit-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 20px;
  }
  .gb-input {
    display: flex;
    align-items: center;
    gap: 8px;
    font-weight: 600;
  }
  .gb-input input {
    width: 96px;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  .gb-slider {
    width: 100%;
    margin: 18px 0 8px;
  }
  .usage-bar {
    height: 8px;
    border-radius: 4px;
    background: var(--surface-3);
    overflow: hidden;
  }
  .usage-bar span {
    display: block;
    height: 100%;
    background: var(--accent-grad);
    transform-origin: left;
    transition: transform 0.25s var(--ease);
  }
  .usage.over .usage-bar span {
    background: var(--danger);
  }
  .usage-txt {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    margin-top: 8px;
    font-size: 13px;
    font-variant-numeric: tabular-nums;
  }
  .toggle-row {
    margin-top: 16px;
  }
  .cl-actions {
    margin-top: 14px;
  }
  .warnbox,
  .protect {
    display: flex;
    gap: 12px;
    align-items: flex-start;
  }
  .warnbox {
    border-color: color-mix(in srgb, var(--warn) 45%, transparent);
    background: color-mix(in srgb, var(--warn) 8%, var(--surface));
    color: var(--warn);
  }
  .warnbox strong,
  .protect strong {
    color: var(--text);
  }
  .protect :global(svg) {
    color: var(--fav);
    flex: none;
    margin-top: 2px;
  }
  .cleanlog .item > span {
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
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
    background: var(--popover);
    border: 1px solid var(--border-2);
    border-radius: 12px;
    box-shadow: var(--shadow);
    z-index: 20;
  }
</style>
