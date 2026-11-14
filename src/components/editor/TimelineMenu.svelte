<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { api } from "../../lib/api";
  import { outside } from "../../lib/editor/actions";
  import { trackName } from "../../lib/editor/labels";
  import type { EditorSession } from "../../lib/editor/session.svelte";
  import { nextSpeed, speedAt, speedLabel } from "../../lib/editor/timeline";
  import { position } from "../../lib/format";
  import { t } from "../../lib/i18n.svelte";
  import { share } from "../../lib/share";
  import { store } from "../../lib/store.svelte";
  import { programKey, withDefault } from "../../lib/trackDefaults";
  import Icon from "../Icon.svelte";

  /**
   * The timeline's right-click menu. On a lane it works at the time under the pointer:
   * split, take a part out, start and end, a marker, text, a still. On an audio track it adds
   * that track's own: silence it in this part, where it starts and ends, mute, save, remove.
   * On a text's bar it offers that text.
   */
  let {
    ed,
    at,
    track,
    text,
    x,
    y,
    onclose,
  }: {
    ed: EditorSession;
    /** The time right-clicked; null on a track's name, where only its own entries apply. */
    at: number | null;
    /** The audio track right-clicked, by index. */
    track: number | null;
    /** The text whose bar was right-clicked, by index. */
    text: number | null;
    x: number;
    y: number;
    onclose: () => void;
  } = $props();

  let menu = $state<HTMLDivElement>();
  // Where it was opened; moved once it knows its own size.
  let left = $state(untrack(() => x));
  let top = $state(untrack(() => y));

  // Kept inside the window: it opens up or to the left where there is no room.
  onMount(() => {
    if (!menu) return;
    const r = menu.getBoundingClientRect();
    left = Math.max(8, Math.min(x, innerWidth - r.width - 8));
    top = y + r.height > innerHeight - 8 ? Math.max(8, y - r.height) : y;
    menu.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus();
  });

  const part = $derived(at === null ? null : ed.partAt(at));
  const marker = $derived(at === null ? undefined : ed.markerNear(at));
  const partSpeed = $derived(part ? speedAt(ed.speeds, (part.start + part.end) / 2) : 1);
  const edit = $derived(track === null ? undefined : ed.edits[track]);
  const name = $derived(track !== null && ed.data?.tracks[track] ? trackName(ed.data.tracks[track]) : "");
  const silentHere = $derived(track !== null && part ? ed.trackSilentIn(track, part) : false);

  /** Runs a menu entry, then closes the menu: closed first, its props would be gone. */
  const run = (action: () => void) => () => {
    action();
    onclose();
  };

  function togglePart() {
    if (at !== null && ed.togglePart(at) === "last") store.notify(t("That's the last part"), t("A clip needs something left in it."));
  }

  async function saveFrame() {
    if (at === null) return;
    await api.saveFrame(ed.id, at);
    store.notify(t("Screenshot saved"), t("It's in your library."));
  }

  async function extract() {
    const data = ed.data;
    if (!data || track === null || !edit) return;
    const i = track;
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
    const settings = store.settings;
    const meta = track === null ? undefined : ed.data?.tracks[track];
    if (!settings || !meta || !edit) return;
    const key = programKey(meta) ?? meta.kind;
    const next = { key, name: meta.name, path: meta.exe ?? null, enabled: edit.enabled, volume: edit.volume };
    store.save({ ...settings, trackDefaults: withDefault(settings.trackDefaults ?? [], next) });
    store.notify(
      t("Default saved"),
      edit.enabled
        ? t("{name} starts at {volume}% in clips you open from now on.", { name, volume: Math.round(edit.volume * 100) })
        : t("{name} starts muted in clips you open from now on.", { name }),
    );
  }

  function removeTrack() {
    if (!edit) return;
    edit.removed = true;
    store.notify(t("Track removed"), t("{name} is left out of the export. Ctrl+Z brings it back.", { name: name || t("Track") }));
  }

  /** Up and down move between the entries, as in any menu. */
  function onKey(e: KeyboardEvent) {
    if (!menu || !["ArrowDown", "ArrowUp", "Home", "End"].includes(e.key)) return;
    e.preventDefault();
    e.stopPropagation();
    const items = [...menu.querySelectorAll<HTMLButtonElement>("button:not(:disabled)")];
    const now = items.indexOf(document.activeElement as HTMLButtonElement);
    const next =
      e.key === "Home" ? 0 : e.key === "End" ? items.length - 1 : (now + (e.key === "ArrowDown" ? 1 : -1) + items.length) % items.length;
    items[next]?.focus();
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="timeline-menu"
  role="menu"
  tabindex="-1"
  aria-label={at !== null ? t("At {time}", { time: position(at) }) : name}
  bind:this={menu}
  style:left="{left}px"
  style:top="{top}px"
  use:outside={onclose}
  onkeydown={onKey}
>
  {#if at !== null}
    <span class="heading mono" aria-hidden="true">{position(at)}</span>
    <button
      type="button"
      role="menuitem"
      onclick={run(() => {
        ed.seek(at);
        ed.play();
      })}
    >
      <Icon name="play" size={16} />{t("Play from here")}
    </button>
    <button type="button" role="menuitem" disabled={!ed.canSplit(at)} onclick={run(() => ed.splitHere(at))}>
      <Icon name="scissors" size={16} />{t("Split here")}<span class="key">S</span>
    </button>
    {#if part?.removed}
      <button type="button" role="menuitem" onclick={run(togglePart)}>
        <Icon name="restore" size={16} />{t("Put this part back")}<span class="key">Del</span>
      </button>
    {:else}
      <button type="button" role="menuitem" disabled={!part || ed.kept.length < 2} onclick={run(togglePart)}>
        <Icon name="trash" size={16} />{t("Remove this part")}<span class="key">Del</span>
      </button>
    {/if}
    <button type="button" role="menuitem" disabled={!part || part.removed} onclick={run(() => ed.cycleSpeed(at))}>
      <Icon name="gauge" size={16} />{t("Play this part at {speed}", { speed: speedLabel(nextSpeed(partSpeed)) })}
    </button>
    <button type="button" role="menuitem" onclick={run(() => ed.setStart(at))}>
      <Icon name="chevronRight" size={16} />{t("Start the clip here")}<span class="key">I</span>
    </button>
    <button type="button" role="menuitem" onclick={run(() => ed.setEnd(at))}>
      <Icon name="back" size={16} />{t("End the clip here")}<span class="key">O</span>
    </button>
    <span class="sep" role="separator"></span>
    <button type="button" role="menuitem" onclick={run(() => ed.toggleMarker(at))}>
      <Icon name="flag" size={16} />{marker ? t("Remove the marker here") : t("Mark this moment")}<span class="key">M</span>
    </button>
    {#if text === null}
      <button type="button" role="menuitem" onclick={run(() => ed.addText(t("Your text"), at))}>
        <Icon name="text" size={16} />{t("Add text here")}
      </button>
    {/if}
    <button type="button" role="menuitem" onclick={run(saveFrame)}>
      <Icon name="camera" size={16} />{t("Save this frame as a screenshot")}
    </button>
  {/if}

  {#if text !== null && ed.texts[text]}
    <span class="sep" role="separator"></span>
    <span class="heading" aria-hidden="true">{ed.texts[text].text}</span>
    <button type="button" role="menuitem" onclick={run(() => (ed.selectedText = text))}>
      <Icon name="edit" size={16} />{t("Edit text")}
    </button>
    <button type="button" role="menuitem" class="danger" onclick={run(() => ed.removeText(text))}>
      <Icon name="trash" size={16} />{t("Remove text")}
    </button>
  {/if}

  {#if track !== null && edit}
    {#if at !== null}<span class="sep" role="separator"></span>{/if}
    <span class="heading" aria-hidden="true">{name}</span>
    {#if part && !part.removed}
      <button type="button" role="menuitem" onclick={run(() => ed.silenceTrack(track, part, !silentHere))}>
        <Icon name={silentHere ? "speaker" : "mute"} size={16} />{silentHere
          ? t("Bring {name} back in this part", { name })
          : t("Silence {name} in this part", { name })}
      </button>
    {/if}
    {#if at !== null}
      <button type="button" role="menuitem" onclick={run(() => ed.setTrackStart(track, at))}>
        <Icon name="chevronRight" size={16} />{t("{name} starts here", { name })}
      </button>
      <button type="button" role="menuitem" onclick={run(() => ed.setTrackEnd(track, at))}>
        <Icon name="back" size={16} />{t("{name} ends here", { name })}
      </button>
    {/if}
    <button type="button" role="menuitem" onclick={run(() => (edit.enabled = !edit.enabled))}>
      <Icon name={edit.enabled ? "mute" : "speaker"} size={16} />{edit.enabled ? t("Mute {name}", { name }) : t("Unmute {name}", { name })}
    </button>
    <button type="button" role="menuitem" onclick={run(extract)}>
      <Icon name="export" size={16} />{t("Save as audio file")}
    </button>
    <button type="button" role="menuitem" onclick={run(keepAsDefault)}>
      <Icon name="speaker" size={16} />{t("Use as default for new clips")}
    </button>
    <button type="button" role="menuitem" class="danger" onclick={run(removeTrack)}>
      <Icon name="trash" size={16} />{t("Remove track")}
    </button>
  {/if}
</div>

<style>
  .timeline-menu {
    position: fixed;
    z-index: 50;
    min-width: 240px;
    max-width: 340px;
    max-height: calc(100vh - 16px);
    overflow-y: auto;
    padding: 6px;
    border-radius: 20px;
    background: var(--raised);
    box-shadow:
      0 0 0 1px rgb(255 255 255 / 0.04),
      0 14px 36px rgb(0 0 0 / 0.5);
    display: flex;
    flex-direction: column;
    gap: 1px;
    outline: none;
  }
  .heading {
    padding: 4px 12px 2px;
    font-size: 11.5px;
    font-weight: 700;
    color: var(--ghost);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  button {
    flex: none;
    display: flex;
    align-items: center;
    gap: 10px;
    height: 32px;
    padding: 0 12px;
    border: 0;
    border-radius: 14px;
    background: transparent;
    color: var(--ink);
    font-weight: 600;
    text-align: left;
    white-space: nowrap;
  }
  button:hover:not(:disabled),
  button:focus-visible {
    background: var(--menu-point);
    outline: none;
  }
  button:hover:not(:disabled) :global(svg),
  button:focus-visible :global(svg) {
    color: var(--ink);
  }
  button:disabled {
    opacity: 0.4;
    cursor: default;
  }
  button :global(svg) {
    flex: none;
    color: var(--muted);
  }
  .danger,
  .danger :global(svg) {
    color: var(--red);
  }
  .key {
    margin-left: auto;
    padding-left: 18px;
    font-size: 11.5px;
    font-weight: 600;
    color: var(--ghost);
  }
  .sep {
    flex: none;
    height: 1px;
    margin: 4px 8px;
    background: var(--line);
  }
</style>
