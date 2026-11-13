<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { fileUrl } from "../../lib/api";
  import type { EditorSession } from "../../lib/editor/session.svelte";
  import { t } from "../../lib/i18n.svelte";
  import { store } from "../../lib/store.svelte";
  import { tip } from "../../lib/tooltip";
  import Icon from "../Icon.svelte";
  import CropFrame from "./CropFrame.svelte";
  import TextLayer from "./TextLayer.svelte";

  /** The picture, as large as the room allows at the clip's own shape, with each track playing beside it. */
  let {
    ed,
    viewer = $bindable(),
    fullscreen = $bindable(false),
    onready,
  }: {
    ed: EditorSession;
    viewer?: HTMLDivElement;
    fullscreen?: boolean;
    /** The video knows its length; the editor puts the playhead where it belongs. */
    onready: () => void;
  } = $props();

  let preview = $state<HTMLDivElement>();

  /** The player fills the screen; Escape or the button brings it back. */
  export async function toggleFullscreen() {
    if (!viewer) return;
    if (document.fullscreenElement) await document.exitFullscreen().catch(() => {});
    else await viewer.requestFullscreen().catch(() => {});
  }

  $effect(() => {
    const sync = () => {
      fullscreen = document.fullscreenElement === viewer && !!viewer;
      // The page alone can only fill the window; the window goes full screen with it.
      getCurrentWindow().setFullscreen(fullscreen).catch(() => {});
    };
    document.addEventListener("fullscreenchange", sync);
    return () => {
      document.removeEventListener("fullscreenchange", sync);
      if (document.fullscreenElement) document.exitFullscreen().catch(() => {});
    };
  });
</script>

{#if ed.data}
  {@const data = ed.data}
  <div class="viewer" class:fullscreen bind:this={viewer}>
    <button
      type="button"
      class="fs-toggle"
      aria-label={fullscreen ? t("Leave full screen") : t("Full screen")}
      use:tip={fullscreen ? t("Leave full screen · Esc") : t("Full screen · F")}
      onclick={toggleFullscreen}
    >
      <Icon name={fullscreen ? "shrink" : "expand"} size={16} />
    </button>
    <div class="preview" bind:this={preview} style:--ratio={data.width && data.height ? data.width / data.height : 16 / 9}>
      <!-- svelte-ignore a11y_media_has_caption -->
      <video
        bind:this={ed.video}
        src={store.visible ? fileUrl(data.path) : undefined}
        muted
        preload="auto"
        onloadedmetadata={onready}
        onseeked={(e) => {
          if (!ed.playing) ed.current = e.currentTarget.currentTime;
        }}
        onclick={() => ed.toggle()}
      ></video>
      {#each data.tracks as track, i (i)}
        <audio bind:this={ed.audios[i]} src={store.visible ? fileUrl(track.audio) : undefined} preload="auto"></audio>
      {/each}
      {#if ed.vertical && preview}
        <CropFrame {ed} {preview} />
      {/if}
      {#if ed.texts.length}
        <TextLayer {ed} />
      {/if}
      {#if !ed.playing}
        <button type="button" class="play" aria-label={t("Play")} use:tip={t("Play · Space")} onclick={() => ed.play()}><Icon name="play" size={20} /></button>
      {/if}
    </div>
  </div>
{/if}

<style>
  .viewer {
    flex: 1;
    min-height: 120px;
    container-type: size;
    position: relative;
    display: grid;
    place-items: center;
  }
  .viewer.fullscreen {
    background: #000;
  }
  .viewer.fullscreen .preview {
    border-radius: 0;
  }
  .fs-toggle {
    position: absolute;
    top: 10px;
    right: 10px;
    z-index: 5;
    width: 34px;
    height: 34px;
    display: grid;
    place-items: center;
    border: 0;
    border-radius: 12px;
    background: rgb(0 0 0 / 0.55);
    color: #fff;
    opacity: 0;
    transition: opacity 160ms ease;
  }
  .viewer:hover .fs-toggle,
  .fs-toggle:focus-visible {
    opacity: 1;
  }
  .preview {
    position: relative;
    width: min(100cqw, 100cqh * var(--ratio));
    aspect-ratio: var(--ratio);
    border-radius: 20px;
    overflow: hidden;
    background: var(--black);
  }
  video {
    width: 100%;
    height: 100%;
    object-fit: contain;
    display: block;
    cursor: pointer;
  }
  .play {
    position: absolute;
    top: 50%;
    left: 50%;
    width: 44px;
    height: 44px;
    margin: -22px 0 0 -22px;
    padding-left: 2px;
    border: 0;
    border-radius: 50%;
    background: rgb(0 0 0 / 0.5);
    color: #fff;
    backdrop-filter: blur(8px);
    display: grid;
    place-items: center;
    transition: transform 160ms var(--travel), background-color 160ms ease;
  }
  .play:hover {
    background: rgb(0 0 0 / 0.7);
    transform: scale(1.06);
  }
</style>
