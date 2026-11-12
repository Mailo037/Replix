<script lang="ts">
  import { follow, wheel } from "../../lib/editor/actions";
  import { frameBox, placed, resized, zoomed, type Corner } from "../../lib/editor/crop";
  import type { EditorSession } from "../../lib/editor/session.svelte";
  import { t } from "../../lib/i18n.svelte";

  /** The 9:16 frame over the preview: drag it to move, pull a corner or scroll to resize. */
  let { ed, preview }: { ed: EditorSession; preview: HTMLElement } = $props();

  const box = $derived(frameBox(ed.cropShare, ed.crop));

  function move(e: PointerEvent) {
    const rect = preview.getBoundingClientRect();
    let last = { x: e.clientX, y: e.clientY };
    follow(e, (ev) => {
      const now = frameBox(ed.cropShare, ed.crop);
      ed.crop = placed(ed.cropShare, now.left + (ev.clientX - last.x) / rect.width, now.top + (ev.clientY - last.y) / rect.height, ed.crop.scale);
      last = { x: ev.clientX, y: ev.clientY };
    });
  }

  function resize(corner: Corner, e: PointerEvent) {
    const rect = preview.getBoundingClientRect();
    const start = { ...ed.crop };
    follow(e, (ev) => (ed.crop = resized(ed.cropShare, start, corner, (ev.clientX - rect.left) / rect.width, (ev.clientY - rect.top) / rect.height)), { stop: true });
  }

  function key(e: KeyboardEvent) {
    const d = 0.05;
    if (e.key === "ArrowLeft" || e.key === "ArrowRight") ed.crop = placed(ed.cropShare, box.left + (e.key === "ArrowLeft" ? -d : d) * (1 - box.width), box.top, ed.crop.scale);
    else if (e.key === "ArrowUp" || e.key === "ArrowDown") ed.crop = placed(ed.cropShare, box.left, box.top + (e.key === "ArrowUp" ? -d : d) * (1 - box.height), ed.crop.scale);
    else if (e.key === "+" || e.key === "-") ed.crop = zoomed(ed.cropShare, ed.crop, ed.crop.scale + (e.key === "+" ? d : -d));
    else return;
    e.preventDefault();
    e.stopPropagation();
  }
</script>

<button
  type="button"
  class="crop"
  style:left="{box.left * 100}%"
  style:top="{box.top * 100}%"
  style:width="{box.width * 100}%"
  style:height="{box.height * 100}%"
  aria-label={t("9:16 frame. Drag or use the arrow keys to move it; drag a corner, scroll or press + and - to resize it")}
  use:wheel={(e) => {
    // Up makes the frame smaller: it zooms in.
    if (e.deltaY) ed.crop = zoomed(ed.cropShare, ed.crop, ed.crop.scale * (e.deltaY < 0 ? 0.95 : 1 / 0.95));
  }}
  onpointerdown={move}
  onkeydown={key}
>
  {#each ["nw", "ne", "sw", "se"] as const as c (c)}
    <span class="corner {c}" role="presentation" onpointerdown={(e) => resize(c, e)}></span>
  {/each}
  <span class="crop-label mono">9:16</span>
</button>

<style>
  .crop {
    position: absolute;
    box-shadow: 0 0 0 9999px rgb(10 10 9 / 0.8);
    padding: 0;
    border: 2px solid var(--accent);
    border-radius: 10px;
    background: transparent;
    cursor: grab;
    touch-action: none;
  }
  .crop:active {
    cursor: grabbing;
  }
  .corner {
    position: absolute;
    width: 14px;
    height: 14px;
    border-radius: 4px;
    background: var(--accent);
    touch-action: none;
  }
  .corner.nw {
    top: -7px;
    left: -7px;
    cursor: nwse-resize;
  }
  .corner.se {
    bottom: -7px;
    right: -7px;
    cursor: nwse-resize;
  }
  .corner.ne {
    top: -7px;
    right: -7px;
    cursor: nesw-resize;
  }
  .corner.sw {
    bottom: -7px;
    left: -7px;
    cursor: nesw-resize;
  }
  .crop-label {
    position: absolute;
    top: 8px;
    left: 50%;
    transform: translateX(-50%);
    padding: 2px 6px;
    border-radius: 8px;
    background: var(--accent);
    color: var(--on-accent);
    font-size: 11px;
  }
</style>
