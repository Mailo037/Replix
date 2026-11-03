<script lang="ts">
  import { onMount } from "svelte";
  import { raster } from "./lib/ascii";
  import { store } from "./lib/store.svelte";
  import Editor from "./components/Editor.svelte";
  import Library from "./components/Library.svelte";
  import Onboarding from "./components/Onboarding.svelte";
  import Settings from "./components/Settings.svelte";
  import Sidebar from "./components/Sidebar.svelte";
  import TitleBar from "./components/TitleBar.svelte";

  const field = raster();
  let error = $state<string | null>(null);

  onMount(() => {
    store.init().catch((e) => (error = String(e)));
  });

  /** Each page (and each library filter) fades in when you switch to it. */
  const page = $derived(JSON.stringify(store.view));
</script>

<div class="shell">
  <pre class="raster" aria-hidden="true">{field}</pre>

  <TitleBar />

  <div class="body">
    {#if error}
      <p class="meta">Replix could not start: {error}</p>
    {:else if store.settings && !store.settings.onboarded}
      <Onboarding />
    {:else}
      {#if store.view.name !== "editor"}
        <Sidebar />
      {/if}
      <main>
        {#key page}
          <div class="page">
            {#if store.view.name === "library"}
              <Library filter={store.view.filter} game={store.view.game} />
            {:else if store.view.name === "editor"}
              <Editor id={store.view.id} />
            {:else}
              <Settings />
            {/if}
          </div>
        {/key}
      </main>
    {/if}
  </div>

  {#if store.notice}
    <div class="notice" role="status">
      <span class="notice-text"><strong>{store.notice.title}</strong><span>{store.notice.detail}</span></span>
      {#each store.notice.actions as action (action.label)}
        <button
          type="button"
          class="notice-action"
          onclick={() => {
            action.run();
            store.notice = null;
          }}>{action.label}</button
        >
      {/each}
    </div>
  {/if}
</div>

<style>
  .shell {
    position: relative;
    height: 100%;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  .raster {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    height: 440px;
    pointer-events: none;
  }
  .raster {
    margin: 0;
    overflow: hidden;
    font-family: var(--mono);
    font-size: 11px;
    line-height: 16px;
    color: var(--ghost);
    opacity: 0.22;
    mask-image: linear-gradient(to bottom, #000, transparent);
    animation: drift 24s linear infinite;
  }
  @keyframes drift {
    from {
      transform: translateY(0);
    }
    to {
      transform: translateY(-16px);
    }
  }
  .body {
    position: relative;
    flex: 1;
    min-height: 0;
    display: flex;
    gap: 24px;
    padding: 4px 12px 12px;
  }
  main {
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: flex;
    padding-top: 8px;
  }
  .page {
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: flex;
    animation: page 220ms var(--travel);
  }
  @keyframes page {
    from {
      opacity: 0;
      transform: translateY(4px);
    }
  }
  .notice {
    position: absolute;
    left: 50%;
    bottom: 20px;
    z-index: 60;
    transform: translateX(-50%);
    display: flex;
    align-items: center;
    gap: 16px;
    max-width: min(560px, calc(100% - 48px));
    padding: 12px 12px 12px 18px;
    border-radius: 22px;
    background: var(--raised);
    animation: up 240ms var(--travel);
  }
  @keyframes up {
    from {
      opacity: 0;
      transform: translate(-50%, 8px);
    }
  }
  .notice-text {
    display: flex;
    flex-direction: column;
    min-width: 0;
    font-size: 13px;
  }
  .notice-text span {
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .notice-action {
    height: 40px;
    padding: 0 12px;
    border: 0;
    border-radius: 999px;
    background: transparent;
    color: var(--accent);
    font-weight: 600;
    white-space: nowrap;
  }
  .notice-action:hover {
    background: var(--surface);
  }
</style>
