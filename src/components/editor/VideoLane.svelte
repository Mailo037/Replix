<script lang="ts">
  import { fileUrl } from "../../lib/api";
  import { follow, timeAt } from "../../lib/editor/actions";
  import type { EditorSession } from "../../lib/editor/session.svelte";
  import { position } from "../../lib/format";
  import { t } from "../../lib/i18n.svelte";
  import { store } from "../../lib/store.svelte";
  import { tip } from "../../lib/tooltip";
  import Icon from "../Icon.svelte";

  /** The picture's lane: the filmstrip with the start and end handles and the splits. */
  let { ed }: { ed: EditorSession } = $props();

  let lane = $state<HTMLDivElement>();

  function drag(kind: "start" | "end" | "head", e: PointerEvent) {
    if (!lane) return;
    const strip = lane;
    follow(
      e,
      (ev) => {
        const at = timeAt(strip, ev.clientX, ed.duration);
        if (kind === "start") {
          ed.setStart(at);
          ed.seek(ed.trimStart);
        } else if (kind === "end") {
          ed.setEnd(at);
          ed.seek(ed.trimEnd);
        } else ed.seek(at);
      },
      { now: true, stop: true },
    );
  }
</script>

{#if ed.data}
  <div class="row">
    <div class="head">
      <span class="kind"><Icon name="clips" size={16} /></span>
      <span class="name">{t("Video")}</span>
    </div>
    <div
      class="lane strip"
      bind:this={lane}
      role="slider"
      tabindex="0"
      aria-label={t("Position")}
      aria-valuemin={0}
      aria-valuemax={ed.duration}
      aria-valuenow={ed.current}
      onpointerdown={(e) => drag("head", e)}
      style:background-image={ed.data.filmstrip && store.visible ? `url("${fileUrl(ed.data.filmstrip)}")` : undefined}
    >
      <span class="range" style:left="{ed.pct(ed.trimStart)}%" style:width="{ed.pct(ed.trimEnd) - ed.pct(ed.trimStart)}%"></span>
      {#each ed.splits.filter((s) => s > ed.trimStart && s < ed.trimEnd) as s (s)}
        <span class="split-mark" style:left="{ed.pct(s)}%"></span>
        <button
          type="button"
          class="join"
          style:left="{ed.pct(s)}%"
          aria-label={t("Join the parts at {time}", { time: position(s) })}
          use:tip={t("Join the parts")}
          onpointerdown={(e) => e.stopPropagation()}
          onclick={() => ed.join(s)}
        >
          <Icon name="close" size={10} />
        </button>
      {/each}
      <button
        type="button"
        class="handle"
        style:left="{ed.pct(ed.trimStart)}%"
        aria-label={t("Start of the clip")}
        use:tip={t("Drag to set the start · I")}
        onpointerdown={(e) => drag("start", e)}
      ></button>
      <button
        type="button"
        class="handle end"
        style:left="{ed.pct(ed.trimEnd)}%"
        aria-label={t("End of the clip")}
        use:tip={t("Drag to set the end · O")}
        onpointerdown={(e) => drag("end", e)}
      ></button>
    </div>
  </div>
{/if}

<style>
  .strip {
    height: 46px;
    background-size: 100% 100%;
  }
  .range {
    position: absolute;
    top: 0;
    bottom: 0;
    border: 2px solid var(--accent);
    border-radius: 10px;
    pointer-events: none;
  }
  .handle {
    position: absolute;
    top: 0;
    bottom: 0;
    z-index: 2;
    width: 9px;
    padding: 0;
    border: 0;
    border-radius: 10px 0 0 10px;
    background: var(--accent);
    cursor: ew-resize;
    touch-action: none;
  }
  .handle.end {
    margin-left: -9px;
    border-radius: 0 10px 10px 0;
  }
  .handle::after {
    content: "";
    position: absolute;
    top: 50%;
    left: 3.5px;
    width: 2px;
    height: 12px;
    margin-top: -6px;
    border-radius: 1px;
    background: var(--on-accent);
    opacity: 0.55;
  }
  .split-mark {
    position: absolute;
    top: 2px;
    bottom: 2px;
    width: 2px;
    margin-left: -1px;
    background: var(--accent);
    pointer-events: none;
  }
  .join {
    position: absolute;
    top: 50%;
    z-index: 3;
    width: 18px;
    height: 18px;
    margin: -9px 0 0 -9px;
    padding: 0;
    display: grid;
    place-items: center;
    border: 0;
    border-radius: 50%;
    background: var(--accent);
    color: var(--on-accent);
    opacity: 0;
    transition: opacity 140ms ease;
  }
  .strip:hover .join,
  .join:focus-visible {
    opacity: 1;
  }
</style>
