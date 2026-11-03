<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { store } from "../lib/store.svelte";
  import { tip } from "../lib/tooltip";
  import { updates } from "../lib/updates.svelte";
  import Icon from "./Icon.svelte";
  import Mark from "./Mark.svelte";
  import StatusMenu from "./StatusMenu.svelte";

  const win = getCurrentWindow();
  const inSettings = $derived(store.view.name === "settings");
</script>

<div class="titlebar" data-tauri-drag-region>
  <span class="mark" data-tauri-drag-region><Mark size={13} /></span>
  {#if store.settings?.onboarded}
    <div class="tools">
      <StatusMenu />
      <button
        type="button"
        class="gear"
        class:current={inSettings}
        aria-label={updates.phase === "available" ? "Settings, update ready" : "Settings"}
        use:tip={updates.phase === "available" ? "Settings · update ready" : "Settings"}
        aria-pressed={inSettings}
        onclick={() => store.go(inSettings ? { name: "library", filter: "all", game: null } : { name: "settings" })}
      >
        <Icon name="settings" size={16} />
        {#if updates.phase === "available"}<span class="badge-dot"></span>{/if}
      </button>
    </div>
  {/if}
  <div class="controls">
    <button type="button" aria-label="Minimize" use:tip={"Minimize"} onclick={() => win.minimize()}>
      <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true"><path d="M0 5h10" stroke="currentColor" /></svg>
    </button>
    <button type="button" aria-label="Maximize" use:tip={"Maximize"} onclick={() => win.toggleMaximize()}>
      <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true"><rect x="0.5" y="0.5" width="9" height="9" fill="none" stroke="currentColor" /></svg>
    </button>
    <button type="button" class="close" aria-label="Close" use:tip={store.settings?.closeToTray ? "Close · keeps recording in the tray" : "Close"} onclick={() => win.close()}>
      <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true"><path d="M0 0l10 10M10 0L0 10" stroke="currentColor" /></svg>
    </button>
  </div>
</div>

<style>
  .titlebar {
    position: relative;
    z-index: 20;
    display: flex;
    align-items: center;
    height: 34px;
    flex: none;
    padding-left: 14px;
  }
  .mark {
    display: flex;
  }
  .tools {
    margin-left: auto;
    margin-right: 8px;
    display: flex;
    align-items: center;
    gap: 2px;
  }
  .gear {
    position: relative;
    width: 30px;
    height: 26px;
    border: 0;
    border-radius: 13px;
    background: transparent;
    color: var(--muted);
    display: grid;
    place-items: center;
    transition: background-color 140ms ease, color 140ms ease;
  }
  .gear:hover {
    background: var(--surface);
    color: var(--ink);
  }
  .gear.current {
    color: var(--accent);
  }
  .badge-dot {
    position: absolute;
    top: 4px;
    right: 6px;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--accent);
  }
  .controls {
    margin-left: auto;
    display: flex;
  }
  .tools + .controls {
    margin-left: 0;
  }
  .controls button {
    width: 42px;
    height: 34px;
    border: 0;
    background: transparent;
    color: var(--muted);
    display: flex;
    align-items: center;
    justify-content: center;
    transition: background-color 120ms ease;
  }
  .controls button:hover {
    background: var(--hover);
    color: var(--ink);
  }
  .controls .close:hover {
    background: #c42b1c;
    color: #fff;
  }
</style>
