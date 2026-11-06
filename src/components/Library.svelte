<script lang="ts">
  import { api, type Item, type PendingSave } from "../lib/api";
  import { day, size } from "../lib/format";
  import { store, type Filter } from "../lib/store.svelte";
  import { recycle, undo } from "../lib/undo";
  import { tip } from "../lib/tooltip";
  import ClipTile from "./ClipTile.svelte";
  import PendingTile from "./PendingTile.svelte";
  import Icon from "./Icon.svelte";
  import ImageViewer from "./ImageViewer.svelte";
  import Mark from "./Mark.svelte";
  import { chooseAndImport } from "../lib/importing";
  import { t, tParts } from "../lib/i18n.svelte";
  import { isWindows } from "../lib/platform";

  let { filter, game }: { filter: Filter; game: string | null } = $props();
  let query = $state("");
  /** Which screenshot is open in the viewer. */
  let viewing = $state<number | null>(null);
  let selecting = $state(false);
  let selected = $state<string[]>([]);
  /** Where a Shift-click range starts. */
  let anchor: string | null = null;

  const titles = $derived<Record<Filter, string>>({
    all: t("All clips"),
    clips: t("Clips"),
    screenshots: t("Screenshots"),
    sessions: t("Sessions"),
    drafts: t("Drafts"),
    favorites: t("Favorites"),
  });

  const gameLabel = (name: string) => (name === "Desktop" ? t("Desktop") : name);

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

  /** Saves still being written that belong in this list. */
  const waiting = $derived.by(() => {
    const q = query.trim().toLowerCase();
    return store.placeholders.filter((p) => {
      if (game && p.game !== game) return false;
      if (filter === "clips" && p.kind !== "clip") return false;
      if (filter === "sessions" && p.kind !== "session") return false;
      if (filter === "screenshots" || filter === "drafts" || filter === "favorites") return false;
      return !q || p.title.toLowerCase().includes(q) || p.game.toLowerCase().includes(q);
    });
  });

  const groups = $derived.by(() => {
    const out: { label: string; pending: PendingSave[]; items: Item[] }[] = [];
    // Saves in progress are the newest, so they lead their day.
    for (const save of waiting) {
      const label = day(save.created);
      const last = out[out.length - 1];
      if (last?.label === label) last.pending.push(save);
      else out.push({ label, pending: [save], items: [] });
    }
    for (const item of visible) {
      const label = day(item.created);
      const last = out[out.length - 1];
      if (last?.label === label) last.items.push(item);
      else out.push({ label, pending: [], items: [item] });
    }
    return out;
  });

  const shots = $derived(visible.filter((i) => i.kind === "screenshot"));

  const clipKey = $derived(store.settings?.hotkeys.clip.replace("+", " + ") ?? "Alt + C");

  const totalSize = $derived(visible.reduce((sum, i) => sum + i.size, 0));
  const selectedIds = $derived(new Set(selected));
  const picked = $derived(visible.filter((i) => selectedIds.has(i.id)));
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
    // Counted first: in Favorites, unstarred items leave the selection once the change lands.
    const count = picked.length;
    await api.setFavorite(picked.map((i) => i.id), on);
    store.notify(on ? t("Added to favorites") : t("Removed from favorites"), t("{n} items", { n: count }));
  }

  async function copyPicked() {
    await api.copyItems(picked.map((i) => i.id));
    store.notify(t("Copied"), t("Paste into Discord, a chat or a folder."));
  }

  async function deletePicked() {
    const count = picked.length;
    const ids = picked.map((i) => i.id);
    stopSelecting();
    await recycle(ids, count === 1 ? (picked[0]?.title ?? t("{n} items", { n: count })) : t("{n} items", { n: count }));
  }

  let search = $state<HTMLInputElement>();

  function onKey(e: KeyboardEvent) {
    const typing = (e.target as HTMLElement).closest("input, textarea, select");
    if (viewing !== null) return;
    const mod = e.ctrlKey || e.metaKey;
    if ((mod && !e.shiftKey && e.key.toLowerCase() === "f") || (!typing && e.key === "/")) {
      e.preventDefault();
      search?.focus();
      search?.select();
    } else if (mod && !e.shiftKey && e.key.toLowerCase() === "e" && visible.length) {
      e.preventDefault();
      if (selecting) stopSelecting();
      else selecting = true;
    } else if (e.key === "Escape" && typing === search) {
      if (query) query = "";
      else search?.blur();
    } else if (e.key === "Escape") {
      if (selecting) stopSelecting();
    } else if (!typing && (e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "a" && visible.length) {
      e.preventDefault();
      selecting = true;
      selected = visible.map((i) => i.id);
    } else if (!typing && (e.ctrlKey || e.metaKey) && !e.shiftKey && e.key.toLowerCase() === "z") {
      e.preventDefault();
      undo.last();
    } else if (!typing && e.key === "Delete" && picked.length) {
      deletePicked();
    }
  }
</script>

<svelte:window onkeydown={onKey} />

<section class="library">
  <header>
    {#if selecting}
      <h1>{selected.length ? t("{n} selected", { n: selected.length }) : t("Select clips")}</h1>
      <span class="meta">{selected.length ? size(pickedSize) : t("Click to pick, Shift for a range")}</span>
      <div class="tools">
        {#if picked.length}
          <div class="actions" role="toolbar" aria-label={t("Selected clips")}>
            <button type="button" onclick={copyPicked}><Icon name="copy" size={16} />{t("Copy")}</button>
            <button type="button" onclick={favoritePicked}><Icon name="star" size={16} />{t("Favorite")}</button>
            <button type="button" class="danger" onclick={deletePicked}><Icon name="trash" size={16} />{isWindows ? t("Recycle bin") : t("Trash")}</button>
          </div>
        {/if}
        <button type="button" class="btn ghost small" onclick={() => (selected = visible.map((i) => i.id))}>{t("Select all")}</button>
        <button type="button" class="btn small" onclick={stopSelecting}>{t("Done")}</button>
      </div>
    {:else}
      <h1>{game === null ? titles[filter] : gameLabel(game)}</h1>
      {#if visible.length}
        <span class="chip mono">{t("{n} clips", { n: visible.length })}</span>
        <span class="chip mono">{size(totalSize)}</span>
      {/if}
      <div class="tools">
        <button
          type="button"
          class="icon-btn"
          aria-label={t("Import videos and pictures")}
          use:tip={t("Import videos and pictures · or drop them here")}
          onclick={chooseAndImport}
        >
          <Icon name="import" />
        </button>
        {#if visible.length}
          <button type="button" class="btn" use:tip={t("Select · Ctrl+E")} onclick={() => (selecting = true)}>{t("Select")}</button>
        {/if}
        <label class="search">
          <Icon name="search" size={16} />
          <input type="search" placeholder={t("Search clips")} aria-label={t("Search clips")} spellcheck="false" bind:value={query} bind:this={search} />
          {#if !query}<kbd class="search-key mono" aria-hidden="true">Ctrl+F</kbd>{/if}
          {#if query}
            <button type="button" class="clear" aria-label={t("Clear search")} onclick={() => (query = "")}>
              <Icon name="close" size={14} />
            </button>
          {/if}
        </label>
      </div>
    {/if}
  </header>

  <div class="scroll body">
    {#if store.loaded && visible.length === 0 && waiting.length === 0}
      <div class="empty">
        <span class="empty-mark"><Mark size={64} /></span>
        {#if store.items.length === 0}
          <h2>{t("No clips yet")}</h2>
          <p>{#each tParts("Play something and press {key} to keep the last {n} seconds.", { n: store.settings?.bufferSeconds ?? 30 }) as part}{#if part === "{key}"}<kbd>{clipKey}</kbd>{:else}{part}{/if}{/each}</p>
          <button type="button" class="btn small" onclick={chooseAndImport}><Icon name="import" size={16} />{t("Import videos")}</button>
        {:else if query}
          <h2>{t("Nothing matches “{query}”", { query })}</h2>
          <p>{t("Try a game name or part of a title.")}</p>
        {:else}
          <h2>{t("Nothing here yet")}</h2>
          <p>
            {filter === "favorites"
              ? t("Star a clip to find it here.")
              : filter === "drafts"
                ? t("Clips you save as a draft in the editor show up here.")
                : t("Your clips will show up here.")}
          </p>
        {/if}
      </div>
    {/if}

    {#each groups as group (group.label)}
      <section class="group" aria-label={group.label}>
        <div class="group-head">
          <h2>{group.label}</h2>
          <span class="ring mono">{group.items.length + group.pending.length}</span>
        </div>
        <div class="grid">
          {#each group.pending as save (save.id)}
            <PendingTile {save} />
          {/each}
          {#each group.items as item (item.id)}
            <ClipTile {item} onopen={open} onselect={select} {selecting} selected={selectedIds.has(item.id)} />
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
    gap: 10px;
    min-height: 48px;
    padding: 0 12px 10px;
  }
  .chip {
    padding: 3px 10px;
    border-radius: 999px;
    background: var(--surface);
    font-size: 12px;
    white-space: nowrap;
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
    padding: 4px;
    margin-right: 6px;
    border-radius: 22px;
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
    padding: 0 14px;
    border: 0;
    border-radius: 18px;
    background: transparent;
    color: var(--ink);
    font-weight: 700;
    white-space: nowrap;
  }
  .actions button:hover {
    background: var(--hover);
  }
  .actions .danger {
    color: var(--red);
  }
  h1 {
    margin: 0 6px 0 0;
    font-size: 30px;
    font-weight: 800;
    letter-spacing: -0.035em;
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
    height: 38px;
    padding: 0 14px;
    border: 2px solid var(--line);
    border-radius: 999px;
    background: var(--overlay);
    color: var(--ghost);
    transition: border-color 160ms ease;
  }
  .search:focus-within {
    border-color: var(--accent);
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
  .search-key {
    flex: none;
    padding: 2px 7px;
    border-radius: 7px;
    background: var(--surface);
    color: var(--ghost);
    font-size: 11px;
    pointer-events: none;
  }
  .search:focus-within .search-key {
    display: none;
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
    border-radius: 50%;
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
    padding: 0 12px 24px;
    display: flex;
    flex-direction: column;
    gap: 24px;
    mask-image: linear-gradient(to bottom, transparent 0, #000 16px, #000 calc(100% - 28px), transparent 100%);
  }
  /* Days scrolled out of view are skipped by layout and paint, which keeps a big library
     smooth. The side padding leaves room for stickers, outlines and the hover zoom, which
     poke out of the tiles and would otherwise be clipped. */
  .group {
    display: flex;
    flex-direction: column;
    gap: 12px;
    margin-inline: -10px;
    padding-inline: 10px;
    content-visibility: auto;
    contain-intrinsic-size: auto 640px;
  }
  .group-head {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 16px 0 4px;
  }
  .group h2 {
    margin: 0;
    font-size: 21px;
    font-weight: 800;
  }
  .ring {
    min-width: 24px;
    height: 24px;
    padding: 0 7px;
    border: 2px solid var(--accent);
    border-radius: 999px;
    color: var(--accent);
    font-size: 12px;
    display: grid;
    place-items: center;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(230px, 1fr));
    gap: 26px 18px;
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
    border-radius: 32px;
    background: var(--overlay);
    display: grid;
    place-items: center;
    margin-bottom: 18px;
  }
  .empty h2 {
    margin: 0 0 6px;
    font-size: 20px;
    font-weight: 800;
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
