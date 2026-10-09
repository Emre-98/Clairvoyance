<script lang="ts">
  import { app, boot } from "./lib/store.svelte";
  import TitleBar from "./components/TitleBar.svelte";
  import Sidebar from "./components/Sidebar.svelte";
  import Toasts from "./components/Toasts.svelte";
  import Home from "./pages/Home.svelte";
  import Library from "./pages/Library.svelte";
  import Clips from "./pages/Clips.svelte";
  import GameDetail from "./pages/GameDetail.svelte";

  // Settings and the first-run setup are loaded on demand: they're rarely open, and release builds
  // obfuscate only these chunks (ui/vite.config.ts), never the library, player or overlay code.
  // Settings is preloaded once the app is idle, so opening it stays instant.
  let settingsPage: Promise<typeof import("./pages/Settings.svelte").default> | null = null;
  const loadSettings = () => (settingsPage ??= import("./pages/Settings.svelte").then((m) => m.default));
  const loadSetup = () => import("./pages/Setup.svelte").then((m) => m.default);

  let error = $state<string | null>(null);
  boot()
    .then(() => (window.requestIdleCallback ?? ((f: () => void) => setTimeout(f, 2000)))(() => void loadSettings()))
    .catch((e) => (error = String(e)));

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
        {#key app.route.page === "game" ? "game:" + app.route.id : app.route.page}
        <div class="route">
        {#if app.route.page === "home"}
          <Home />
        {:else if app.route.page === "games"}
          <Library />
        {:else if app.route.page === "clips"}
          <Clips />
        {:else if app.route.page === "game"}
          {#key app.route.id}<GameDetail id={app.route.id} t={app.route.t} />{/key}
        {:else if app.route.page === "settings"}
          {#await loadSettings() then Settings}<Settings section={app.route.section ?? "setup"} />{/await}
        {/if}
        </div>
        {/key}
      </main>
      {#if !app.settings.first_run_done}{#await loadSetup() then Setup}<Setup />{/await}{/if}
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
  /* Page change: a short fade + rise on the GPU (opacity/transform only). */
  .route {
    height: 100%;
    animation: route-in 0.17s var(--ease) both;
  }
  @keyframes route-in {
    from {
      opacity: 0;
      transform: translateY(6px);
    }
  }
</style>
