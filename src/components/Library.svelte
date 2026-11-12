<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { onDestroy } from "svelte";
  import { api, type Item, type PendingSave } from "../lib/api";
  import { day, size } from "../lib/format";
  import { store, type Filter } from "../lib/store.svelte";
  import { recycle, undo } from "../lib/undo";
  import { tip } from "../lib/tooltip";
  import ClipTile from "./ClipTile.svelte";
  import PendingTile from "./PendingTile.svelte";
  import LiveTile from "./LiveTile.svelte";
  import Icon from "./Icon.svelte";
  import ImageViewer from "./ImageViewer.svelte";
  import Mark from "./Mark.svelte";
  import TagEditor from "./TagEditor.svelte";
  import { chooseAndImport } from "../lib/importing";
  import { t, tParts } from "../lib/i18n.svelte";
  import { isWindows } from "../lib/platform";

  let { filter, game, tag = null }: { filter: Filter; game: string | null; tag?: string | null } = $props();
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
  const wears = (i: Item, name: string) => (i.tags ?? []).some((x) => x.toLowerCase() === name.toLowerCase());

  const visible = $derived.by(() => {
    const q = query.trim().toLowerCase();
    // "#win" looks for the tag; plain words find titles, games and tags alike.
    const word = q.replace(/^#/, "");
    return store.items.filter((i) => {
      if (game && i.game !== game) return false;
      if (tag && !wears(i, tag)) return false;
      if (filter === "clips" && i.kind !== "clip") return false;
      if (filter === "screenshots" && i.kind !== "screenshot") return false;
      if (filter === "sessions" && i.kind !== "session") return false;
      if (filter === "drafts" && !i.draft) return false;
      if (filter === "favorites" && !i.favorite) return false;
      if (!q) return true;
      if (q.startsWith("#")) return (i.tags ?? []).some((x) => x.toLowerCase().includes(word));
      return i.title.toLowerCase().includes(q) || i.game.toLowerCase().includes(q) || (i.tags ?? []).some((x) => x.toLowerCase().includes(word));
    });
  });

  /** Saves still being written that belong in this list. */
  const waiting = $derived.by(() => {
    const q = query.trim().toLowerCase();
    return store.placeholders.filter((p) => {
      // A clip still being saved has no tags yet.
      if (tag) return false;
      if (game && p.game !== game) return false;
      if (filter === "clips" && p.kind !== "clip") return false;
      if (filter === "sessions" && p.kind !== "session") return false;
      if (filter === "screenshots" || filter === "drafts" || filter === "favorites") return false;
      return !q || p.title.toLowerCase().includes(q) || p.game.toLowerCase().includes(q);
    });
  });

  /** The session being recorded right now, if it belongs in this list. */
  const live = $derived.by(() => {
    const { sessionSince: since, target } = store.status;
    if (!since || tag || (filter !== "all" && filter !== "sessions")) return null;
    const name = target ?? "";
    if (game && name !== game) return null;
    const title = `${name} session`.trim();
    const q = query.trim().toLowerCase();
    if (q && !title.toLowerCase().includes(q)) return null;
    return { since, game: name, title };
  });

  type Entry =
    | { key: string; live: NonNullable<typeof live> }
    | { key: string; save: PendingSave }
    | { key: string; item: Item };

  const groups = $derived.by(() => {
    const out: { label: string; entries: Entry[] }[] = [];
    const add = (label: string, entry: Entry) => {
      const last = out[out.length - 1];
      if (last?.label === label) last.entries.push(entry);
      else out.push({ label, entries: [entry] });
    };
    // What is being recorded and saved is the newest, so it leads its day.
    if (live) add(day(Date.now()), { key: "live", live });
    for (const save of waiting) add(day(save.created), { key: save.id, save });
    for (const item of visible) add(day(item.created), { key: item.id, item });
    return out;
  });

  // Only the tiles near the window exist. A library of thousands of clips would otherwise
  // be thousands of tiles and pictures held in memory, most of them never looked at. Each
  // day is cut into blocks of a few rows; a block away from view is an empty box of the
  // height its tiles would have, so the scroll bar and position stay as they are.
  const GAP_X = 18;
  const GAP_Y = 26;
  const MIN_TILE = 230;
  const ROWS = 3;
  /** Under a thumbnail: the gap, the title and the line below it. */
  const TEXT = 52;

  let body = $state<HTMLElement>();
  let width = $state(0);
  const cols = $derived(Math.max(1, Math.floor((width + GAP_X) / (MIN_TILE + GAP_X))));
  /** A row is as high as a 16:9 thumbnail of the column's width and the text under it. The
      grid is given this height too, so a box away from view is exactly as tall as its tiles. */
  const rowHeight = $derived(((Math.max(width, MIN_TILE) - (cols - 1) * GAP_X) / cols) * (9 / 16) + TEXT);

  const days = $derived.by(() => {
    const per = cols * ROWS;
    let index = 0;
    return groups.map((group) => {
      const blocks: { key: string; index: number; rows: number; entries: Entry[] }[] = [];
      for (let i = 0; i < group.entries.length; i += per) {
        const entries = group.entries.slice(i, i + per);
        blocks.push({ key: `${group.label}#${i / per}`, index: index++, rows: Math.ceil(entries.length / cols), entries });
      }
      return { label: group.label, count: group.entries.length, blocks };
    });
  });

  /** Blocks in or near the window, by key, as the observer last saw them. */
  let near = $state<Record<string, boolean>>({});
  let observer: IntersectionObserver | undefined;
  const blockKeys = new WeakMap<Element, string>();

  /** Until the observer has looked at a block, the first ones count as in view. */
  const shown = (block: { key: string; index: number }) => near[block.key] ?? block.index < 2;

  function watch(node: HTMLElement, key: string) {
    observer ??= new IntersectionObserver(
      (seen) => {
        for (const entry of seen) {
          const k = blockKeys.get(entry.target);
          if (k !== undefined && near[k] !== entry.isIntersecting) near[k] = entry.isIntersecting;
        }
      },
      // A screen or so ahead in both directions, so scrolling does not show empty boxes.
      { root: node.closest(".body"), rootMargin: "900px 0px" },
    );
    blockKeys.set(node, key);
    observer.observe(node);
    return {
      update(next: string) {
        blockKeys.set(node, (key = next));
      },
      destroy() {
        observer?.unobserve(node);
        delete near[key];
      },
    };
  }

  $effect(() => {
    const node = body;
    if (!node) return;
    const measure = () => {
      const style = getComputedStyle(node);
      width = node.clientWidth - parseFloat(style.paddingLeft) - parseFloat(style.paddingRight);
    };
    measure();
    const resize = new ResizeObserver(measure);
    resize.observe(node);
    return () => {
      resize.disconnect();
      observer?.disconnect();
      observer = undefined;
    };
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
    void tag;
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

  /** Where the tag panel for the selection opened, while it is open. */
  let tagging = $state<DOMRect | null>(null);
  /** Every tag any picked clip wears; adding or removing one changes all of them. */
  const pickedTags = $derived.by(() => {
    const out: string[] = [];
    for (const i of picked) for (const x of i.tags ?? []) if (!out.some((y) => y.toLowerCase() === x.toLowerCase())) out.push(x);
    return out;
  });

  function tagPicked(add: string[], remove: string[]) {
    api.tagItems(picked.map((i) => i.id), add, remove).catch((e) => store.notify(t("Tags not saved"), String(e)));
  }

  /** A montage being made from picked clips: its ticket, how far it got and how many clips. */
  let combining = $state<{ ticket: number; progress: number; count: number } | null>(null);
  /** Pictures cannot go into a montage. */
  const combinable = $derived(picked.filter((i) => i.kind !== "screenshot"));
  const stopProgress = listen<[number, number]>("export-progress", (e) => {
    if (combining && e.payload[0] === combining.ticket) combining.progress = e.payload[1];
  });
  onDestroy(() => stopProgress.then((f) => f()));

  async function combinePicked() {
    const ids = combinable.map((i) => i.id);
    const ticket = Date.now() * 1000 + Math.floor(Math.random() * 1000);
    combining = { ticket, progress: 0, count: ids.length };
    stopSelecting();
    try {
      const id = await api.combineClips(ids, t("Montage"), ticket);
      store.notify(t("Montage saved"), t("{n} clips, one after another", { n: ids.length }), {
        label: t("Open"),
        run: () => store.go({ name: "editor", id }),
      });
    } catch (e) {
      if (String(e) !== "Export cancelled") store.notify(t("Couldn't combine the clips"), String(e));
    } finally {
      combining = null;
    }
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
            <button type="button" aria-haspopup="dialog" onclick={(e) => (tagging = e.currentTarget.getBoundingClientRect())}>
              <Icon name="tag" size={16} />{t("Tag")}
            </button>
            {#if combinable.length >= 2 && !combining}
              <button type="button" use:tip={t("Join them into one clip, oldest first")} onclick={combinePicked}>
                <Icon name="combine" size={16} />{t("Combine")}
              </button>
            {/if}
            <button type="button" class="danger" onclick={deletePicked}><Icon name="trash" size={16} />{isWindows ? t("Recycle bin") : t("Trash")}</button>
          </div>
        {/if}
        <button type="button" class="btn ghost small" onclick={() => (selected = visible.map((i) => i.id))}>{t("Select all")}</button>
        <button type="button" class="btn small" onclick={stopSelecting}>{t("Done")}</button>
      </div>
    {:else}
      <h1>{tag !== null ? `#${tag}` : game === null ? titles[filter] : gameLabel(game)}</h1>
      {#if visible.length}
        <span class="chip mono">{t("{n} clips", { n: visible.length })}</span>
        <span class="chip mono">{size(totalSize)}</span>
      {/if}
      <div class="tools">
        {#if combining}
          <span class="chip mono" role="status">{t("Combining {n} clips… {percent}%", { n: combining.count, percent: Math.round(combining.progress * 100) })}</span>
          <button type="button" class="btn ghost small" onclick={() => combining && api.cancelExport(combining.ticket)}>{t("Cancel")}</button>
        {/if}
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

  <div class="scroll body" bind:this={body} style:--cols={cols} style:--row="{rowHeight}px">
    {#if store.loaded && visible.length === 0 && waiting.length === 0 && !live}
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

    {#each days as group (group.label)}
      <section class="group" aria-label={group.label}>
        <div class="group-head">
          <h2>{group.label}</h2>
          <span class="ring mono">{group.count}</span>
        </div>
        {#each group.blocks as block (block.key)}
          <!-- One element in view or not, so the observer keeps watching the same box. -->
          <div
            class="grid block"
            class:shown={shown(block)}
            use:watch={block.key}
            style:height={shown(block) ? undefined : `${block.rows * rowHeight + (block.rows - 1) * GAP_Y}px`}
          >
            {#if shown(block)}
              {#each block.entries as entry (entry.key)}
                {#if "live" in entry}
                  <LiveTile {...entry.live} />
                {:else if "save" in entry}
                  <PendingTile save={entry.save} />
                {:else}
                  <ClipTile item={entry.item} onopen={open} onselect={select} {selecting} selected={selectedIds.has(entry.item.id)} />
                {/if}
              {/each}
            {/if}
          </div>
        {/each}
      </section>
    {/each}
  </div>
</section>

{#if tagging && picked.length}
  <TagEditor
    tags={pickedTags}
    anchor={tagging}
    onadd={(name) => tagPicked([name], [])}
    onremove={(name) => tagPicked([], [name])}
    onclose={() => (tagging = null)}
  />
{/if}

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
  .group {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  /* A few rows of a day. Away from view it is an empty box of the same height (see `days`);
     the gap between blocks is the gap between rows, so a day reads as one grid. */
  .block {
    flex: none;
    contain: layout style;
  }
  .block + .block {
    margin-top: 14px;
  }
  .block:not(.shown) {
    contain: strict;
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
  /* The column count comes from the script, which sizes the empty boxes by it. */
  .grid {
    display: grid;
    grid-template-columns: repeat(var(--cols, 4), minmax(0, 1fr));
    grid-auto-rows: var(--row, auto);
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
