<script lang="ts">
  import { follow, timeAt } from "../../lib/editor/actions";
  import { markerIcon } from "../../lib/editor/labels";
  import type { EditorSession } from "../../lib/editor/session.svelte";
  import { rulerLabel, rulerTicks } from "../../lib/editor/timeline";
  import { position } from "../../lib/format";
  import { t } from "../../lib/i18n.svelte";
  import { tip } from "../../lib/tooltip";
  import Icon from "../Icon.svelte";

  /** The time ruler with the clip's markers on it; right-click a marker to name it. */
  let { ed, onmarkermenu }: { ed: EditorSession; onmarkermenu: (at: number, e: MouseEvent) => void } = $props();

  const ticks = $derived(rulerTicks(ed.duration));

  function scrub(e: PointerEvent) {
    const lane = e.currentTarget as HTMLElement;
    follow(e, (ev) => ed.seek(timeAt(lane, ev.clientX, ed.duration)), { now: true, stop: true });
  }
</script>

<div class="row ruler-row">
  <span class="head"></span>
  <div class="lane ruler" role="presentation" onpointerdown={scrub}>
    {#each ticks as tick (tick)}
      <span class="tick" style:left="{ed.pct(tick)}%"><span class="mono">{rulerLabel(tick)}</span></span>
    {/each}
    {#each ed.markers as m (m.at)}
      <button
        type="button"
        class="marker"
        class:named={!!m.name}
        style:left="{ed.pct(m.at)}%"
        aria-label={m.name ? t("Marker {name} at {time}", { name: m.name, time: position(m.at) }) : t("Marker at {time}", { time: position(m.at) })}
        aria-haspopup="dialog"
        use:tip={m.name ? `${m.name} · ${t("Jump here · right-click to edit")}` : t("Jump here · right-click to edit")}
        onpointerdown={(e) => e.stopPropagation()}
        onclick={() => ed.seek(m.at)}
        oncontextmenu={(e) => onmarkermenu(m.at, e)}
      >
        <Icon name={markerIcon(m)} size={12} />
        {#if m.name}<span class="marker-name">{m.name}</span>{/if}
      </button>
    {/each}
  </div>
</div>

<style>
  .ruler-row .lane {
    height: 20px;
    background: transparent;
  }
  .tick {
    position: absolute;
    top: 0;
    bottom: 0;
    border-left: 1px solid var(--line);
  }
  .tick span {
    position: absolute;
    top: 2px;
    left: 4px;
    font-size: 10.5px;
    color: var(--ghost);
    white-space: nowrap;
  }
  .marker {
    position: absolute;
    top: 0;
    z-index: 3;
    width: 20px;
    height: 20px;
    margin-left: -6px;
    padding: 0;
    display: grid;
    place-items: center;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: var(--lavender);
  }
  .marker:hover,
  .marker:focus-visible {
    background: var(--surface);
    color: var(--ink);
  }
  /* A named marker shows its name beside the icon, on a backing that hides the ticks below. */
  .marker.named {
    display: flex;
    align-items: center;
    gap: 4px;
    width: auto;
    max-width: 140px;
    padding: 0 6px 0 4px;
    margin-left: -8px;
    background: var(--black);
  }
  .marker.named:hover,
  .marker.named:focus-visible {
    z-index: 4;
    max-width: 320px;
    background: var(--surface);
  }
  .marker-name {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    font-size: 11px;
    font-weight: 600;
  }
</style>
