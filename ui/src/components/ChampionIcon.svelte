<script lang="ts" module>
  // One shared lookup of the current Data Dragon version for all icons.
  let pending: Promise<string | null> | null = null;
  export function fetchVersion(): Promise<string | null> {
    pending ??= fetch("https://ddragon.leagueoflegends.com/api/versions.json")
      .then((r) => r.json())
      .then((v: string[]) => {
        try {
          localStorage.setItem("ddragon-version", v[0]);
          localStorage.setItem("ddragon-version-at", String(Date.now()));
        } catch {}
        return v[0];
      })
      .catch(() => null);
    return pending;
  }
</script>

<script lang="ts">
  // Champion portraits come from Riot's public Data Dragon CDN (only while the window is open).
  let { id, name, size = 40, round = false }: { id?: string | null; name?: string | null; size?: number; round?: boolean } = $props();

  let version = $state<string | null>(cachedVersion());
  let failed = $state(false);

  function cachedVersion(): string | null {
    try {
      const v = localStorage.getItem("ddragon-version");
      const at = Number(localStorage.getItem("ddragon-version-at") ?? 0);
      if (v && Date.now() - at < 3 * 86400000) return v;
    } catch {}
    return null;
  }

  $effect(() => {
    if (!version && id) fetchVersion().then((v) => (version = v));
  });

  const initials = $derived((name ?? id ?? "?").slice(0, 2).toUpperCase());
  const src = $derived(id && version ? `https://ddragon.leagueoflegends.com/cdn/${version}/img/champion/${id}.png` : null);
</script>

<div class="ci" class:round style="width:{size}px;height:{size}px;font-size:{Math.round(size * 0.36)}px">
  {#if src && !failed}
    <img {src} alt={name ?? id ?? ""} onerror={() => (failed = true)} draggable="false" />
  {:else}
    <span>{initials}</span>
  {/if}
</div>

<style>
  .ci {
    flex: none;
    border-radius: 10px;
    overflow: hidden;
    background: linear-gradient(135deg, #2a3147, #1a1f2e);
    display: grid;
    place-items: center;
    font-weight: 700;
    color: var(--text-2);
    border: 1px solid rgba(255, 255, 255, 0.08);
  }
  .round {
    border-radius: 50%;
  }
  img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    transform: scale(1.08);
  }
</style>
