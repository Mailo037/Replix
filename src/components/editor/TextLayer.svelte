<script lang="ts">
  import { follow } from "../../lib/editor/actions";
  import { frameBox } from "../../lib/editor/crop";
  import type { EditorSession } from "../../lib/editor/session.svelte";
  import { t } from "../../lib/i18n.svelte";

  /**
   * The texts over the preview, as the export will show them: placed in the frame (the 9:16
   * crop in that format, else the whole picture). Drag one to move it.
   */
  let { ed }: { ed: EditorSession } = $props();

  let layer = $state<HTMLDivElement>();
  const box = $derived(ed.vertical ? frameBox(ed.cropShare, ed.crop) : { left: 0, top: 0, width: 1, height: 1 });
  const shown = (i: number) => {
    const text = ed.texts[i];
    return ed.selectedText === i || (ed.current >= text.start && ed.current < text.end);
  };

  function move(i: number, e: PointerEvent) {
    if (!layer) return;
    ed.selectedText = i;
    const rect = layer.getBoundingClientRect();
    const text = ed.texts[i];
    const from = { x: text.x, y: text.y, cx: e.clientX, cy: e.clientY };
    const clamp = (v: number) => Math.min(1, Math.max(0, v));
    follow(
      e,
      (ev) => {
        const t = ed.texts[i];
        if (!t) return;
        t.x = clamp(from.x + (ev.clientX - from.cx) / rect.width);
        t.y = clamp(from.y + (ev.clientY - from.cy) / rect.height);
      },
      { stop: true },
    );
  }
</script>

<div
  class="text-layer"
  bind:this={layer}
  style:left="{box.left * 100}%"
  style:top="{box.top * 100}%"
  style:width="{box.width * 100}%"
  style:height="{box.height * 100}%"
>
  {#each ed.texts as text, i (i)}
    {#if shown(i) && text.png}
      <img
        class="text"
        class:selected={ed.selectedText === i}
        src={text.png}
        alt={text.text}
        title={t("Drag to move")}
        draggable="false"
        style:left="{text.x * 100}%"
        style:top="{text.y * 100}%"
        style:height="{text.height * 100}%"
        onpointerdown={(e) => move(i, e)}
      />
    {/if}
  {/each}
</div>

<style>
  .text-layer {
    position: absolute;
    z-index: 3;
    pointer-events: none;
  }
  .text {
    position: absolute;
    transform: translate(-50%, -50%);
    max-width: none;
    pointer-events: auto;
    cursor: move;
    touch-action: none;
    user-select: none;
  }
  .text.selected {
    outline: 2px dashed var(--pink);
    outline-offset: 2px;
    border-radius: 6px;
  }
</style>
