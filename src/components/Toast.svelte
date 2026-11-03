<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount } from "svelte";
  import Mark from "./Mark.svelte";

  // Lives in its own transparent window above the game.
  let title = $state("");
  let detail = $state("");
  let key = $state(0);

  // A click puts it away at once.
  function dismiss() {
    title = "";
    getCurrentWindow().hide();
  }

  onMount(() => {
    const unlisten = listen<{ title: string; detail: string }>("toast", (e) => {
      title = e.payload.title;
      detail = e.payload.detail;
      key += 1;
    });
    return () => {
      unlisten.then((f) => f());
    };
  });
</script>

{#key key}
  {#if title}
    <button type="button" class="toast" aria-label="{title}. Click to close" onclick={dismiss}>
      <span class="tile"><Mark size={15} /></span>
      <span class="text">
        <span class="title">{title}</span>
        <span class="detail">{detail}</span>
      </span>
      <span class="close" aria-hidden="true">
        <svg width="10" height="10" viewBox="0 0 10 10"><path d="M1 1l8 8M9 1L1 9" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" /></svg>
      </span>
    </button>
  {/if}
{/key}

<style>
  .toast {
    margin: 6px;
    display: flex;
    align-items: center;
    gap: 12px;
    width: calc(100% - 12px);
    padding: 10px 16px 10px 10px;
    border: 0;
    border-radius: 18px;
    background: #2d2d2d;
    color: #eee;
    font-family: var(--sans);
    font-size: 13px;
    text-align: left;
    cursor: pointer;
    animation: in 280ms cubic-bezier(0.22, 0.61, 0.36, 1);
  }
  @keyframes in {
    from {
      opacity: 0;
      transform: translateY(-8px) scale(0.98);
    }
  }
  .tile {
    width: 40px;
    height: 40px;
    flex: none;
    border-radius: 12px;
    background: #0a0a09;
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
    width: 22px;
    height: 22px;
    flex: none;
    display: grid;
    place-items: center;
    border-radius: 6px;
    color: #8a8a8a;
    opacity: 0;
    transition: opacity 140ms ease;
  }
  .toast:hover .close {
    opacity: 1;
  }
  .detail {
    font-size: 12px;
    color: #aaa;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
