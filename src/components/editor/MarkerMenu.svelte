<script lang="ts">
  import { outside } from "../../lib/editor/actions";
  import { markerIcon, markerIcons } from "../../lib/editor/labels";
  import type { EditorSession } from "../../lib/editor/session.svelte";
  import { position } from "../../lib/format";
  import { t } from "../../lib/i18n.svelte";
  import { tip } from "../../lib/tooltip";
  import Icon from "../Icon.svelte";

  /** Names a marker, gives it an icon or takes it away. */
  let { ed, at, x, y, onclose }: { ed: EditorSession; at: number; x: number; y: number; onclose: () => void } = $props();

  const marker = $derived(ed.markers.find((m) => m.at === at));
</script>

{#if marker}
  {@const m = marker}
  <div class="marker-menu" role="dialog" aria-label={t("Marker at {time}", { time: position(m.at) })} style:left="{x}px" style:top="{y}px" use:outside={onclose}>
    <!-- svelte-ignore a11y_autofocus -->
    <input
      class="marker-input"
      aria-label={t("Marker name")}
      placeholder={t("Name this moment")}
      maxlength="80"
      spellcheck="false"
      autofocus
      value={m.name ?? ""}
      oninput={(e) => ed.editMarker(at, { name: e.currentTarget.value.trim() ? e.currentTarget.value : undefined })}
      onkeydown={(e) => e.key === "Enter" && onclose()}
    />
    <div class="marker-icons" role="radiogroup" aria-label={t("Icon")}>
      {#each markerIcons() as icon (icon.name)}
        <button
          type="button"
          role="radio"
          aria-checked={markerIcon(m) === icon.name}
          aria-label={icon.label}
          use:tip={icon.label}
          onclick={() => ed.editMarker(at, { icon: icon.name === "flag" ? undefined : icon.name })}
        >
          <Icon name={icon.name} size={16} />
        </button>
      {/each}
    </div>
    <button
      type="button"
      class="marker-remove"
      onclick={() => {
        ed.removeMarker(at);
        onclose();
      }}
    >
      <Icon name="trash" size={16} />{t("Remove marker")}
    </button>
  </div>
{/if}

<style>
  .marker-menu {
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
  .marker-input {
    height: 34px;
    padding: 0 12px;
    border: 0;
    border-radius: 14px;
    background: var(--black);
    color: var(--ink);
    font: inherit;
    font-weight: 600;
  }
  .marker-input:focus-visible {
    outline: 2px solid var(--lavender);
    outline-offset: -2px;
  }
  .marker-icons {
    display: grid;
    grid-template-columns: repeat(9, 1fr);
    gap: 2px;
  }
  .marker-icons button {
    height: 28px;
    padding: 0;
    display: grid;
    place-items: center;
    border: 0;
    border-radius: 10px;
    background: transparent;
    color: var(--muted);
  }
  .marker-icons button:hover {
    background: var(--hover);
    color: var(--ink);
  }
  .marker-icons button[aria-checked="true"] {
    background: var(--surface);
    color: var(--lavender);
  }
  .marker-remove {
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
  .marker-remove:hover,
  .marker-remove:focus-visible {
    background: var(--menu-point);
    outline: none;
  }
</style>
