<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount } from "svelte";
  import type { ToastPosition } from "../lib/api";
  import Mark from "./Mark.svelte";

  // How long a note stays up; hovering it stops the clock.
  const LIFETIME = 4000;

  // Lives in its own transparent window above the game.
  let title = $state("");
  let detail = $state("");
  let position = $state<ToastPosition>("topRight");
  let accent = $state("#c5f26b");
  let scale = $state(1);
  let key = $state(0);

  const bottom = $derived(position === "bottomLeft" || position === "bottomRight");

  // A click, or the outline running out, puts it away.
  function dismiss() {
    title = "";
    getCurrentWindow().hide();
  }

  onMount(() => {
    const unlisten = listen<{ title: string; detail: string; position: ToastPosition; accent: string; scale: number }>("toast", (e) => {
      scale = e.payload.scale ?? 1;
      title = e.payload.title;
      detail = e.payload.detail;
      position = e.payload.position;
      accent = e.payload.accent;
      key += 1;
    });
    return () => {
      unlisten.then((f) => f());
    };
  });
</script>

<div class="stage" class:bottom style:zoom={scale} style:--zoom={scale}>
  {#key key}
    {#if title}
      <button
        type="button"
        class="toast"
        aria-label="{title}. Click to close"
        style:--life="{LIFETIME}ms"
        style:--ring={accent}
        onclick={dismiss}
      >
        <span class="tile"><Mark size={15} /></span>
        <span class="text">
          <span class="title">{title}</span>
          <span class="detail">{detail}</span>
        </span>
        <span class="close" aria-hidden="true">
          <svg class="ring" viewBox="0 0 26 26">
            <circle cx="13" cy="13" r="12" pathLength="100" onanimationend={dismiss} />
          </svg>
          <svg width="10" height="10" viewBox="0 0 10 10"><path d="M1 1l8 8M9 1L1 9" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" /></svg>
        </span>
      </button>
    {/if}
  {/key}
</div>

<style>
  .stage {
    height: calc(100vh / var(--zoom, 1));
    display: flex;
    flex-direction: column;
  }
  .stage.bottom {
    justify-content: flex-end;
  }
  .toast {
    --from: -8px;
    margin: 6px;
    display: flex;
    align-items: center;
    gap: 12px;
    width: calc(100% - 12px);
    padding: 10px 14px 10px 10px;
    border: 0;
    border-radius: 24px;
    background: #33284a;
    color: #f3eefc;
    font-family: var(--sans);
    font-size: 13px;
    text-align: left;
    cursor: pointer;
    animation: in 280ms cubic-bezier(0.22, 0.61, 0.36, 1);
  }
  .bottom .toast {
    --from: 8px;
  }
  @keyframes in {
    from {
      opacity: 0;
      transform: translateY(var(--from)) scale(0.98);
    }
  }
  /* The time left, drawn as a ring that runs down around the close button. */
  .ring {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    transform: rotate(-90deg);
    pointer-events: none;
  }
  .ring circle {
    fill: none;
    stroke: var(--ring);
    stroke-width: 2;
    stroke-linecap: round;
    stroke-dasharray: 100 100;
    animation: run var(--life) linear forwards;
  }
  .toast:hover .ring circle {
    animation-play-state: paused;
  }
  @keyframes run {
    to {
      stroke-dashoffset: 100;
    }
  }
  .tile {
    width: 40px;
    height: 40px;
    flex: none;
    border-radius: 16px;
    background: #0e0a14;
    display: grid;
    place-items: center;
  }
  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .title {
    font-weight: 600;
  }
  .close {
    position: relative;
    width: 26px;
    height: 26px;
    flex: none;
    display: grid;
    place-items: center;
    border-radius: 50%;
    color: #a397b8;
    transition: color 140ms ease;
  }
  .toast:hover .close {
    color: #f3eefc;
  }
  .detail {
    font-size: 12px;
    color: #c4bad6;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
