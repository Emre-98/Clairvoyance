<script lang="ts">
  import { app, saveSettings, reloadSettings, toast } from "../lib/store.svelte";
  import { api, pickFolder } from "../lib/api";
  import type { Settings } from "../lib/types";
  import Icon from "../components/Icon.svelte";
  import BuiltinRecorder from "../components/BuiltinRecorder.svelte";

  let step = $state(0);
  let riotId = $state(String(app.settings?.games?.league?.riot_id ?? ""));
  let saveDir = $state(app.settings?.save_dir ?? "");
  let autostart = $state(app.settings?.start_with_windows ?? false);
  const steps = ["Welcome", "Recording", "You", "Done"];

  async function saveYou() {
    const s = structuredClone($state.snapshot(app.settings!)) as Settings;
    s.games.league = { ...(s.games.league ?? {}), riot_id: riotId.trim() };
    s.save_dir = saveDir.trim();
    s.start_with_windows = autostart;
    s.start_minimized = autostart;
    try {
      await saveSettings(s);
      step = 3;
    } catch (e) {
      toast(String(e), "error");
    }
  }

  async function finish() {
    await api.finishFirstRun();
    await reloadSettings();
  }
</script>

<div class="overlay">
  <div class="wizard card fade-in">
    <div class="steps">
      {#each steps as s, i}
        <div class="st" class:on={i === step} class:done={i < step}><span>{#if i < step}<Icon name="check" size={12} stroke={3} />{:else}{i + 1}{/if}</span>{s}</div>
      {/each}
    </div>

    {#if step === 0}
      <div class="hero">
        <img src="/logo.svg" alt="" width="84" height="84" />
        <h1>Welcome to GameRecorder</h1>
        <p>Your games, recorded automatically, with every kill, death and objective on the timeline.</p>
        <ul>
          <li><Icon name="rec" size={16} /> Starts recording by itself when a League match begins</li>
          <li><Icon name="bolt" size={16} /> Jump straight to any kill, death, steal or ult on the timeline</li>
          <li><Icon name="cpu" size={16} /> Records on your graphics card, so there's no FPS drop, with nothing else to install</li>
        </ul>
      </div>
      <div class="foot"><div class="spacer"></div><button class="btn primary" onclick={() => (step = 1)}>Get started</button></div>
    {:else if step === 1}
      <h2>Recording</h2>
      <p class="lead">GameRecorder records on its own, using your graphics card. Try it: it records 5 seconds of your screen.</p>
      <BuiltinRecorder />
      <div class="foot">
        <button class="btn ghost" onclick={() => (step = 0)}>Back</button>
        <div class="spacer"></div>
        <button class="btn primary" onclick={() => (step = 2)}>Next</button>
      </div>
    {:else if step === 2}
      <h2>A few details</h2>
      <div class="form">
        <label>Your Riot ID <small>(optional: found automatically in game)</small>
          <input class="input" bind:value={riotId} placeholder="Name#TAG" />
        </label>
        <label>Where to save games
          <div class="row">
            <input class="input grow" bind:value={saveDir} placeholder={app.info?.default_save_dir} />
            <button class="btn small" onclick={async () => { const f = await pickFolder(); if (f) saveDir = f; }}>Browse</button>
          </div>
          <small>Needs a few GB per hour of play. You can set auto clean-up in Settings > Storage.</small>
        </label>
        <label class="check"><input type="checkbox" bind:checked={autostart} />Start with Windows (in the tray) so games are never missed</label>
      </div>
      <div class="foot">
        <button class="btn ghost" onclick={() => (step = 1)}>Back</button>
        <div class="spacer"></div>
        <button class="btn primary" onclick={saveYou}>Next</button>
      </div>
    {:else}
      <div class="hero">
        <div class="donecheck"><Icon name="check" size={34} stroke={2.6} /></div>
        <h1>You're all set</h1>
        <p>Start a League game and GameRecorder does the rest. In game:</p>
        <div class="keys">
          <div><kbd>{app.settings?.hotkey_clip}</kbd><span>Save the last {app.settings?.video.replay_buffer_secs} seconds as a clip</span></div>
          <div><kbd>{app.settings?.hotkey_marker}</kbd><span>Add a marker to find a moment later</span></div>
        </div>
        <p class="muted small">Closing the window keeps GameRecorder running in the tray. Want to see it work first? Settings > Advanced > Simulate a League game.</p>
      </div>
      <div class="foot"><div class="spacer"></div><button class="btn primary" onclick={finish}>Open GameRecorder</button></div>
    {/if}
  </div>
</div>

<style>
  .overlay {
    position: absolute;
    inset: 0;
    z-index: 40;
    display: grid;
    place-items: center;
    background:
      radial-gradient(800px 400px at 20% 10%, rgba(46, 230, 197, 0.08), transparent 70%),
      radial-gradient(700px 400px at 90% 90%, rgba(123, 97, 255, 0.12), transparent 70%),
      var(--bg);
    overflow-y: auto;
    padding: 30px;
  }
  .wizard {
    width: min(720px, 100%);
    padding: 26px 32px 22px;
    box-shadow: var(--shadow);
  }
  .steps {
    display: flex;
    gap: 22px;
    margin-bottom: 26px;
    font-size: 12.5px;
    font-weight: 600;
    color: var(--muted);
  }
  .st {
    display: flex;
    align-items: center;
    gap: 7px;
  }
  .st span {
    width: 20px;
    height: 20px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    background: var(--surface-3);
    font-size: 11px;
  }
  .st.on {
    color: var(--text);
  }
  .st.on span {
    background: var(--accent-grad);
    color: #081016;
  }
  .st.done span {
    background: rgba(61, 220, 132, 0.2);
    color: var(--ok);
  }
  .hero {
    text-align: center;
    padding: 10px 10px 4px;
  }
  .hero h1 {
    margin: 14px 0 8px;
  }
  .hero p {
    color: var(--text-2);
  }
  ul {
    list-style: none;
    padding: 0;
    margin: 22px auto 6px;
    display: inline-flex;
    flex-direction: column;
    gap: 10px;
    text-align: left;
  }
  li {
    display: flex;
    gap: 10px;
    align-items: center;
    color: var(--text-2);
  }
  li :global(svg) {
    color: var(--accent);
  }
  .lead {
    color: var(--text-2);
    margin: 6px 0 12px;
  }
  .form {
    display: flex;
    flex-direction: column;
    gap: 18px;
    margin-top: 16px;
  }
  .form label {
    display: flex;
    flex-direction: column;
    gap: 6px;
    font-weight: 600;
    font-size: 13px;
  }
  .form label.check {
    flex-direction: row;
    align-items: center;
    gap: 10px;
    font-weight: 500;
  }
  .check input {
    accent-color: var(--accent);
    width: 16px;
    height: 16px;
  }
  small {
    font-weight: 400;
    color: var(--muted);
  }
  .grow {
    flex: 1;
  }
  .foot {
    display: flex;
    gap: 10px;
    margin-top: 26px;
    padding-top: 16px;
    border-top: 1px solid var(--border);
  }
  .donecheck {
    width: 72px;
    height: 72px;
    margin: 0 auto;
    border-radius: 50%;
    display: grid;
    place-items: center;
    background: rgba(61, 220, 132, 0.15);
    color: var(--ok);
  }
  .keys {
    display: inline-flex;
    flex-direction: column;
    gap: 10px;
    margin: 12px 0;
    text-align: left;
  }
  .keys div {
    display: flex;
    gap: 12px;
    align-items: center;
    color: var(--text-2);
  }
  .small {
    font-size: 12.5px;
  }
</style>
