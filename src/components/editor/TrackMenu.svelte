<script lang="ts">
  import { api } from "../../lib/api";
  import { outside } from "../../lib/editor/actions";
  import { trackName } from "../../lib/editor/labels";
  import type { EditorSession } from "../../lib/editor/session.svelte";
  import { t } from "../../lib/i18n.svelte";
  import { share } from "../../lib/share";
  import { store } from "../../lib/store.svelte";
  import { programKey, withDefault } from "../../lib/trackDefaults";
  import Icon from "../Icon.svelte";

  /** The right-click menu of an audio track. */
  let { ed, i, x, y, onclose }: { ed: EditorSession; i: number; x: number; y: number; onclose: () => void } = $props();

  async function extract() {
    onclose();
    const data = ed.data;
    if (!data) return;
    const track = data.tracks[i];
    const edit = ed.edits[i];
    const name = trackName(track);
    // The part of the track that is heard in the clip.
    const start = Math.max(ed.trimStart, edit.start ?? 0);
    const end = Math.min(ed.trimEnd, edit.end ?? data.duration);
    store.notify(t("Saving audio…"), name);
    try {
      const out = await api.extractTrack(ed.id, i, `${ed.title.trim() || t("Clip")} - ${name}`, start, end);
      store.notify(t("Audio saved"), t("{name} as M4A", { name }), [
        { label: t("Copy"), run: () => share.copy(out) },
        { label: t("Show"), run: () => api.revealPath(out) },
      ]);
    } catch (e) {
      store.notify(t("Couldn't save the audio"), String(e));
    }
  }

  /** Keeps how this track sounds now as how it starts in every clip opened without a draft. */
  function keepAsDefault() {
    onclose();
    const settings = store.settings;
    const track = ed.data?.tracks[i];
    if (!settings || !track) return;
    const edit = ed.edits[i];
    const name = trackName(track);
    const key = programKey(track) ?? track.kind;
    const next = { key, name: track.name, path: track.exe ?? null, enabled: edit.enabled, volume: edit.volume };
    store.save({ ...settings, trackDefaults: withDefault(settings.trackDefaults ?? [], next) });
    store.notify(
      t("Default saved"),
      edit.enabled
        ? t("{name} starts at {volume}% in clips you open from now on.", { name, volume: Math.round(edit.volume * 100) })
        : t("{name} starts muted in clips you open from now on.", { name }),
    );
  }

  function remove() {
    onclose();
    const track = ed.data?.tracks[i];
    ed.edits[i].removed = true;
    store.notify(t("Track removed"), t("{name} is left out of the export. Ctrl+Z brings it back.", { name: track ? trackName(track) : t("Track") }));
  }
</script>

<div class="track-menu" role="menu" tabindex="-1" style:left="{x}px" style:top="{y}px" use:outside={onclose}>
  <button type="button" role="menuitem" onclick={extract}>
    <Icon name="export" size={16} />{t("Save as audio file")}
  </button>
  <button type="button" role="menuitem" onclick={keepAsDefault}>
    <Icon name="speaker" size={16} />{t("Use as default for new clips")}
  </button>
  <button type="button" role="menuitem" class="danger" onclick={remove}>
    <Icon name="trash" size={16} />{t("Remove track")}
  </button>
</div>

<style>
  .track-menu {
    position: fixed;
    z-index: 50;
    min-width: 220px;
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
  .track-menu button {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 34px;
    padding: 0 12px;
    border: 0;
    border-radius: 14px;
    background: transparent;
    color: var(--ink);
    font-weight: 600;
    text-align: left;
  }
  .track-menu button:hover {
    background: var(--hover);
  }
  .track-menu .danger {
    color: var(--red);
  }
</style>
