<script lang="ts">
  import { app, boot } from "./lib/store.svelte";
  import TitleBar from "./components/TitleBar.svelte";
  import Sidebar from "./components/Sidebar.svelte";
  import Toasts from "./components/Toasts.svelte";
  import Home from "./pages/Home.svelte";
  import Library from "./pages/Library.svelte";
  import Clips from "./pages/Clips.svelte";
  import GameDetail from "./pages/GameDetail.svelte";
  import Settings from "./pages/Settings.svelte";
  import Setup from "./pages/Setup.svelte";

  let error = $state<string | null>(null);
  boot().catch((e) => (error = String(e)));

  // Block the WebView's own context menu / reload shortcuts for an app feel.
  function keys(e: KeyboardEvent) {
    if (e.key === "F5" || (e.ctrlKey && e.key.toLowerCase() === "r")) e.preventDefault();
  }
</script>

<svelte:window onkeydown={keys} oncontextmenu={(e) => { if (!(e.target as HTMLElement)?.closest?.("input,textarea,.selectable")) e.preventDefault(); }} />

<div class="shell">
  <TitleBar />
  <div class="body">
    {#if error}
      <div class="page"><div class="empty">Something went wrong starting the app: {error}</div></div>
    {:else if app.settings && app.info}
      <Sidebar />
      <main>
        {#if app.route.page === "home"}
          <Home />
        {:else if app.route.page === "games"}
          <Library />
        {:else if app.route.page === "clips"}
          <Clips />
        {:else if app.route.page === "game"}
          {#key app.route.id}<GameDetail id={app.route.id} t={app.route.t} />{/key}
        {:else if app.route.page === "settings"}
          <Settings section={app.route.section ?? "setup"} />
        {/if}
      </main>
      {#if !app.settings.first_run_done}<Setup />{/if}
    {/if}
  </div>
  <Toasts />
</div>

<style>
  .shell {
    height: 100%;
    display: flex;
    flex-direction: column;
  }
  .body {
    flex: 1;
    display: flex;
    min-height: 0;
    position: relative;
  }
  main {
    flex: 1;
    min-width: 0;
    height: 100%;
    overflow: hidden;
  }
</style>
