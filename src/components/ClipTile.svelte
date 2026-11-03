<script lang="ts">
  import { startDrag } from "@crabnebula/tauri-plugin-drag";
  import { api, fileUrl, type Item } from "../lib/api";
  import { duration, time } from "../lib/format";
  import { share } from "../lib/share";
  import { store } from "../lib/store.svelte";
  import { fit } from "../lib/place";
  import { tip } from "../lib/tooltip";
  import Icon from "./Icon.svelte";

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
  let menu = $state(false);
  let menuEl = $state<HTMLDivElement>();
  let moreBtn = $state<HTMLButtonElement>();
  /** The menu was open when the ⋯ button was pressed, so the click closes it rather than reopening. */
  let wasOpen = false;

  const thumbUrl = $derived(item.thumb ? fileUrl(item.thumb) : null);
  const isNew = $derived(!item.seen && Date.now() - item.created < 86_400_000 && item.kind !== "screenshot");
  const badge = $derived(isNew ? "[ NEW ]" : item.draft ? "[ DRAFT ]" : item.kind === "session" ? "[ SESSION ]" : null);
  const label = $derived(item.kind === "screenshot" ? "PNG" : duration(item.duration));

  // At the cursor on right-click, under the ⋯ button otherwise; always inside the window.
  function showMenu(at: DOMRect, align: "start" | "end") {
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

  async function act(what: "favorite" | "reveal" | "delete" | "copy" | "link") {
    menuEl?.hidePopover();
    if (what === "favorite") await api.setFavorite(item.id, !item.favorite);
    if (what === "reveal") await api.revealItem(item.id);
    if (what === "copy") await share.copy(item.path);
    if (what === "link") await share.link(item.path);
    if (what === "delete") {
      await api.deleteItems([item.id]);
      store.notify("Moved to the recycle bin", item.title);
    }
  }

  // Dragging a tile hands the file to Windows, so it drops into Discord or a folder.
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
    aria-label={selecting ? `Select ${item.title}` : `Open ${item.title}`}
    aria-pressed={selecting ? selected : undefined}
    draggable="true"
    ondragstart={drag}
    onpointerenter={() => item.kind !== "screenshot" && store.warm(item.id)}
  >
    <span class="thumb">
      {#if thumbUrl}
        <img src={thumbUrl} alt="" class="real" class:loaded loading="lazy" onload={() => (loaded = true)} />
      {/if}
      {#if selecting}
        <span class="label badge top check" class:accent={selected} aria-hidden="true">{selected ? "[×]" : "[ ]"}</span>
      {:else if badge}
        <span class="label badge top" class:accent={isNew}>{badge}</span>
      {/if}
      {#if item.favorite}
        <span class="fav" aria-label="Favorite"><Icon name="star" size={14} filled /></span>
      {/if}
      <span class="label badge bottom">{label}</span>
    </span>
    <span class="text">
      <span class="title">{item.title}</span>
      <span class="meta">{item.game && !item.title.includes(item.game) ? `${item.game} · ` : ""}{time(item.created)}</span>
    </span>
  </button>

  {#if !selecting}
    <button type="button" class="more" bind:this={moreBtn}
      aria-label="More actions"
      use:tip={"More"}
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
  {#if !selecting}
    <div class="menu" role="menu" popover="auto" bind:this={menuEl}>
      <button type="button" role="menuitem" onclick={() => act("copy")}>
        <Icon name="copy" size={16} />Copy
      </button>
      <button type="button" role="menuitem" onclick={() => act("link")}>
        <Icon name="link" size={16} />Get a link <span class="hint">72 h</span>
      </button>
      <span class="sep" role="separator"></span>
      <button type="button" role="menuitem" onclick={() => act("favorite")}>
        <Icon name="star" size={16} />{item.favorite ? "Remove favorite" : "Add to favorites"}
      </button>
      <button type="button" role="menuitem" onclick={() => act("reveal")}><Icon name="folder" size={16} />Show in folder</button>
      <button type="button" role="menuitem" class="danger" onclick={() => act("delete")}>
        <Icon name="trash" size={16} />Move to recycle bin
      </button>
    </div>
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
    gap: 8px;
    padding: 0;
    border: 0;
    background: transparent;
    text-align: left;
    border-radius: 14px;
  }
  .thumb {
    position: relative;
    display: block;
    aspect-ratio: 16 / 9;
    border-radius: 14px;
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
    font-size: 12px;
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
    padding: 2px 7px;
    border-radius: 6px;
    background: rgba(0, 0, 0, 0.65);
    color: #fff;
  }
  .label.top {
    top: 8px;
    left: 8px;
  }
  .label.accent {
    color: var(--accent);
  }
  .label.bottom {
    right: 8px;
    bottom: 8px;
    font-size: 11px;
  }
  .fav {
    position: absolute;
    top: 8px;
    right: 8px;
    color: var(--accent);
    display: flex;
    filter: drop-shadow(0 1px 2px rgba(0, 0, 0, 0.6));
  }
  .text {
    display: flex;
    flex-direction: column;
    padding: 0 4px;
    min-width: 0;
  }
  .title {
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .more {
    position: absolute;
    right: 2px;
    bottom: 2px;
    width: 32px;
    height: 32px;
    border: 0;
    border-radius: 10px;
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
    min-width: 210px;
    padding: 4px;
    border: 0;
    border-radius: 14px;
    background: var(--raised);
    color: var(--ink);
    box-shadow:
      0 0 0 1px rgb(255 255 255 / 0.04),
      0 14px 36px rgb(0 0 0 / 0.5);
  }
  .menu {
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
    height: 32px;
    padding: 0 10px;
    border: 0;
    border-radius: 8px;
    background: transparent;
    color: var(--ink);
    text-align: left;
  }
  .menu button:hover {
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
