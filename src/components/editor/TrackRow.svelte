<script lang="ts">
  import type { EditorTrack } from "../../lib/api";
  import { appIcon } from "../../lib/appIcons.svelte";
  import { follow, timeAt } from "../../lib/editor/actions";
  import { TRACK_ICONS, trackName } from "../../lib/editor/labels";
  import type { EditorSession } from "../../lib/editor/session.svelte";
  import { t } from "../../lib/i18n.svelte";
  import { tip } from "../../lib/tooltip";
  import Icon from "../Icon.svelte";
  import Slider from "../Slider.svelte";
  import Waveform from "../Waveform.svelte";

  /** One audio track: mute, volume, its waveform and where in the clip it is heard. */
  let { ed, i, track }: { ed: EditorSession; i: number; track: EditorTrack } = $props();

  const edit = $derived(ed.edits[i]);
  const name = $derived(trackName(track));
  const icon = $derived(appIcon(track.exe));
  const from = $derived(edit.start ?? 0);
  const to = $derived(edit.end ?? ed.duration);

  function scrub(e: PointerEvent) {
    const lane = e.currentTarget as HTMLElement;
    follow(e, (ev) => ed.seek(timeAt(lane, ev.clientX, ed.duration)), { now: true, stop: true });
  }

  /** Drags the track's own start or end; it is silent outside them. */
  function dragEdge(side: "start" | "end", e: PointerEvent) {
    const lane = (e.currentTarget as HTMLElement).parentElement!;
    const duration = ed.duration;
    follow(
      e,
      (ev) => {
        const at = timeAt(lane, ev.clientX, duration);
        const edit = ed.edits[i];
        if (side === "start") ed.setTrackStart(i, at);
        else ed.setTrackEnd(i, at);
        ed.seek(side === "start" ? (edit.start ?? 0) : (edit.end ?? duration));
      },
      { stop: true },
    );
  }
</script>

<!-- Right-click opens the timeline's menu with this track's own entries (data-track). -->
<div class="row" class:off={!edit.enabled} role="group" aria-label={name} data-track={i}>
  <div class="head">
    <button
      type="button"
      class="kind mute"
      aria-pressed={!edit.enabled}
      aria-label={edit.enabled ? t("Mute {name}", { name }) : t("Unmute {name}", { name })}
      use:tip={edit.enabled ? t("Mute") : t("Unmute")}
      onclick={() => (edit.enabled = !edit.enabled)}
    >
      {#if !edit.enabled}
        <Icon name="mute" size={16} />
      {:else if icon}
        <img class="app-icon" src={icon} alt="" />
      {:else}
        <Icon name={TRACK_ICONS[track.kind] ?? "speaker"} size={16} />
      {/if}
    </button>
    <div class="head-body">
      <span class="name">{name}</span>
      <Slider
        label={t("{name} volume", { name })}
        value={Math.round(edit.volume * 100)}
        valuetext="{Math.round(edit.volume * 100)}%"
        disabled={!edit.enabled}
        wheel={false}
        oninput={(v) => (edit.volume = v / 100)}
      />
    </div>
  </div>
  <div class="lane audio-lane" role="presentation" onpointerdown={scrub}>
    <Waveform id={ed.id} track={i} duration={ed.duration} muted={!edit.enabled} />
    {#if from > 0}<span class="track-dim" style:left="0" style:width="{ed.pct(from)}%"></span>{/if}
    {#each edit.silent ?? [] as [a, b] (a)}
      <span class="track-silent" style:left="{ed.pct(a)}%" style:width="{ed.pct(b) - ed.pct(a)}%" aria-hidden="true">
        <Icon name="mute" size={12} />
      </span>
    {/each}
    {#if to < ed.duration}<span class="track-dim" style:left="{ed.pct(to)}%" style:right="0"></span>{/if}
    <button
      type="button"
      class="track-handle"
      class:set={from > 0}
      style:left="{ed.pct(from)}%"
      aria-label={t("Where {name} starts", { name })}
      use:tip={t("Drag to set where this track starts · double-click to reset")}
      onpointerdown={(e) => dragEdge("start", e)}
      ondblclick={() => (edit.start = undefined)}
    ></button>
    <button
      type="button"
      class="track-handle end"
      class:set={to < ed.duration}
      style:left="{ed.pct(to)}%"
      aria-label={t("Where {name} ends", { name })}
      use:tip={t("Drag to set where this track ends · double-click to reset")}
      onpointerdown={(e) => dragEdge("end", e)}
      ondblclick={() => (edit.end = undefined)}
    ></button>
  </div>
</div>

<style>
  .mute {
    padding: 0;
    transition: background-color 160ms ease, color 160ms ease;
  }
  .mute:hover {
    background: var(--hover);
  }
  .off .mute {
    background: transparent;
    color: var(--ghost);
  }
  .off .name {
    color: var(--ghost);
  }
  .head-body {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .app-icon {
    width: 18px;
    height: 18px;
    object-fit: contain;
  }
  .audio-lane {
    height: 38px;
    overflow: hidden;
  }
  .track-dim {
    position: absolute;
    top: 0;
    bottom: 0;
    background: rgb(10 10 9 / 0.7);
    pointer-events: none;
  }
  /* Taken out of this track alone: hatched, with the mute sign where there is room. */
  .track-silent {
    position: absolute;
    top: 0;
    bottom: 0;
    display: grid;
    place-items: center;
    overflow: hidden;
    background: repeating-linear-gradient(-45deg, rgb(10 10 9 / 0.82) 0 5px, rgb(10 10 9 / 0.55) 5px 10px);
    color: var(--ghost);
    pointer-events: none;
  }
  .track-handle {
    position: absolute;
    top: 0;
    bottom: 0;
    z-index: 2;
    width: 6px;
    padding: 0;
    border: 0;
    border-radius: 6px 0 0 6px;
    background: var(--lavender);
    opacity: 0;
    cursor: ew-resize;
    touch-action: none;
    transition: opacity 140ms ease;
  }
  .track-handle.end {
    margin-left: -6px;
    border-radius: 0 6px 6px 0;
  }
  .audio-lane:hover .track-handle,
  .track-handle.set,
  .track-handle:focus-visible {
    opacity: 1;
  }
</style>
