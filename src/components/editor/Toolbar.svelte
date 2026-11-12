<script lang="ts">
  import type { EditorSession } from "../../lib/editor/session.svelte";
  import { position } from "../../lib/format";
  import { t, tParts } from "../../lib/i18n.svelte";
  import { tip } from "../../lib/tooltip";
  import Icon from "../Icon.svelte";
  import Segmented from "../Segmented.svelte";
  import Slider from "../Slider.svelte";

  let {
    ed,
    canUndo,
    canRedo,
    onundo,
    onredo,
    ontogglepart,
    onframe,
  }: {
    ed: EditorSession;
    canUndo: boolean;
    canRedo: boolean;
    onundo: () => void;
    onredo: () => void;
    ontogglepart: () => void;
    onframe: () => void;
  } = $props();
</script>

<div class="toolbar">
  <button
    type="button"
    class="tool icon"
    aria-label={ed.playing ? t("Pause") : t("Play")}
    use:tip={ed.playing ? t("Pause · Space") : t("Play · Space")}
    onclick={() => ed.toggle()}
  >
    <Icon name={ed.playing ? "pause" : "play"} size={16} />
  </button>
  <span class="clock mono"><span class="ink">{position(ed.current)}</span> / {position(ed.duration)}</span>
  <span class="sep"></span>
  <button type="button" class="tool" use:tip={t("Split at the playhead · S")} onclick={() => ed.splitHere()}>
    <Icon name="scissors" size={16} />{t("Split")}
  </button>
  <button
    type="button"
    class="tool"
    disabled={!ed.here || ed.parts.length < 2}
    use:tip={ed.here?.removed ? t("Put this part back · Del") : t("Remove the part under the playhead · Del")}
    onclick={ontogglepart}
  >
    {#if ed.here?.removed}
      <Icon name="restore" size={16} />{t("Put back")}
    {:else}
      <Icon name="trash" size={16} />{t("Remove")}
    {/if}
  </button>
  <span class="sep"></span>
  <button
    type="button"
    class="tool icon"
    aria-label={t("Previous marker")}
    use:tip={t("Previous marker · ,")}
    disabled={!ed.hasPrevMarker}
    onclick={() => ed.jumpMarker(-1)}
  >
    <Icon name="prevMarker" size={16} />
  </button>
  <button
    type="button"
    class="tool"
    aria-pressed={ed.onMarker}
    use:tip={ed.onMarker ? t("Remove the marker here · M") : t("Mark this moment · M")}
    onclick={() => ed.toggleMarker()}
  >
    <Icon name="flag" size={16} />{ed.onMarker ? t("Unmark") : t("Mark")}
  </button>
  <button
    type="button"
    class="tool icon"
    aria-label={t("Next marker")}
    use:tip={t("Next marker · .")}
    disabled={!ed.hasNextMarker}
    onclick={() => ed.jumpMarker(1)}
  >
    <Icon name="nextMarker" size={16} />
  </button>
  <span class="sep"></span>
  <button type="button" class="tool icon" aria-label={t("Undo")} use:tip={t("Undo · Ctrl + Z")} disabled={!canUndo} onclick={onundo}>
    <Icon name="undo" size={16} />
  </button>
  <button type="button" class="tool icon" aria-label={t("Redo")} use:tip={t("Redo · Ctrl + Y")} disabled={!canRedo} onclick={onredo}>
    <Icon name="redo" size={16} />
  </button>
  <span class="spacer"></span>
  <span class="length mono">
    {ed.parts.length > 1 ? `${t("{kept} of {parts} parts", { kept: ed.kept.length, parts: ed.parts.length })} · ` : ""}{#each tParts("{time} long") as part}{#if part === "{time}"}<span class="ink">{position(ed.length)}</span>{:else}{part}{/if}{/each}
  </span>
  <div class="format">
    <Segmented
      label={t("Format")}
      track="var(--black)"
      value={ed.vertical ? "tall" : "wide"}
      options={[
        { value: "wide", label: "16:9" },
        { value: "tall", label: "9:16" },
      ]}
      onchange={(v) => (ed.vertical = v === "tall")}
    />
  </div>
  <div class="master" use:tip={t("Volume of all tracks together")}>
    <button
      type="button"
      class="tool icon"
      aria-label={ed.masterVolume ? t("Mute all") : t("Unmute all")}
      onclick={() => (ed.masterVolume = ed.masterVolume ? 0 : 1)}
    >
      <Icon name={ed.masterVolume ? "speaker" : "mute"} size={16} />
    </button>
    <span class="master-slider">
      <Slider
        label={t("Volume of all tracks")}
        value={Math.round(ed.masterVolume * 100)}
        max={200}
        step={5}
        valuetext="{Math.round(ed.masterVolume * 100)}%"
        oninput={(v) => (ed.masterVolume = v / 100)}
      />
    </span>
    <span class="mono meta master-value">{Math.round(ed.masterVolume * 100)}%</span>
  </div>
  <button type="button" class="tool icon" aria-label={t("Save this frame as a screenshot")} use:tip={t("Save this frame as a screenshot")} onclick={onframe}>
    <Icon name="camera" size={16} />
  </button>
</div>

<style>
  .toolbar {
    display: flex;
    align-items: center;
    gap: 2px;
    min-height: 36px;
  }
  .tool {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 30px;
    padding: 0 11px;
    border: 0;
    border-radius: 999px;
    background: transparent;
    color: var(--muted);
    font-size: 13px;
    font-weight: 600;
    white-space: nowrap;
  }
  .tool:hover:not(:disabled) {
    background: var(--surface);
    color: var(--ink);
  }
  .tool:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .tool.icon {
    width: 30px;
    padding: 0;
    justify-content: center;
  }
  .clock {
    padding: 0 8px;
    font-size: 12px;
    color: var(--ghost);
    white-space: nowrap;
  }
  .ink {
    color: var(--ink);
  }
  .sep {
    width: 1px;
    height: 16px;
    margin: 0 6px;
    background: var(--line);
  }
  .spacer {
    flex: 1;
  }
  .length {
    padding: 0 8px;
    font-size: 12px;
    color: var(--ghost);
    white-space: nowrap;
  }
  .format {
    width: 116px;
    margin: 0 4px;
  }
  .master {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .master-slider {
    width: 96px;
  }
  .master-value {
    width: 4.5ch;
    font-size: 12px;
  }
</style>
