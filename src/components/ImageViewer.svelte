<script lang="ts">
  import { api, fileUrl, type Item } from "../lib/api";
  import { share } from "../lib/share";
  import { tip } from "../lib/tooltip";
  import Icon from "./Icon.svelte";
  import { store } from "../lib/store.svelte";
  import { t } from "../lib/i18n.svelte";

  // Screenshots full size: wheel or double-click to zoom, drag to look around, arrows for the next one.
  let { items, index = $bindable(), onclose }: { items: Item[]; index: number; onclose: () => void } = $props();

  const MAX = 8;
  let stage = $state<HTMLDivElement>();
  let img = $state<HTMLImageElement>();
  /** 1 is the picture fitted to the window. */
  let scale = $state(1);
  let x = $state(0);
  let y = $state(0);
  let dragging = $state(false);
  /** Fitted width compared to the real width, so the zoom can be shown in real pixels. */
  let fitRatio = $state(1);

  const item = $derived(items[index]);
  const percent = $derived(Math.round(scale * fitRatio * 100));

  $effect(() => {
    void item;
    reset();
  });

  function measure() {
    if (img && img.naturalWidth) fitRatio = img.clientWidth / img.naturalWidth;
  }

  function reset() {
    scale = 1;
    x = 0;
    y = 0;
  }

  /** Keeps the picture from being pushed out of sight. */
  function clamp() {
    if (!img || !stage) return;
    const maxX = Math.max(0, (img.clientWidth * scale - stage.clientWidth) / 2);
    const maxY = Math.max(0, (img.clientHeight * scale - stage.clientHeight) / 2);
    x = Math.min(maxX, Math.max(-maxX, x));
    y = Math.min(maxY, Math.max(-maxY, y));
  }

  /** Zooms so the point under (cx, cy) stays where it is. */
  function zoomTo(next: number, cx?: number, cy?: number) {
    if (!stage) return;
    next = Math.min(MAX, Math.max(1, next));
    const r = stage.getBoundingClientRect();
    const px = (cx ?? r.left + r.width / 2) - (r.left + r.width / 2);
    const py = (cy ?? r.top + r.height / 2) - (r.top + r.height / 2);
    x = px - (px - x) * (next / scale);
    y = py - (py - y) * (next / scale);
    scale = next;
    if (scale === 1) reset();
    else clamp();
  }

  function onWheel(e: WheelEvent) {
    e.preventDefault();
    zoomTo(scale * Math.exp(-e.deltaY * 0.0015), e.clientX, e.clientY);
  }

  function onDouble(e: MouseEvent) {
    if (scale > 1) zoomTo(1);
    else zoomTo(fitRatio < 0.8 ? 1 / fitRatio : 2.5, e.clientX, e.clientY);
  }

  let moved = false;
  function onDown(e: PointerEvent) {
    if (e.button !== 0) return;
    moved = false;
    if (scale === 1) return;
    const target = e.currentTarget as HTMLElement;
    target.setPointerCapture(e.pointerId);
    dragging = true;
    let lastX = e.clientX;
    let lastY = e.clientY;
    const move = (ev: PointerEvent) => {
      x += ev.clientX - lastX;
      y += ev.clientY - lastY;
      if (Math.abs(ev.clientX - e.clientX) + Math.abs(ev.clientY - e.clientY) > 3) moved = true;
      lastX = ev.clientX;
      lastY = ev.clientY;
      clamp();
    };
    const up = () => {
      dragging = false;
      target.removeEventListener("pointermove", move);
      target.removeEventListener("pointerup", up);
    };
    target.addEventListener("pointermove", move);
    target.addEventListener("pointerup", up);
  }

  // A click on the dark area around the picture closes the viewer.
  function onStageClick(e: MouseEvent) {
    if (!moved && scale === 1 && e.target === stage) onclose();
  }

  function go(step: number) {
    const next = index + step;
    if (next >= 0 && next < items.length) index = next;
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") onclose();
    else if (e.key === "ArrowLeft") go(-1);
    else if (e.key === "ArrowRight") go(1);
    else if (e.key === "+" || e.key === "=") zoomTo(scale * 1.25);
    else if (e.key === "-") zoomTo(scale / 1.25);
    else if (e.key === "0") zoomTo(1);
    else return;
    e.preventDefault();
    e.stopPropagation();
  }
</script>

<svelte:window onkeydown={onKey} onresize={() => (measure(), clamp())} />

<div class="viewer" role="dialog" aria-label={item?.title ?? t("Screenshot")}>
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div
    bind:this={stage}
    class="stage"
    class:zoomed={scale > 1}
    class:dragging
    onwheel={onWheel}
    ondblclick={onDouble}
    onpointerdown={onDown}
    onclick={onStageClick}
  >
    {#if item}
      {#key item.id}
        <!-- Hidden in the tray, the picture is let go; it loads again on return. -->
        <img
          hidden={!store.visible}
          bind:this={img}
          src={store.visible ? fileUrl(item.path) : undefined}
          alt={item.title}
          draggable="false"
          onload={measure}
          style:transform="translate({x}px, {y}px) scale({scale})"
        />
      {/key}
    {/if}
    {#if index > 0}
      <button type="button" class="nav prev" aria-label={t("Previous")} use:tip={t("Previous · ←")} onclick={() => go(-1)}>
        <Icon name="back" />
      </button>
    {/if}
    {#if index < items.length - 1}
      <button type="button" class="nav next" aria-label={t("Next")} use:tip={t("Next · →")} onclick={() => go(1)}>
        <Icon name="back" />
      </button>
    {/if}
  </div>

  {#if item}
    <div class="bar">
      <span class="info">
        <span class="name">{item.title}</span>
        <span class="mono meta">{item.width}×{item.height}{items.length > 1 ? t(" · {n} of {total}", { n: index + 1, total: items.length }) : ""}</span>
      </span>
      <div class="zoom" role="group" aria-label={t("Zoom")}>
        <button type="button" aria-label={t("Zoom out")} use:tip={t("Zoom out · −")} disabled={scale <= 1} onclick={() => zoomTo(scale / 1.25)}>
          <span class="minus"></span>
        </button>
        <button type="button" class="pct mono" use:tip={scale > 1 ? t("Fit to window · 0") : t("Real size")} onclick={() => (scale > 1 ? zoomTo(1) : zoomTo(1 / fitRatio))}>
          {percent}%
        </button>
        <button type="button" aria-label={t("Zoom in")} use:tip={t("Zoom in · +")} disabled={scale >= MAX} onclick={() => zoomTo(scale * 1.25)}>
          <Icon name="plus" size={16} />
        </button>
      </div>
      <div class="actions">
        <button type="button" class="btn small" onclick={() => share.copy(item.path)}><Icon name="copy" size={16} />{t("Copy")}</button>
        <button type="button" class="btn small" onclick={() => api.revealItem(item.id)}><Icon name="folder" size={16} />{t("Show in folder")}</button>
        <button type="button" class="icon-btn" aria-label={t("Close")} use:tip={t("Close · Esc")} onclick={onclose}><Icon name="close" size={16} /></button>
      </div>
    </div>
  {/if}
</div>

<style>
  /* Starts below the title bar, so the window can still be dragged. */
  .viewer {
    position: fixed;
    inset: 34px 0 0;
    z-index: 50;
    display: flex;
    flex-direction: column;
    background: rgb(8 8 8 / 0.94);
    animation: fade 180ms var(--travel);
  }
  .stage {
    position: relative;
    flex: 1;
    min-height: 0;
    display: grid;
    place-items: center;
    padding: 24px 72px 8px;
    overflow: hidden;
    cursor: zoom-in;
  }
  .stage.zoomed {
    cursor: grab;
  }
  .stage.dragging {
    cursor: grabbing;
  }
  img {
    max-width: 100%;
    max-height: 100%;
    border-radius: 20px;
    transform-origin: center;
    transition: transform 160ms var(--travel);
    animation: pop 220ms var(--travel);
  }
  .dragging img {
    transition: none;
  }
  .nav {
    position: absolute;
    top: 50%;
    width: 44px;
    height: 44px;
    margin-top: -22px;
    display: grid;
    place-items: center;
    border: 0;
    border-radius: 50%;
    background: rgb(40 40 40 / 0.85);
    color: var(--ink);
  }
  .nav:hover {
    background: var(--hover);
  }
  .prev {
    left: 16px;
  }
  .next {
    right: 16px;
  }
  .next :global(svg) {
    transform: scaleX(-1);
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 10px 16px 14px;
  }
  .info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 500;
  }
  .zoom {
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 3px;
    border-radius: 999px;
    background: var(--overlay);
  }
  .zoom button {
    height: 30px;
    min-width: 30px;
    display: grid;
    place-items: center;
    border: 0;
    border-radius: 999px;
    background: transparent;
    color: var(--ink);
  }
  .zoom button:hover:not(:disabled) {
    background: var(--hover);
  }
  .zoom button:disabled {
    color: var(--ghost);
    cursor: default;
  }
  .zoom .pct {
    min-width: 58px;
    font-size: 12px;
  }
  .minus {
    width: 12px;
    height: 1.7px;
    border-radius: 1px;
    background: currentColor;
  }
  .actions {
    flex: 1;
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
  .actions .icon-btn {
    width: 36px;
    height: 36px;
    border-radius: 20px;
  }
  @keyframes fade {
    from {
      opacity: 0;
    }
  }
  @keyframes pop {
    from {
      opacity: 0;
      scale: 0.97;
    }
  }
</style>
