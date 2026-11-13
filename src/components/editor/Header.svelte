<script lang="ts">
  import type { Preset } from "../../lib/api";
  import type { EditorSession } from "../../lib/editor/session.svelte";
  import { time } from "../../lib/format";
  import { t } from "../../lib/i18n.svelte";
  import { pop } from "../../lib/motion";
  import { tip } from "../../lib/tooltip";
  import Icon from "../Icon.svelte";

  let {
    ed,
    progress,
    onback,
    onsave,
    onexport,
    oncancel,
  }: {
    ed: EditorSession;
    /** How far the export is, 0 to 1; null while none runs. */
    progress: number | null;
    onback: () => void;
    onsave: () => void;
    onexport: (preset: Preset) => void;
    oncancel: () => void;
  } = $props();

  let menu = $state(false);

  function choose(preset: Preset) {
    menu = false;
    onexport(preset);
  }
</script>

<svelte:window onclick={(e) => menu && !(e.target as HTMLElement).closest(".split") && (menu = false)} />

<header class="top">
  <button type="button" class="icon-btn back" aria-label={t("Back to clips")} use:tip={t("Back to clips · Esc")} onclick={onback}>
    <Icon name="back" size={16} />
  </button>
  {#if ed.data}
    <div class="heading">
      <label class="visually-hidden" for="clip-title">{t("Title")}</label>
      <input id="clip-title" class="title" bind:value={ed.title} spellcheck="false" />
      <span class="meta">
        {ed.data.game ? `${ed.data.game} · ` : ""}{time(ed.data.created)} · {ed.data.width}×{ed.data.height}{ed.data.draft ? ` · ${t("Draft")}` : ""}
      </span>
    </div>
    <div class="actions">
      {#if progress !== null}
        <button type="button" class="btn small ghost" onclick={oncancel}>{t("Cancel export")}</button>
      {/if}
      <button type="button" class="btn small ghost" onclick={onsave} use:tip={"Ctrl + S"}>{t("Save draft")}</button>
      <div class="split">
        <button type="button" class="btn small primary" disabled={progress !== null} onclick={() => choose("original")}>
          {progress !== null ? t("Exporting {n}%", { n: Math.round(progress * 100) }) : t("Export")}
        </button>
        <button
          type="button"
          class="btn small primary caret"
          aria-label={t("More export options")}
          use:tip={t("More formats")}
          aria-expanded={menu}
          disabled={progress !== null}
          onclick={() => (menu = !menu)}
        >
          <Icon name="chevronDown" size={16} />
        </button>
        {#if menu}
          <div class="menu" role="menu" transition:pop>
            <button type="button" role="menuitem" onclick={() => choose("original")}>
              <span>{t("Full quality")}</span><span class="meta">{t("MP4, as recorded")}</span>
            </button>
            <button type="button" role="menuitem" onclick={() => choose("discord")}>
              <span>{t("For Discord")}</span><span class="meta">{t("Under 10 MB")}</span>
            </button>
            <button type="button" role="menuitem" onclick={() => choose("tracks")}>
              <span>{t("Separate tracks")}</span><span class="meta">{t("MKV for video editors, every track on its own")}</span>
            </button>
            <button type="button" role="menuitem" onclick={() => choose("gif")}>
              <span>{t("GIF")}</span>
              <span class="meta">{ed.length > 20 ? t("Long GIFs get big. Under 15 s works best.") : t("Silent, loops, 15 fps")}</span>
            </button>
          </div>
        {/if}
      </div>
    </div>
  {/if}
</header>

<style>
  .top {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 40px;
  }
  .back {
    background: transparent;
    color: var(--muted);
  }
  .back:hover {
    background: var(--surface);
    color: var(--ink);
  }
  .heading {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: baseline;
    gap: 10px;
  }
  .title {
    min-width: 80px;
    max-width: 50%;
    field-sizing: content;
    padding: 3px 6px;
    margin-left: -6px;
    border: 0;
    border-radius: 10px;
    background: transparent;
    font-size: 22px;
    font-weight: 800;
    letter-spacing: -0.025em;
    outline: none;
  }
  .title:hover,
  .title:focus {
    background: var(--overlay);
  }
  .heading .meta {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .actions {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .split {
    position: relative;
    display: flex;
    gap: 2px;
  }
  .split .btn.primary:first-child {
    border-radius: 999px 6px 6px 999px;
    min-width: 80px;
  }
  .caret {
    width: 34px;
    padding: 0 2px 0 0;
    border-radius: 6px 999px 999px 6px;
  }
  .menu {
    position: absolute;
    right: 0;
    top: calc(100% + 6px);
    z-index: 20;
    width: 240px;
    padding: 6px;
    border-radius: 20px;
    background: var(--raised);
    box-shadow:
      0 0 0 1px rgb(255 255 255 / 0.04),
      0 14px 36px rgb(0 0 0 / 0.5);
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .menu button {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    padding: 8px 12px;
    border: 0;
    border-radius: 14px;
    background: transparent;
    font-weight: 700;
    text-align: left;
  }
  .menu button:hover {
    background: var(--hover);
  }
</style>
