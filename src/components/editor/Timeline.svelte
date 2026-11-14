<script lang="ts">
  import type { Snippet } from "svelte";
  import { follow, timeAt, wheel } from "../../lib/editor/actions";
  import type { EditorSession } from "../../lib/editor/session.svelte";
  import { t } from "../../lib/i18n.svelte";
  import { tip } from "../../lib/tooltip";
  import MarkerMenu from "./MarkerMenu.svelte";
  import Ruler from "./Ruler.svelte";
  import TimelineMenu from "./TimelineMenu.svelte";
  import TrackRow from "./TrackRow.svelte";
  import VideoLane from "./VideoLane.svelte";

  /** Audio tracks shown at once until the timeline is dragged taller or shorter; more scroll. */
  const MAX_ROWS = 3;
  /** An audio row's height with the gap below it. */
  const ROW = 45;
  /** How tall the audio tracks may grow before they scroll, as last dragged; kept for next time. */
  const ROWS_KEY = "replix.editor.rowsHeight";

  let {
    ed,
    playerRoom,
    toolbar,
    lanes,
  }: {
    ed: EditorSession;
    /** Height the player can still give up, in pixels. */
    playerRoom: () => number;
    toolbar: Snippet;
    /** More lanes under the picture's, such as texts. */
    lanes?: Snippet;
  } = $props();

  let rows = $state<HTMLDivElement>();
  let rowsHeight = $state(readRowsHeight());

  /** The marker being named, and where its panel opened. */
  let markerMenu = $state<{ at: number; x: number; y: number } | null>(null);
  /** The right-click menu: the time and the track or text it was opened on, and where. */
  let menu = $state<{ at: number | null; track: number | null; text: number | null; x: number; y: number } | null>(null);

  // A new clip, or undo bringing back other markers, closes the panel of one that is gone.
  $effect(() => {
    if (markerMenu && !ed.markers.some((m) => m.at === markerMenu!.at)) markerMenu = null;
  });

  function readRowsHeight(): number {
    try {
      const saved = Number(localStorage.getItem(ROWS_KEY));
      if (saved > 0) return saved;
    } catch {
      // Storage may be off; the default works as well.
    }
    return MAX_ROWS * ROW - 4;
  }

  /**
   * Sets how tall the audio tracks are: at least one row, at most all of them, and never more
   * than `room` taller than now, so the player keeps its smallest size.
   */
  function setRowsHeight(want: number, room: number) {
    if (!rows) return;
    const shown = rows.getBoundingClientRect().height;
    const most = Math.max(ROW - 4, Math.min(rows.scrollHeight, shown + room));
    rowsHeight = Math.round(Math.min(most, Math.max(ROW - 4, want)));
    try {
      localStorage.setItem(ROWS_KEY, String(rowsHeight));
    } catch {
      // Not kept; it still applies until the editor closes.
    }
  }

  /** Dragging the grip above the timeline makes the tracks taller (up) or shorter (down). */
  function resize(e: PointerEvent) {
    if (!rows) return;
    const startY = e.clientY;
    const startHeight = rows.getBoundingClientRect().height;
    // The player shrinks as the tracks grow, so its room is measured afresh each move.
    follow(e, (ev) => setRowsHeight(startHeight + (startY - ev.clientY), playerRoom()));
  }

  /**
   * Mouse wheel over the timeline moves the playhead: a fifth of a second per notch, one frame
   * with Shift. Over the track names on the left it scrolls the audio tracks instead.
   */
  function scrub(e: WheelEvent) {
    if (e.target instanceof Element && e.target.closest(".head")) {
      if (rows) rows.scrollTop += e.deltaMode === WheelEvent.DOM_DELTA_LINE ? e.deltaY * 40 : e.deltaY;
      return;
    }
    const delta = e.deltaY || e.deltaX;
    ed.seek(ed.current + (delta < 0 ? 1 : -1) * (e.shiftKey ? 1 / 30 : 0.2));
  }

  function openMarkerMenu(at: number, e: MouseEvent) {
    e.preventDefault();
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    markerMenu = { at, x: Math.max(8, Math.min(r.left - 8, innerWidth - 268)), y: Math.min(r.bottom + 6, innerHeight - 190) };
  }

  /**
   * Right-click anywhere on the timeline: the menu works at the time under the pointer, with
   * the entries of the audio track or text it was on. A marker opens its own panel instead.
   */
  function openMenu(e: MouseEvent) {
    if (e.defaultPrevented || !(e.target instanceof Element)) return;
    const lane = e.target.closest<HTMLElement>(".lane");
    const row = e.target.closest<HTMLElement>("[data-track]");
    if (!lane && !row) return;
    e.preventDefault();
    // From the keyboard (the menu key) there is no pointer: the playhead's time, by the lane.
    const keyboard = e.clientX === 0 && e.clientY === 0;
    const box = (lane ?? row)!.getBoundingClientRect();
    const at = !lane ? null : keyboard ? ed.current : timeAt(lane, e.clientX, ed.duration);
    const bar = e.target.closest<HTMLElement>("[data-text-bar]");
    menu = {
      at,
      track: row ? Number(row.dataset.track) : null,
      text: bar ? Number(bar.dataset.textBar) : null,
      x: keyboard ? box.left + box.width * (ed.pct(ed.current) / 100) : e.clientX,
      y: keyboard ? box.bottom + 4 : e.clientY,
    };
  }

  function restoreTracks() {
    for (const edit of ed.edits) delete edit.removed;
  }
</script>

{#if ed.data}
  {@const data = ed.data}
  <div class="timeline">
    {#if data.tracks.length}
      <button
        type="button"
        class="grip"
        aria-label={t("Timeline height")}
        use:tip={t("Drag to make the timeline taller or shorter · double-click to reset")}
        onpointerdown={resize}
        ondblclick={() => setRowsHeight(MAX_ROWS * ROW - 4, playerRoom())}
        onkeydown={(e) => {
          if (e.key !== "ArrowUp" && e.key !== "ArrowDown") return;
          e.preventDefault();
          e.stopPropagation();
          const now = rows?.getBoundingClientRect().height ?? rowsHeight;
          setRowsHeight(now + (e.key === "ArrowUp" ? ROW : -ROW), playerRoom());
        }}
      ></button>
    {/if}
    {@render toolbar()}

    <div class="tracks" role="group" aria-label={t("Timeline")} use:wheel={scrub} oncontextmenu={openMenu}>
      <Ruler {ed} onmarkermenu={openMarkerMenu} />
      <VideoLane {ed} />
      {@render lanes?.()}

      <div class="audio-rows scroll" bind:this={rows} style:max-height="min({rowsHeight}px, max({ROW - 4}px, 100vh - 400px))">
        {#each data.tracks as track, i (i)}
          {#if ed.edits[i] && !ed.edits[i].removed}
            <TrackRow {ed} {i} {track} />
          {/if}
        {/each}
      </div>
      {#if ed.removedTracks}
        <div class="row removed-row">
          <span class="head"></span>
          <button type="button" class="btn ghost small" onclick={restoreTracks}>
            {t("{n} removed tracks · Bring back", { n: ed.removedTracks })}
          </button>
        </div>
      {/if}

      {#if markerMenu}
        <MarkerMenu {ed} {...markerMenu} onclose={() => (markerMenu = null)} />
      {/if}
      {#if menu}
        <TimelineMenu {ed} {...menu} onclose={() => (menu = null)} />
      {/if}

      <div class="overlay" aria-hidden="true">
        <span class="dim" style:left="0" style:width="{ed.pct(ed.trimStart)}%"></span>
        <span class="dim" style:left="{ed.pct(ed.trimEnd)}%" style:right="0"></span>
        {#each ed.parts.filter((p) => p.removed) as p (p.start)}
          <span class="cut" style:left="{ed.pct(p.start)}%" style:width="{ed.pct(p.end) - ed.pct(p.start)}%"></span>
        {/each}
        {#each ed.markers as m (m.at)}
          <span class="marker-line" style:left="{ed.pct(m.at)}%"></span>
        {/each}
        <span class="playhead" style:transform="translate3d({ed.pct(ed.current)}%, 0, 0)"></span>
      </div>
    </div>
    {#if !data.tracks.length}
      <p class="meta none">{t("This clip has no sound.")}</p>
    {/if}
  </div>
{/if}

<style>
  .timeline {
    position: relative;
    flex: none;
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 8px 12px 12px;
    border-radius: 24px;
    background: var(--overlay);
  }
  /* Along the timeline's top edge; the pill shows where to grab. */
  .grip {
    position: absolute;
    top: -6px;
    left: 24px;
    right: 24px;
    height: 14px;
    z-index: 4;
    padding: 0;
    border: 0;
    background: transparent;
    cursor: ns-resize;
    touch-action: none;
    outline: none;
  }
  .grip::after {
    content: "";
    position: absolute;
    top: 8px;
    left: 50%;
    width: 36px;
    height: 4px;
    margin-left: -18px;
    border-radius: 2px;
    background: var(--line);
    transition: background-color 140ms ease, width 140ms ease, margin-left 140ms ease;
  }
  .grip:hover::after,
  .grip:focus-visible::after,
  .grip:active::after {
    width: 56px;
    margin-left: -28px;
    background: var(--accent);
  }

  /* Every lane has its name on the left and the clip's whole length on the right; the
     lanes live in components of their own and share these. Kept weak, so a lane's own
     rules always win over them. */
  .tracks {
    --head: 168px;
    position: relative;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  :where(.tracks) :global(.row) {
    display: grid;
    grid-template-columns: var(--head) 1fr;
    gap: 10px;
    align-items: stretch;
  }
  :where(.tracks) :global(.head) {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  :where(.tracks) :global(.kind) {
    width: 28px;
    height: 28px;
    flex: none;
    display: grid;
    place-items: center;
    border: 0;
    border-radius: 10px;
    background: var(--surface);
    color: var(--accent);
  }
  :where(.tracks) :global(.name) {
    font-size: 13px;
    font-weight: 700;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  :where(.tracks) :global(.lane) {
    position: relative;
    border-radius: 10px;
    background: var(--black);
    cursor: pointer;
    touch-action: none;
  }
  .removed-row {
    justify-items: start;
  }
  /* Past the height dragged at the grip the rest scroll, so the timeline keeps its height. */
  .audio-rows {
    display: flex;
    flex-direction: column;
    gap: 4px;
    overflow-y: auto;
    overscroll-behavior: contain;
  }

  /* Shading and the playhead run across every lane. */
  .overlay {
    position: absolute;
    top: 0;
    bottom: 0;
    left: calc(var(--head) + 10px);
    right: 0;
    pointer-events: none;
  }
  .dim {
    position: absolute;
    top: 20px;
    bottom: 0;
    background: rgb(10 10 9 / 0.66);
  }
  .cut {
    position: absolute;
    top: 20px;
    bottom: 0;
    background: repeating-linear-gradient(-45deg, rgb(10 10 9 / 0.8) 0 5px, rgb(10 10 9 / 0.55) 5px 10px);
  }
  .marker-line {
    position: absolute;
    top: 20px;
    bottom: 0;
    width: 0;
    border-left: 1px dashed var(--lavender);
    opacity: 0.7;
  }
  /* Move a separate layer so the line and cap do not leave paint trails over the ruler. */
  .playhead {
    position: absolute;
    top: 0;
    bottom: -4px;
    left: 0;
    width: 100%;
    will-change: transform;
  }
  .playhead::after {
    content: "";
    position: absolute;
    top: 0;
    bottom: 0;
    left: -1px;
    width: 2px;
    background: #fff;
    box-shadow: 0 0 0 1px rgb(0 0 0 / 0.35);
  }
  .playhead::before {
    content: "";
    position: absolute;
    z-index: 1;
    top: 0;
    left: -5px;
    width: 10px;
    height: 10px;
    border-radius: 3px 3px 50% 50%;
    background: #fff;
  }
  .none {
    margin: 4px 0 0;
    padding-left: calc(var(--head, 168px) + 10px);
  }
</style>
