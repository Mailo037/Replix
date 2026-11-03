<script lang="ts">
  import { api, type Item } from "../lib/api";
  import { day, size } from "../lib/format";
  import { store, type Filter } from "../lib/store.svelte";
  import ClipTile from "./ClipTile.svelte";
  import Icon from "./Icon.svelte";
  import ImageViewer from "./ImageViewer.svelte";
  import Mark from "./Mark.svelte";

  let { filter, game }: { filter: Filter; game: string | null } = $props();
  let query = $state("");
  /** Which screenshot is open in the viewer. */
  let viewing = $state<number | null>(null);
  let selecting = $state(false);
  let selected = $state<string[]>([]);
  /** Where a Shift-click range starts. */
  let anchor: string | null = null;

  const titles: Record<Filter, string> = {
    all: "All clips",
    clips: "Clips",
    screenshots: "Screenshots",
    sessions: "Sessions",
    drafts: "Drafts",
    favorites: "Favorites",
  };

  const visible = $derived.by(() => {
    const q = query.trim().toLowerCase();
    return store.items.filter((i) => {
      if (game && i.game !== game) return false;
      if (filter === "clips" && i.kind !== "clip") return false;
      if (filter === "screenshots" && i.kind !== "screenshot") return false;
      if (filter === "sessions" && i.kind !== "session") return false;
      if (filter === "drafts" && !i.draft) return false;
      if (filter === "favorites" && !i.favorite) return false;
      return !q || i.title.toLowerCase().includes(q) || i.game.toLowerCase().includes(q);
    });
  });

  const groups = $derived.by(() => {
    const out: { label: string; items: Item[] }[] = [];
    for (const item of visible) {
      const label = day(item.created);
      const last = out[out.length - 1];
      if (last?.label === label) last.items.push(item);
      else out.push({ label, items: [item] });
    }
    return out;
  });

  const shots = $derived(visible.filter((i) => i.kind === "screenshot"));

  const clipKey = $derived(store.settings?.hotkeys.clip.replace("+", " + ") ?? "Alt + C");

  const totalSize = $derived(visible.reduce((sum, i) => sum + i.size, 0));
  const picked = $derived(visible.filter((i) => selected.includes(i.id)));
  const pickedSize = $derived(picked.reduce((sum, i) => sum + i.size, 0));

  // A different list starts a fresh selection.
  $effect(() => {
    void filter;
    void game;
    stopSelecting();
  });

  function open(item: Item) {
    if (item.kind === "screenshot") viewing = shots.indexOf(item);
    else store.go({ name: "editor", id: item.id });
  }

  function select(item: Item, e: MouseEvent) {
    selecting = true;
    if (e.shiftKey && anchor) {
      const ids = visible.map((i) => i.id);
      const [a, b] = [ids.indexOf(anchor), ids.indexOf(item.id)].sort((x, y) => x - y);
      if (a >= 0) {
        selected = [...new Set([...selected, ...ids.slice(a, b + 1)])];
        return;
      }
    }
    selected = selected.includes(item.id) ? selected.filter((id) => id !== item.id) : [...selected, item.id];
    anchor = item.id;
  }

  function stopSelecting() {
    selecting = false;
    selected = [];
    anchor = null;
  }

  async function favoritePicked() {
    const on = picked.some((i) => !i.favorite);
    await Promise.all(picked.map((i) => api.setFavorite(i.id, on)));
    store.notify(on ? "Added to favorites" : "Removed from favorites", `${picked.length} ${picked.length === 1 ? "item" : "items"}`);
  }

  async function copyPicked() {
    await api.copyItems(picked.map((i) => i.id));
    store.notify("Copied", "Paste into Discord, a chat or a folder.");
  }

  async function deletePicked() {
    const count = picked.length;
    await api.deleteItems(picked.map((i) => i.id));
    stopSelecting();
    store.notify("Moved to the recycle bin", `${count} ${count === 1 ? "item" : "items"}`);
  }

  function onKey(e: KeyboardEvent) {
    const typing = (e.target as HTMLElement).closest("input, textarea, select");
    if (viewing !== null) return;
    if (e.key === "Escape") {
      if (selecting) stopSelecting();
    } else if (!typing && (e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "a" && visible.length) {
      e.preventDefault();
      selecting = true;
      selected = visible.map((i) => i.id);
    } else if (!typing && e.key === "Delete" && picked.length) {
      deletePicked();
    }
  }
</script>

<svelte:window onkeydown={onKey} />

<section class="library">
  <header>
    {#if selecting}
      <h1>{selected.length ? `${selected.length} selected` : "Select clips"}</h1>
      <span class="meta">{selected.length ? size(pickedSize) : "Click to pick, Shift for a range"}</span>
      <div class="tools">
        {#if picked.length}
          <div class="actions" role="toolbar" aria-label="Selected clips">
            <button type="button" onclick={copyPicked}><Icon name="copy" size={16} />Copy</button>
            <button type="button" onclick={favoritePicked}><Icon name="star" size={16} />Favorite</button>
            <button type="button" class="danger" onclick={deletePicked}><Icon name="trash" size={16} />Recycle bin</button>
          </div>
        {/if}
        <button type="button" class="btn ghost small" onclick={() => (selected = visible.map((i) => i.id))}>Select all</button>
        <button type="button" class="btn small" onclick={stopSelecting}>Done</button>
      </div>
    {:else}
      <h1>{game ?? titles[filter]}</h1>
      {#if visible.length}
        <span class="meta mono">{visible.length} · {size(totalSize)}</span>
      {/if}
      <div class="tools">
        {#if visible.length}
          <button type="button" class="btn ghost small" onclick={() => (selecting = true)}>Select</button>
        {/if}
        <label class="search">
          <Icon name="search" size={16} />
          <input type="search" placeholder="Search clips" aria-label="Search clips" spellcheck="false" bind:value={query} />
          {#if query}
            <button type="button" class="clear" aria-label="Clear search" onclick={() => (query = "")}>
              <Icon name="close" size={14} />
            </button>
          {/if}
        </label>
      </div>
    {/if}
  </header>

  <div class="scroll body">
    {#if store.loaded && visible.length === 0}
      <div class="empty">
        <span class="empty-mark"><Mark size={64} /></span>
        {#if store.items.length === 0}
          <h2>No clips yet</h2>
          <p>Play something and press <kbd>{clipKey}</kbd> to keep the last {store.settings?.bufferSeconds ?? 30} seconds.</p>
        {:else if query}
          <h2>Nothing matches “{query}”</h2>
          <p>Try a game name or part of a title.</p>
        {:else}
          <h2>Nothing here yet</h2>
          <p>
            {filter === "favorites"
              ? "Star a clip to find it here."
              : filter === "drafts"
                ? "Clips you save as a draft in the editor show up here."
                : "Your clips will show up here."}
          </p>
        {/if}
      </div>
    {/if}

    {#each groups as group (group.label)}
      <section class="group" aria-label={group.label}>
        <div class="group-head">
          <h2>{group.label}</h2>
          <span class="mono meta">{group.items.length}</span>
        </div>
        <div class="grid">
          {#each group.items as item (item.id)}
            <ClipTile {item} onopen={open} onselect={select} {selecting} selected={selected.includes(item.id)} />
          {/each}
        </div>
      </section>
    {/each}
  </div>
</section>

{#if viewing !== null && shots.length}
  <ImageViewer items={shots} bind:index={viewing} onclose={() => (viewing = null)} />
{/if}

<style>
  .library {
    position: relative;
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  header {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 44px;
    padding: 0 6px 8px;
  }
  header > .meta {
    white-space: nowrap;
  }
  .tools {
    margin-left: auto;
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .actions {
    display: flex;
    gap: 2px;
    padding: 3px;
    margin-right: 6px;
    border-radius: 14px;
    background: var(--overlay);
    animation: appear 180ms var(--travel);
  }
  @keyframes appear {
    from {
      opacity: 0;
      transform: translateY(-4px);
    }
  }
  .actions button {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 34px;
    padding: 0 12px;
    border: 0;
    border-radius: 11px;
    background: transparent;
    color: var(--ink);
    font-weight: 500;
    white-space: nowrap;
  }
  .actions button:hover {
    background: var(--hover);
  }
  .actions .danger {
    color: var(--red);
  }
  h1 {
    margin: 0;
    font-size: 16px;
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .search {
    display: flex;
    align-items: center;
    gap: 10px;
    flex: 0 1 280px;
    min-width: 160px;
    height: 32px;
    padding: 0 12px;
    border-radius: 9px;
    background: var(--overlay);
    color: var(--ghost);
  }
  .search:focus-within {
    color: var(--accent);
  }
  .search input {
    flex: 1;
    min-width: 0;
    border: 0;
    background: transparent;
    outline: none;
    color: var(--ink);
  }
  .search input::placeholder {
    color: var(--ghost);
  }
  .search input::-webkit-search-cancel-button {
    display: none;
  }
  .clear {
    display: grid;
    place-items: center;
    width: 24px;
    height: 24px;
    margin-right: -6px;
    border: 0;
    border-radius: 7px;
    background: transparent;
    color: var(--ghost);
  }
  .clear:hover {
    background: var(--surface);
    color: var(--ink);
  }
  .body {
    flex: 1;
    min-height: 0;
    padding: 0 6px 24px;
    display: flex;
    flex-direction: column;
    gap: 28px;
    mask-image: linear-gradient(to bottom, transparent 0, #000 16px, #000 calc(100% - 28px), transparent 100%);
  }
  .group {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .group-head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    padding: 16px 6px 0;
  }
  .group h2 {
    margin: 0;
    font-size: 14px;
    font-weight: 600;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(230px, 1fr));
    gap: 20px 16px;
  }
  .empty {
    margin: auto;
    max-width: 380px;
    padding: 48px 0;
    text-align: center;
    display: flex;
    flex-direction: column;
    align-items: center;
  }
  .empty-mark {
    width: 96px;
    height: 96px;
    border-radius: 24px;
    background: var(--black);
    display: grid;
    place-items: center;
    margin-bottom: 18px;
  }
  .empty h2 {
    margin: 0 0 6px;
    font-size: 14px;
    font-weight: 600;
  }
  .empty p {
    margin: 0;
    color: var(--muted);
  }
  kbd {
    font-family: var(--mono);
    font-size: 12px;
    padding: 2px 6px;
    border-radius: 6px;
    background: var(--surface);
    color: var(--ink);
  }
</style>
