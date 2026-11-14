<script lang="ts">
  import { onMount } from "svelte";
  import { raster } from "./lib/ascii";
  import { store } from "./lib/store.svelte";
  import Editor from "./components/Editor.svelte";
  import FfmpegSetup from "./components/FfmpegSetup.svelte";
  import Library from "./components/Library.svelte";
  import LiveSession from "./components/LiveSession.svelte";
  import Onboarding from "./components/Onboarding.svelte";
  import Settings from "./components/Settings.svelte";
  import Sidebar from "./components/Sidebar.svelte";
  import TitleBar from "./components/TitleBar.svelte";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { importPaths } from "./lib/importing";
  import { t } from "./lib/i18n.svelte";

  /** Lines in the field, each 16 px high: the field is taller than the space it fills. */
  const ROWS = 34;
  // The field twice over: it drifts up by one field and starts again, with no seam.
  const field = raster(260, ROWS).repeat(2);
  let error = $state<string | null>(null);

  /** Files from Explorer are being dragged over the window. */
  let dropping = $state(false);

  onMount(() => {
    store.init().catch((e) => (error = String(e)));
    // Videos and pictures dropped on the window go into the library.
    const unlisten = getCurrentWebview().onDragDropEvent((e) => {
      if (e.payload.type === "enter" || e.payload.type === "over") dropping = !focused() && !!store.settings?.onboarded;
      else if (e.payload.type === "leave") dropping = false;
      else if (e.payload.type === "drop") {
        const wanted = dropping;
        dropping = false;
        if (wanted) importPaths(e.payload.paths);
      }
    });
    return () => {
      unlisten.then((f) => f());
    };
  });

  /** A screen of its own for one thing, the editor or a session being recorded: no sidebar, no dropping. */
  const focused = () => store.view.name === "editor" || store.view.name === "live";

  /** Each page (and each library filter) fades in when you switch to it. */
  const page = $derived(JSON.stringify(store.view));
</script>

<div class="shell">
  <!-- Moved by the compositor, smoothly: the drift costs the page no layout and no repaint. -->
  <div class="raster" aria-hidden="true"><pre style:--field="{ROWS * 16}px" style:--drift="{ROWS * 16 * 1.5}s">{field}</pre></div>

  <TitleBar />

  <div class="body">
    {#if error}
      <p class="meta">{t("Replix could not start: {error}", { error })}</p>
    {:else if store.settings && !store.settings.onboarded}
      <Onboarding />
    {:else}
      {#if !focused()}
        <Sidebar />
      {/if}
      <main>
        {#key page}
          <div class="page">
            {#if store.view.name === "library"}
              <Library filter={store.view.filter} game={store.view.game} tag={store.view.tag ?? null} />
            {:else if store.view.name === "editor"}
              <Editor id={store.view.id} />
            {:else if store.view.name === "live"}
              <LiveSession />
            {:else}
              <Settings />
            {/if}
          </div>
        {/key}
      </main>
    {/if}
  </div>

  <FfmpegSetup />

  {#if dropping}
    <div class="drop" aria-hidden="true"><span>{t("Drop to add to your library")}</span></div>
  {/if}

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
  .drop {
    position: absolute;
    inset: 12px;
    z-index: 60;
    display: grid;
    place-items: center;
    border: 2px dashed var(--accent);
    border-radius: 28px;
    background: rgb(10 10 9 / 0.6);
    pointer-events: none;
    font-size: 18px;
    font-weight: 800;
  }
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
    overflow: hidden;
    opacity: 0.22;
    mask-image: linear-gradient(to bottom, #000, transparent);
  }
  .raster pre {
    margin: 0;
    font-family: var(--mono);
    font-size: 11px;
    line-height: 16px;
    color: var(--ghost);
    will-change: transform;
    /* Up by one field in a steady glide, a pixel every second and a half, then from the
       start again, which looks the same. Reduced motion keeps it still. */
    animation: drift var(--drift) linear infinite;
  }
  @keyframes drift {
    to {
      transform: translateY(calc(-1 * var(--field)));
    }
  }
  /* Without the focus, perhaps beside a game, it stands still and the page draws nothing. */
  :global(.resting) .raster pre {
    animation-play-state: paused;
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
