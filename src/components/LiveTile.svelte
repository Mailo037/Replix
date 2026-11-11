<script lang="ts">
  import { api, fileUrl } from "../lib/api";
  import { duration, gameColor, time } from "../lib/format";
  import { store } from "../lib/store.svelte";
  import { t } from "../lib/i18n.svelte";

  /** The session being recorded right now; it becomes a clip tile once it is stopped and saved. */
  let { since, game, title }: { since: number; game: string; title: string } = $props();

  let loaded = $state(false);
  let now = $state(Date.now());
  /** Asked to stop; the tile goes once Replix says the session is over. */
  let stopping = $state(false);

  $effect(() => {
    // Read off the start time, so it is right again when the window returns.
    if (!store.visible) return;
    now = Date.now();
    const timer = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(timer);
  });

  const picture = $derived(store.sessionPicture?.since === since ? fileUrl(store.sessionPicture.path) : null);
  const elapsed = $derived(Math.max(0, (now - since) / 1000));
  /** The dot blinks once a second while Replix is in front; a step, not a running animation. */
  const dim = $derived(store.focused && Math.floor(elapsed) % 2 === 1);

  const gameLabel = (name: string) => (name === "Desktop" ? t("Desktop") : name);

  function stop() {
    stopping = true;
    api.toggleSession().catch(() => (stopping = false));
  }
</script>

<div class="tile" role="group" aria-label={t("{title}, recording", { title })}>
  <span class="sticker live">{t("rec")}</span>
  <span class="thumb">
    <!-- Hidden in the tray, the picture is let go so WebView2 can free it. -->
    {#if picture && store.visible}
      <img src={picture} alt="" class="real" class:loaded decoding="async" onload={() => (loaded = true)} />
    {/if}
    <span class="label top"><span class="dot" class:dim aria-hidden="true"></span>{t("Recording")}</span>
    <button type="button" class="label stop" disabled={stopping} onclick={stop}>
      <span class="square" aria-hidden="true"></span>{t("Stop")}
    </button>
    <span class="label badge bottom" role="timer">{duration(elapsed)}</span>
  </span>
  <span class="text">
    <span class="title">{title}</span>
    <span class="meta">
      {#if game}<span class="swatch" style:background={gameColor(game)} aria-hidden="true"></span>{/if}
      {game && !title.includes(game) ? `${gameLabel(game)} · ` : ""}{time(since)}
    </span>
  </span>
</div>

<style>
  .tile {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .sticker {
    position: absolute;
    top: -7px;
    left: -7px;
    z-index: 2;
  }
  .sticker.live {
    background: var(--red);
    color: var(--on-sticker);
  }
  .thumb {
    position: relative;
    display: block;
    aspect-ratio: 16 / 9;
    border-radius: var(--r-thumb);
    overflow: hidden;
    background: var(--black);
    outline: 2px solid var(--red);
    outline-offset: 3px;
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
    display: flex;
    align-items: center;
    gap: 7px;
    padding: 3px 9px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--canvas) 82%, transparent);
    color: var(--ink);
  }
  .label.top {
    right: 10px;
    top: 10px;
  }
  .label.bottom {
    right: 10px;
    bottom: 10px;
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--red);
    transition: opacity 300ms ease;
  }
  .dot.dim {
    opacity: 0.35;
  }
  .stop {
    left: 10px;
    bottom: 8px;
    height: 28px;
    padding: 0 12px;
    border: 0;
    font-weight: 700;
    transition: background-color 160ms ease;
  }
  .stop:hover:not(:disabled) {
    background: var(--red);
    color: var(--on-sticker);
  }
  .stop:disabled {
    color: var(--muted);
    cursor: progress;
  }
  .square {
    width: 9px;
    height: 9px;
    border-radius: 2px;
    background: currentColor;
  }
  .text {
    display: flex;
    flex-direction: column;
    padding: 0 4px;
    min-width: 0;
  }
  .title {
    font-size: 15px;
    font-weight: 700;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .meta {
    display: flex;
    align-items: center;
    gap: 7px;
    margin-top: 2px;
    font-size: 12.5px;
  }
  .swatch {
    width: 8px;
    height: 8px;
    flex: none;
    border-radius: 3px;
  }
</style>
