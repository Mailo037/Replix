<script lang="ts">
  import { fileUrl, type PendingSave } from "../lib/api";
  import { duration, gameColor, time } from "../lib/format";
  import { store } from "../lib/store.svelte";

  let { save }: { save: PendingSave } = $props();

  let loaded = $state(false);
  const previewUrl = $derived(save.preview ? fileUrl(save.preview) : null);
</script>

<div class="tile" role="group" aria-label={`${save.title}, saving`} aria-busy="true">
  <span class="thumb">
    <!-- Hidden in the tray, the pictures are let go so WebView2 can free them. -->
    {#if previewUrl && store.visible}
      <img src={previewUrl} alt="" class="real" class:loaded decoding="async" onload={() => (loaded = true)} />
    {/if}
    <span class="shade pending-loop" aria-hidden="true"></span>
    <span class="label top saving"><span class="dot pending-loop" aria-hidden="true"></span>Saving…</span>
    <span class="label bottom">{duration(save.duration)}</span>
  </span>
  <span class="text">
    <span class="title">{save.title}</span>
    <span class="meta">
      {#if save.game}<span class="swatch" style:background={gameColor(save.game)} aria-hidden="true"></span>{/if}
      {save.game && !save.title.includes(save.game) ? `${save.game} · ` : ""}{time(save.created)}
    </span>
  </span>
</div>

<style>
  .tile {
    display: flex;
    flex-direction: column;
    gap: 10px;
    cursor: progress;
  }
  .thumb {
    position: relative;
    display: block;
    aspect-ratio: 16 / 9;
    border-radius: var(--r-thumb);
    overflow: hidden;
    background: var(--black);
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
  /* Dimmed and slowly sweeping until the clip is in the library. */
  .shade {
    position: absolute;
    inset: 0;
    background:
      linear-gradient(100deg, transparent 30%, color-mix(in srgb, #fff 10%, transparent) 50%, transparent 70%) 0 0 / 250% 100%,
      color-mix(in srgb, var(--canvas) 35%, transparent);
    /* Looping: stands still while Replix is in the background (see app.css). */
    animation: sweep 1.6s linear infinite;
  }
  @keyframes sweep {
    from {
      background-position: 100% 0, 0 0;
    }
    to {
      background-position: -150% 0, 0 0;
    }
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
  .label.bottom {
    right: 10px;
    bottom: 10px;
  }
  .saving {
    display: flex;
    align-items: center;
    gap: 7px;
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--accent);
    animation: pulse 1.2s ease-in-out infinite;
  }
  @keyframes pulse {
    50% {
      opacity: 0.35;
    }
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
    color: var(--muted);
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
  @media (prefers-reduced-motion: reduce) {
    .shade,
    .dot {
      animation: none;
    }
  }
</style>
