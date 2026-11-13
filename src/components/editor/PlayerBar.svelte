<script lang="ts">
  import { follow, timeAt } from "../../lib/editor/actions";
  import type { EditorSession } from "../../lib/editor/session.svelte";
  import { position } from "../../lib/format";
  import { t } from "../../lib/i18n.svelte";
  import { tip } from "../../lib/tooltip";
  import Icon from "../Icon.svelte";
  import Slider from "../Slider.svelte";

  /**
   * The controls over the picture in full screen: a bar to scrub along the whole clip (its
   * removed parts, start and end and markers on it), play, markers, and how loud the preview
   * plays. The volume here is the player's own; the export keeps the edit's.
   */
  let { ed, onleave }: { ed: EditorSession; onleave: () => void } = $props();

  let track = $state<HTMLDivElement>();
  /** Where the pointer rests over the bar, to show the time there. */
  let hover = $state<number | null>(null);
  let dragging = $state(false);

  function scrub(e: PointerEvent) {
    if (!track) return;
    const lane = track;
    dragging = true;
    follow(e, (ev) => ed.seek(timeAt(lane, ev.clientX, ed.duration)), { now: true, stop: true });
    lane.addEventListener("pointerup", () => (dragging = false), { once: true });
  }

  function onKey(e: KeyboardEvent) {
    const step = e.shiftKey ? 1 : 5;
    if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
      e.preventDefault();
      e.stopPropagation();
      ed.seek(ed.current + (e.key === "ArrowLeft" ? -step : step));
    } else if (e.key === "Home" || e.key === "End") {
      e.preventDefault();
      e.stopPropagation();
      ed.seek(e.key === "Home" ? ed.trimStart : ed.trimEnd);
    }
  }

  const loudness = $derived(Math.round(ed.listenVolume * 100));
</script>

<div class="bar" role="group" aria-label={t("Player controls")}>
  <div
    class="scrub"
    class:active={dragging}
    bind:this={track}
    role="slider"
    tabindex="0"
    aria-label={t("Position")}
    aria-valuemin={0}
    aria-valuemax={Math.round(ed.duration * 10) / 10}
    aria-valuenow={Math.round(ed.current * 10) / 10}
    aria-valuetext={position(ed.current)}
    onpointerdown={scrub}
    onpointermove={(e) => track && (hover = timeAt(track, e.clientX, ed.duration))}
    onpointerleave={() => (hover = null)}
    onkeydown={onKey}
  >
    <div class="rail">
      <span class="out" style:left="0" style:width="{ed.pct(ed.trimStart)}%"></span>
      <span class="out" style:left="{ed.pct(ed.trimEnd)}%" style:right="0"></span>
      <span class="played" style:width="{ed.pct(ed.current)}%"></span>
      {#each ed.parts.filter((p) => p.removed) as p (p.start)}
        <span class="cut" style:left="{ed.pct(p.start)}%" style:width="{ed.pct(p.end) - ed.pct(p.start)}%"></span>
      {/each}
    </div>
    {#each ed.markers as m (m.at)}
      <span class="mark" style:left="{ed.pct(m.at)}%" title={m.name}></span>
    {/each}
    <span class="knob" style:left="{ed.pct(ed.current)}%"></span>
    {#if hover !== null}
      <span class="when mono" style:left="clamp(24px, {ed.pct(hover)}%, calc(100% - 24px))">{position(hover)}</span>
    {/if}
  </div>

  <div class="row">
    <button
      type="button"
      class="ctl"
      aria-label={ed.playing ? t("Pause") : t("Play")}
      use:tip={ed.playing ? t("Pause · Space") : t("Play · Space")}
      onclick={() => ed.toggle()}
    >
      <Icon name={ed.playing ? "pause" : "play"} size={18} />
    </button>
    <button
      type="button"
      class="ctl"
      aria-label={t("Previous marker")}
      use:tip={t("Previous marker · ,")}
      disabled={!ed.hasPrevMarker}
      onclick={() => ed.jumpMarker(-1)}
    >
      <Icon name="prevMarker" size={16} />
    </button>
    <button
      type="button"
      class="ctl"
      class:on={ed.onMarker}
      aria-pressed={ed.onMarker}
      aria-label={ed.onMarker ? t("Remove the marker here") : t("Mark this moment")}
      use:tip={ed.onMarker ? t("Remove the marker here · M") : t("Mark this moment · M")}
      onclick={() => ed.toggleMarker()}
    >
      <Icon name="flag" size={16} />
    </button>
    <button
      type="button"
      class="ctl"
      aria-label={t("Next marker")}
      use:tip={t("Next marker · .")}
      disabled={!ed.hasNextMarker}
      onclick={() => ed.jumpMarker(1)}
    >
      <Icon name="nextMarker" size={16} />
    </button>
    <span class="clock mono"><span class="ink">{position(ed.current)}</span> / {position(ed.duration)}</span>
    <span class="spacer"></span>
    <button
      type="button"
      class="ctl"
      aria-label={ed.listenMuted ? t("Unmute") : t("Mute")}
      use:tip={ed.listenMuted ? t("Unmute") : t("Mute")}
      onclick={() => ed.toggleListen()}
    >
      <Icon name={ed.listenMuted || !ed.listenVolume ? "mute" : "speaker"} size={18} />
    </button>
    <span class="volume" use:tip={t("Only how loud you hear it; the export keeps its own volume · ↑ ↓")}>
      <Slider
        label={t("Volume")}
        value={ed.listenMuted ? 0 : loudness}
        step={5}
        valuetext="{ed.listenMuted ? 0 : loudness}%"
        oninput={(v) => ed.setListenVolume(v / 100)}
      />
    </span>
    <span class="mono value">{ed.listenMuted ? 0 : loudness}%</span>
    <button type="button" class="ctl" aria-label={t("Leave full screen")} use:tip={t("Leave full screen · Esc")} onclick={onleave}>
      <Icon name="shrink" size={16} />
    </button>
  </div>
</div>

<style>
  .bar {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    z-index: 6;
    padding: 40px 22px 14px;
    display: flex;
    flex-direction: column;
    gap: 6px;
    background: linear-gradient(to top, rgb(0 0 0 / 0.78), rgb(0 0 0 / 0.4) 60%, transparent);
    color: #fff;
    transition: opacity 220ms ease;
  }
  .scrub {
    position: relative;
    height: 18px;
    display: flex;
    align-items: center;
    cursor: pointer;
    touch-action: none;
    outline: none;
  }
  .scrub:focus-visible .rail {
    outline: 2px solid var(--accent);
    outline-offset: 3px;
  }
  .rail {
    position: relative;
    width: 100%;
    height: 5px;
    border-radius: 3px;
    background: rgb(255 255 255 / 0.28);
    overflow: hidden;
    transition: height 120ms ease;
  }
  .scrub:hover .rail,
  .scrub.active .rail {
    height: 8px;
  }
  .rail > span {
    position: absolute;
    top: 0;
    bottom: 0;
  }
  .played {
    left: 0;
    background: var(--accent);
  }
  /* Before the start and after the end: not in the export. */
  .out {
    z-index: 1;
    background: rgb(10 10 9 / 0.62);
  }
  .cut {
    z-index: 1;
    background: repeating-linear-gradient(-45deg, rgb(10 10 9 / 0.85) 0 4px, rgb(10 10 9 / 0.5) 4px 8px);
  }
  .mark {
    position: absolute;
    top: -3px;
    width: 4px;
    height: 4px;
    margin-left: -2px;
    border-radius: 50%;
    background: var(--lavender);
    pointer-events: none;
  }
  .knob {
    position: absolute;
    top: 50%;
    width: 14px;
    height: 14px;
    margin: -7px 0 0 -7px;
    border-radius: 50%;
    background: #fff;
    box-shadow: 0 0 0 1px rgb(0 0 0 / 0.35);
    pointer-events: none;
  }
  .when {
    position: absolute;
    bottom: calc(100% + 6px);
    transform: translateX(-50%);
    padding: 2px 7px;
    border-radius: 8px;
    background: rgb(0 0 0 / 0.75);
    font-size: 11.5px;
    white-space: nowrap;
    pointer-events: none;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 2px;
  }
  .ctl {
    width: 36px;
    height: 36px;
    padding: 0;
    display: grid;
    place-items: center;
    border: 0;
    border-radius: 12px;
    background: transparent;
    color: #fff;
  }
  .ctl:hover:not(:disabled),
  .ctl:focus-visible {
    background: rgb(255 255 255 / 0.14);
  }
  .ctl:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .ctl.on {
    color: var(--lavender);
  }
  .clock {
    padding: 0 10px;
    font-size: 12.5px;
    color: rgb(255 255 255 / 0.7);
    white-space: nowrap;
  }
  .ink {
    color: #fff;
  }
  .spacer {
    flex: 1;
  }
  .volume {
    width: 110px;
  }
  .value {
    width: 4.5ch;
    padding-left: 6px;
    font-size: 12px;
    color: rgb(255 255 255 / 0.7);
  }
</style>
