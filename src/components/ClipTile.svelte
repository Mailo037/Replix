<script lang="ts">
  import { startDrag } from "@crabnebula/tauri-plugin-drag";
  import { onDestroy, tick } from "svelte";
  import { api, fileUrl, type Item } from "../lib/api";
  import { duration, gameColor, time } from "../lib/format";
  import { share } from "../lib/share";
  import { recycle, rename } from "../lib/undo";
  import { links, remaining } from "../lib/links.svelte";
  import { store } from "../lib/store.svelte";
  import { fit } from "../lib/place";
  import { tip } from "../lib/tooltip";
  import { t } from "../lib/i18n.svelte";
  import { isWindows } from "../lib/platform";
  import Icon from "./Icon.svelte";
  import TagEditor from "./TagEditor.svelte";

  let {
    item,
    onopen,
    onselect,
    selecting = false,
    selected = false,
  }: {
    item: Item;
    onopen: (item: Item) => void;
    /** Ctrl, Shift or selection mode: pick the tile instead of opening it. */
    onselect: (item: Item, event: MouseEvent) => void;
    selecting?: boolean;
    selected?: boolean;
  } = $props();

  let loaded = $state(false);
  // Passing over a row of tiles on the way somewhere is no hint; resting on one is.
  let resting: ReturnType<typeof setTimeout> | undefined;
  onDestroy(() => clearTimeout(resting));
  let menu = $state(false);
  /** The menu is built the first time it opens; most tiles never need one. */
  let menuMounted = $state(false);
  let menuEl = $state<HTMLDivElement>();
  let moreBtn = $state<HTMLButtonElement>();
  /** The menu was open when the ⋯ button was pressed, so the click closes it rather than reopening. */
  let wasOpen = false;

  const thumbUrl = $derived(item.thumb ? fileUrl(item.thumb) : null);
  const isNew = $derived(!item.seen && Date.now() - item.created < 86_400_000 && item.kind !== "screenshot");
  const badge = $derived(isNew ? "new" : item.draft ? "draft" : item.kind === "session" ? "session" : null);
  const uploading = $derived(!!links.uploading[item.path]);
  const link = $derived(links.of(item.path));
  const label = $derived(item.kind === "screenshot" ? "PNG" : duration(item.duration));

  const gameLabel = (name: string) => (name === "Desktop" ? t("Desktop") : name);

  // At the cursor on right-click, under the ⋯ button otherwise; always inside the window.
  async function showMenu(at: DOMRect, align: "start" | "end") {
    if (!menuMounted) {
      menuMounted = true;
      await tick();
    }
    if (!menuEl) return;
    if (!menuEl.matches(":popover-open")) menuEl.showPopover();
    fit(menuEl, at, { align, gap: align === "start" ? 2 : 6 });
    menuEl.querySelector<HTMLElement>("button")?.focus();
  }

  $effect(() => {
    if (!menuEl) return;
    const node = menuEl;
    const sync = (e: Event) => (menu = (e as ToggleEvent).newState === "open");
    node.addEventListener("toggle", sync);
    return () => node.removeEventListener("toggle", sync);
  });

  /** Where the tag panel opened, while it is open. */
  let tagging = $state<DOMRect | null>(null);
  const tags = $derived(item.tags ?? []);

  let renaming = $state(false);
  let draftTitle = $state("");

  function focusSelect(node: HTMLInputElement) {
    node.focus();
    node.select();
  }

  async function finishRename(keep: boolean) {
    if (!renaming) return;
    renaming = false;
    const to = draftTitle.trim();
    if (keep) await rename(item.id, item.title, to).catch((e) => store.notify(t("Couldn't rename"), String(e)));
  }

  async function act(what: "favorite" | "reveal" | "delete" | "copy" | "link" | "rename" | "tags") {
    menuEl?.hidePopover();
    if (what === "tags") tagging = (moreBtn ?? menuEl)?.getBoundingClientRect() ?? null;
    if (what === "favorite") await api.setFavorite([item.id], !item.favorite);
    if (what === "reveal") await api.revealItem(item.id);
    if (what === "copy") await share.copy(item.path);
    if (what === "link") await share.link(item.path);
    if (what === "rename") {
      draftTitle = item.title;
      renaming = true;
    }
    if (what === "delete") await recycle([item.id], item.title);
  }

  // Dragging a tile hands the file to the system, so it drops into Discord or a folder.
  function drag(e: DragEvent) {
    const icon = item.thumb ?? (item.kind === "screenshot" ? item.path : null);
    if (!icon || selecting) return;
    e.preventDefault();
    startDrag({ item: [item.path], icon }).catch((err) => console.warn("drag failed", err));
  }
</script>

<div
  class="tile"
  class:selected
  role="group"
  aria-label={item.title}
  oncontextmenu={(e) => {
    if (selecting) return;
    e.preventDefault();
    showMenu(new DOMRect(e.clientX, e.clientY, 0, 0), "start");
  }}
>
  <button
    type="button"
    class="open"
    onclick={(e) => (selecting || e.ctrlKey || e.shiftKey ? onselect(item, e) : onopen(item))}
    aria-label={selecting ? t("Select {title}", { title: item.title }) : t("Open {title}", { title: item.title })}
    aria-pressed={selecting ? selected : undefined}
    draggable="true"
    ondragstart={drag}
    onpointerenter={() => (resting = setTimeout(() => store.warm(item), 350))}
    onpointerleave={() => clearTimeout(resting)}
  >
    {#if !selecting && badge}
      <span class="sticker {badge}">{badge === "new" ? t("new") : badge === "draft" ? t("draft") : t("session")}</span>
    {/if}
    <span class="thumb">
      <!-- Hidden in the tray, the pictures are let go so WebView2 can free them. -->
      {#if thumbUrl && store.visible}
        <img src={thumbUrl} alt="" class="real" class:loaded loading="lazy" decoding="async" onload={() => (loaded = true)} />
      {/if}
      {#if selecting}
        <span class="check" class:on={selected} aria-hidden="true">{#if selected}<Icon name="check" size={14} />{/if}</span>
      {/if}
      {#if item.favorite}
        <span class="fav" aria-label={t("Favorite")}><Icon name="star" size={14} filled /></span>
      {/if}
      {#if uploading}
        <span class="label badge top uploading" aria-label={t("Uploading")}><Icon name="link" size={12} />{t("Uploading…")}</span>
      {/if}
      <span class="label badge bottom">{label}</span>
    </span>
    <span class="text">
      <span class="title" class:hidden={renaming}>{item.title}</span>
      <span class="meta">
        {#if item.game}<span class="swatch" style:background={gameColor(item.game)} aria-hidden="true"></span>{/if}
        <span class="when">{item.game && !item.title.includes(item.game) ? `${gameLabel(item.game)} · ` : ""}{time(item.created)}</span>
        {#each tags.slice(0, 2) as tag (tag)}<span class="tag">#{tag}</span>{/each}
        {#if tags.length > 2}<span class="tag more-tags">+{tags.length - 2}</span>{/if}
      </span>
    </span>
  </button>

  {#if renaming}
    <input
      class="rename"
      aria-label={t("Name")}
      spellcheck="false"
      bind:value={draftTitle}
      use:focusSelect
      onkeydown={(e) => {
        e.stopPropagation();
        if (e.key === "Enter") finishRename(true);
        else if (e.key === "Escape") finishRename(false);
      }}
      onblur={() => finishRename(true)}
    />
  {/if}
  {#if !selecting}
    <button type="button" class="more" bind:this={moreBtn}
      aria-label={t("More actions")}
      use:tip={t("More")}
      aria-expanded={menu}
      onpointerdown={() => (wasOpen = menu)}
      onclick={() => {
        if (wasOpen) wasOpen = false;
        else if (moreBtn) showMenu(moreBtn.getBoundingClientRect(), "end");
      }}
    >
      <Icon name="more" size={18} />
    </button>
  {/if}
  {#if !selecting && menuMounted}
    <div class="menu" role="menu" popover="auto" bind:this={menuEl}>
      <button type="button" role="menuitem" onclick={() => act("copy")}>
        <Icon name="copy" size={16} />{t("Copy")}
      </button>
      <button type="button" role="menuitem" disabled={uploading} onclick={() => act("link")}>
        {#if uploading}
          <Icon name="link" size={16} />{t("Uploading…")} <span class="hint pulse">●</span>
        {:else if link}
          <Icon name="link" size={16} />{t("Copy link")} <span class="hint">{t("{time} left", { time: remaining(link) })}</span>
        {:else}
          <Icon name="link" size={16} />{t("Get a link")} <span class="hint">{t("72 h")}</span>
        {/if}
      </button>
      <span class="sep" role="separator"></span>
      <button type="button" role="menuitem" onclick={() => act("favorite")}>
        <Icon name="star" size={16} filled={item.favorite} />{item.favorite ? t("Remove favorite") : t("Add to favorites")}
      </button>
      <button type="button" role="menuitem" onclick={() => act("rename")}><Icon name="edit" size={16} />{t("Rename")}</button>
      <button type="button" role="menuitem" onclick={() => act("tags")}>
        <Icon name="tag" size={16} />{t("Tags")}{#if tags.length}<span class="hint">{tags.length}</span>{/if}
      </button>
      <button type="button" role="menuitem" onclick={() => act("reveal")}><Icon name="folder" size={16} />{t("Show in folder")}</button>
      <button type="button" role="menuitem" class="danger" onclick={() => act("delete")}>
        <Icon name="trash" size={16} />{isWindows ? t("Move to recycle bin") : t("Move to trash")}
      </button>
    </div>
  {/if}
  {#if tagging}
    <TagEditor
      {tags}
      anchor={tagging}
      onadd={(tag) => api.tagItems([item.id], [tag], []).catch((e) => store.notify(t("Tags not saved"), String(e)))}
      onremove={(tag) => api.tagItems([item.id], [], [tag]).catch((e) => store.notify(t("Tags not saved"), String(e)))}
      onclose={() => (tagging = null)}
    />
  {/if}
</div>


<style>
  .tile {
    position: relative;
  }
  .open {
    width: 100%;
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 0;
    border: 0;
    background: transparent;
    text-align: left;
    border-radius: var(--r-thumb);
  }
  /* Stuck over the thumbnail's top-left corner, poking out a little. */
  .sticker {
    position: absolute;
    top: -7px;
    left: -7px;
    z-index: 2;
    transition: transform 220ms var(--travel);
  }
  .open:hover .sticker {
    transform: rotate(-8deg) scale(1.05);
  }
  .sticker.draft {
    background: var(--pink);
    color: var(--on-sticker);
  }
  .sticker.session {
    background: var(--lavender);
    color: var(--on-sticker);
  }
  .thumb {
    position: relative;
    display: block;
    aspect-ratio: 16 / 9;
    border-radius: var(--r-thumb);
    overflow: hidden;
    background: var(--black);
    transition: transform 220ms var(--travel);
  }
  .open:hover .thumb {
    transform: scale(1.025);
  }
  .selected .thumb {
    outline: 2px solid var(--accent);
    outline-offset: 3px;
    transform: scale(0.97);
  }
  .selected .open:hover .thumb {
    transform: scale(0.97);
  }
  .check {
    position: absolute;
    top: 10px;
    left: 10px;
    width: 24px;
    height: 24px;
    border-radius: 50%;
    border: 2px solid #fff;
    background: color-mix(in srgb, var(--canvas) 50%, transparent);
    color: var(--on-accent);
    display: grid;
    place-items: center;
  }
  .check.on {
    border-color: var(--accent);
    background: var(--accent);
  }
  .real {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
    opacity: 0;
    transition: opacity 260ms ease;
  }
  .real.loaded {
    opacity: 1;
  }
  .label {
    position: absolute;
    padding: 3px 9px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--canvas) 82%, transparent);
    color: var(--ink);
  }
  .label.top {
    left: 10px;
    top: 10px;
  }
  .uploading {
    display: flex;
    /* Looping: stands still while Replix is in the background (see app.css). */
    align-items: center;
    gap: 6px;
    animation: pulse 1.2s ease-in-out infinite;
  }
  .pulse {
    color: var(--accent);
    animation: pulse 1.2s ease-in-out infinite;
  }
  @keyframes pulse {
    50% {
      opacity: 0.45;
    }
  }
  .label.bottom {
    right: 10px;
    bottom: 10px;
  }
  .fav {
    position: absolute;
    top: 10px;
    right: 10px;
    width: 28px;
    height: 28px;
    border-radius: 50%;
    background: var(--canvas);
    color: var(--accent);
    display: grid;
    place-items: center;
  }
  .text {
    display: flex;
    flex-direction: column;
    padding: 0 4px;
    min-width: 0;
  }
  .meta {
    display: flex;
    align-items: center;
    gap: 7px;
    min-width: 0;
    margin-top: 2px;
    font-size: 12.5px;
    white-space: nowrap;
    overflow: hidden;
  }
  .when {
    flex: none;
  }
  .tag {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    padding: 0 7px;
    border-radius: 999px;
    background: var(--surface);
    color: var(--muted);
    font-size: 11.5px;
    font-weight: 600;
    line-height: 18px;
  }
  .more-tags {
    flex: none;
  }
  .swatch {
    width: 8px;
    height: 8px;
    flex: none;
    border-radius: 3px;
  }
  .title {
    font-size: 15px;
    font-weight: 700;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .title.hidden {
    visibility: hidden;
  }
  .rename {
    position: absolute;
    left: 0;
    right: 36px;
    bottom: 19px;
    height: 28px;
    padding: 0 4px;
    border: 0;
    border-radius: 8px;
    background: var(--surface);
    color: var(--ink);
    font: inherit;
    font-size: 15px;
    font-weight: 700;
    outline: 2px solid var(--accent);
  }
  .more {
    position: absolute;
    right: 2px;
    bottom: 2px;
    width: 32px;
    height: 32px;
    border: 0;
    border-radius: 50%;
    background: transparent;
    color: var(--ghost);
    display: grid;
    place-items: center;
    opacity: 0;
    transition: opacity 160ms ease, background-color 160ms ease;
  }
  .tile:hover .more,
  .more:focus-visible,
  .more[aria-expanded="true"] {
    opacity: 1;
  }
  .more:hover {
    background: var(--surface);
    color: var(--ink);
  }
  .menu {
    position: fixed;
    inset: auto;
    margin: 0;
    min-width: 220px;
    padding: 6px;
    border: 0;
    border-radius: 20px;
    background: var(--raised);
    color: var(--ink);
    box-shadow:
      0 0 0 1px rgb(255 255 255 / 0.04),
      0 14px 36px rgb(0 0 0 / 0.5);
  }
  .menu {
    flex-direction: column;
    gap: 2px;
    opacity: 0;
    transform: translateY(-4px);
    transition:
      opacity 140ms ease,
      transform 140ms var(--travel),
      display 140ms allow-discrete,
      overlay 140ms allow-discrete;
  }
  .menu:popover-open {
    display: flex;
    flex-direction: column;
    gap: 2px;
    opacity: 1;
    transform: none;
  }
  @starting-style {
    .menu:popover-open {
      opacity: 0;
      transform: translateY(-4px);
    }
  }
  .menu button {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 34px;
    padding: 0 12px;
    border: 0;
    border-radius: 14px;
    background: transparent;
    color: var(--ink);
    font-weight: 600;
    text-align: left;
  }
  .menu button:disabled {
    color: var(--muted);
    cursor: default;
  }
  .menu button:hover:not(:disabled) {
    background: var(--hover);
  }
  .menu .danger {
    color: var(--red);
  }
  .hint {
    margin-left: auto;
    font-size: 12px;
    color: var(--ghost);
  }
  .sep {
    height: 1px;
    margin: 2px 8px;
    background: var(--line);
  }
</style>
