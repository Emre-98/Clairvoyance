<script lang="ts">
  let { value = $bindable(), single = false }: { value: string; single?: boolean } = $props();
  let listening = $state(false);

  function name(e: KeyboardEvent): string | null {
    const c = e.code;
    if (/^Key[A-Z]$/.test(c)) return c.slice(3);
    if (/^Digit\d$/.test(c)) return c.slice(5);
    if (/^F\d{1,2}$/.test(c)) return c;
    if (/^Numpad\d$/.test(c)) return "Num" + c.slice(6);
    const map: Record<string, string> = {
      Enter: "Enter",
      Space: "Space",
      Tab: "Tab",
      Backspace: "Backspace",
      Insert: "Insert",
      Delete: "Delete",
      Home: "Home",
      End: "End",
      PageUp: "Pageup",
      PageDown: "Pagedown",
      Pause: "Pause",
      Backquote: "`",
    };
    return map[c] ?? null;
  }

  function key(e: KeyboardEvent) {
    if (!listening) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.code === "Escape") {
      listening = false;
      return;
    }
    const k = name(e);
    if (!k) return;
    const parts = [];
    if (!single) {
      if (e.ctrlKey) parts.push("Ctrl");
      if (e.shiftKey) parts.push("Shift");
      if (e.altKey) parts.push("Alt");
    }
    parts.push(k);
    value = parts.join("+");
    listening = false;
  }
</script>

<button class="hk" class:listening onclick={() => (listening = true)} onkeydown={key} onblur={() => (listening = false)}>
  {#if listening}
    Press a key… <span class="muted">(Esc to cancel)</span>
  {:else}
    {#each (value || "None").split("+") as p, i}{#if i > 0}<span class="plus">+</span>{/if}<kbd>{p}</kbd>{/each}
  {/if}
</button>

<style>
  .hk {
    min-width: 150px;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 7px 10px;
    border-radius: var(--radius-sm);
    border: 1px solid var(--border-2);
    background: var(--bg-2);
  }
  .hk.listening {
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--accent-soft);
    color: var(--accent);
    font-size: 13px;
  }
  .plus {
    color: var(--muted);
    font-size: 12px;
  }
  .muted {
    font-size: 12px;
  }
</style>
