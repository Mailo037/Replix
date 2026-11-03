<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import Mark from "./Mark.svelte";

  // Lives in its own transparent window above the game.
  let title = $state("");
  let detail = $state("");
  let key = $state(0);

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
    <div class="toast">
      <span class="tile"><Mark size={15} /></span>
      <span class="text">
        <span class="title">{title}</span>
        <span class="detail">{detail}</span>
      </span>
    </div>
  {/if}
{/key}

<style>
  .toast {
    margin: 6px;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 16px 10px 10px;
    border-radius: 18px;
    background: #2d2d2d;
    color: #eee;
    font-family: var(--sans);
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
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .title {
    font-weight: 600;
  }
  .detail {
    font-size: 12px;
    color: #aaa;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
