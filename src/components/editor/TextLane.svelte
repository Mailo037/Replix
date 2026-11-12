<script lang="ts">
  import { follow, outside, timeAt } from "../../lib/editor/actions";
  import type { EditorSession } from "../../lib/editor/session.svelte";
  import { position } from "../../lib/format";
  import { t } from "../../lib/i18n.svelte";
  import { fit } from "../../lib/place";
  import { tip } from "../../lib/tooltip";
  import Icon from "../Icon.svelte";
  import Slider from "../Slider.svelte";

  /**
   * The texts on the timeline: one bar each from where it shows to where it goes. Drag a bar
   * to move it, its ends to change how long it shows; clicking one opens its panel.
   */
  let { ed }: { ed: EditorSession } = $props();

  let lane = $state<HTMLDivElement>();
  let panel = $state<HTMLDivElement>();
  const selected = $derived(ed.selectedText !== null ? ed.texts[ed.selectedText] : undefined);

  /** The shortest a text shows, in seconds. */
  const SHORTEST = 0.3;

  function drag(i: number, side: "start" | "end" | "both", e: PointerEvent) {
    if (!lane) return;
    const strip = lane;
    const text = ed.texts[i];
    const from = { start: text.start, end: text.end, at: timeAt(strip, e.clientX, ed.duration) };
    ed.selectedText = i;
    follow(
      e,
      (ev) => {
        const at = timeAt(strip, ev.clientX, ed.duration);
        const t = ed.texts[i];
        if (!t) return;
        if (side === "start") t.start = Math.min(Math.max(0, at), t.end - SHORTEST);
        else if (side === "end") t.end = Math.max(Math.min(ed.duration, at), t.start + SHORTEST);
        else {
          const length = from.end - from.start;
          const start = Math.min(Math.max(0, from.start + at - from.at), ed.duration - length);
          [t.start, t.end] = [start, start + length];
        }
        ed.seek(side === "end" ? t.end - 0.05 : t.start);
      },
      { stop: true },
    );
  }

  // The panel opens next to the selected text's bar, and follows a new selection there.
  $effect(() => {
    const i = ed.selectedText;
    if (!panel || i === null || !lane) return;
    const bar = lane.querySelector<HTMLElement>(`[data-text-bar="${i}"]`);
    if (bar) fit(panel, bar.getBoundingClientRect(), { gap: 8 });
  });
</script>

{#if ed.texts.length}
  <div class="row">
    <div class="head">
      <span class="kind"><Icon name="text" size={16} /></span>
      <span class="name">{t("Text")}</span>
    </div>
    <div class="lane text-lane" bind:this={lane} role="presentation">
      {#each ed.texts as text, i (i)}
        <button
          type="button"
          class="bar"
          class:selected={ed.selectedText === i}
          data-text-bar={i}
          style:left="{ed.pct(text.start)}%"
          style:width="{ed.pct(text.end) - ed.pct(text.start)}%"
          aria-label={t("Text “{text}” from {from} to {to}", { text: text.text, from: position(text.start), to: position(text.end) })}
          use:tip={t("Drag to move · drag an end to show it longer or shorter")}
          onpointerdown={(e) => drag(i, "both", e)}
        >
          <span class="edge" role="presentation" onpointerdown={(e) => drag(i, "start", e)}></span>
          <span class="label">{text.text}</span>
          <span class="edge end" role="presentation" onpointerdown={(e) => drag(i, "end", e)}></span>
        </button>
      {/each}
    </div>
  </div>
{/if}

{#if selected && ed.selectedText !== null}
  {@const i = ed.selectedText}
  <div class="text-menu" role="dialog" aria-label={t("Text")} bind:this={panel} use:outside={() => (ed.selectedText = null)}>
    <!-- svelte-ignore a11y_autofocus -->
    <input
      class="words"
      aria-label={t("Words")}
      placeholder={t("Your text")}
      maxlength="80"
      spellcheck="false"
      autofocus
      value={selected.text}
      oninput={(e) => {
        const words = e.currentTarget.value;
        selected.text = words;
        ed.setText(i, words);
      }}
      onkeydown={(e) => e.key === "Enter" && (ed.selectedText = null)}
    />
    <div class="size">
      <span class="meta">{t("Size")}</span>
      <Slider
        label={t("Size")}
        value={Math.round(selected.size * 100)}
        min={3}
        max={20}
        valuetext="{Math.round(selected.size * 100)}%"
        oninput={(v) => ed.setTextSize(i, v / 100)}
      />
    </div>
    <button type="button" class="remove" onclick={() => ed.removeText(i)}>
      <Icon name="trash" size={16} />{t("Remove text")}
    </button>
  </div>
{/if}

<style>
  .text-lane {
    height: 30px;
  }
  .bar {
    position: absolute;
    top: 3px;
    bottom: 3px;
    min-width: 12px;
    display: flex;
    align-items: center;
    padding: 0;
    border: 0;
    border-radius: 8px;
    background: color-mix(in srgb, var(--pink) 30%, var(--surface));
    color: var(--ink);
    cursor: grab;
    touch-action: none;
    overflow: hidden;
  }
  .bar.selected {
    box-shadow: inset 0 0 0 2px var(--pink);
  }
  .label {
    flex: 1;
    min-width: 0;
    padding: 0 6px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 11.5px;
    font-weight: 700;
    pointer-events: none;
  }
  .edge {
    width: 6px;
    align-self: stretch;
    flex: none;
    background: var(--pink);
    cursor: ew-resize;
    opacity: 0;
    transition: opacity 140ms ease;
  }
  .bar:hover .edge,
  .bar.selected .edge {
    opacity: 1;
  }
  .text-menu {
    position: fixed;
    z-index: 50;
    width: 260px;
    padding: 8px;
    border-radius: 20px;
    background: var(--raised);
    box-shadow:
      0 0 0 1px rgb(255 255 255 / 0.04),
      0 14px 36px rgb(0 0 0 / 0.5);
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .words {
    height: 34px;
    padding: 0 12px;
    border: 0;
    border-radius: 14px;
    background: var(--black);
    color: var(--ink);
    font: inherit;
    font-weight: 600;
  }
  .words:focus-visible {
    outline: 2px solid var(--pink);
    outline-offset: -2px;
  }
  .size {
    display: grid;
    grid-template-columns: auto 1fr;
    align-items: center;
    gap: 12px;
    padding: 0 6px 0 12px;
  }
  .remove {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 34px;
    padding: 0 12px;
    border: 0;
    border-radius: 14px;
    background: transparent;
    color: var(--red);
    font-weight: 600;
    text-align: left;
  }
  .remove:hover {
    background: var(--hover);
  }
</style>
