<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { onDestroy, onMount, untrack } from "svelte";
  import { api, type Draft, type Marker, type Preset } from "../lib/api";
  import { History } from "../lib/editor/history.svelte";
  import { EditorSession } from "../lib/editor/session.svelte";
  import { currentRound, endEditorRound } from "../lib/editorRound.svelte";
  import { t } from "../lib/i18n.svelte";
  import { share } from "../lib/share";
  import { store } from "../lib/store.svelte";
  import Header from "./editor/Header.svelte";
  import TextLane from "./editor/TextLane.svelte";
  import Player from "./editor/Player.svelte";
  import Timeline from "./editor/Timeline.svelte";
  import Toolbar from "./editor/Toolbar.svelte";

  // The editor for one clip. What is being changed lives in an EditorSession that its parts
  // share; this file loads the clip, keeps playback and the window in step, and handles the
  // keys, the undo history, saving and exporting.

  let { id }: { id: string } = $props();

  const ed = new EditorSession(untrack(() => id));
  let failure = $state<string | null>(null);
  let progress = $state<number | null>(null);
  let fullscreen = $state(false);
  let viewer = $state<HTMLDivElement>();
  let player = $state<ReturnType<typeof Player>>();

  /** Markers as last saved, so only a change is sent. */
  let savedMarkers = "";
  let markerTimer = 0;

  /** Opening was stopped by hiding the window; it is tried again once the window is back. */
  let interrupted = $state(false);
  /** Counts loads, so an answer to an earlier one cannot overwrite a newer one. */
  let loads = 0;

  onMount(load);

  async function load() {
    interrupted = false;
    const mine = ++loads;
    try {
      const d = await api.openEditor(id, currentRound());
      if (mine !== loads) return;
      ed.load(d, store.settings);
      savedMarkers = JSON.stringify(ed.markers);
    } catch (e) {
      if (mine !== loads) return;
      // Stopped, not failed: the window was hidden while the tracks were being prepared.
      if (e === "stopped") interrupted = true;
      else failure = String(e);
    }
  }

  // Which export this editor waits for; others may run before it.
  let ticket = 0;
  const unlisten = listen<[number, number]>("export-progress", (e) => {
    const [of, share] = e.payload;
    if (progress !== null && of === ticket) progress = share;
  });
  // The filmstrip is made in the background the first time a clip opens.
  const unlistenStrip = listen<[string, string]>("filmstrip", (e) => {
    if (ed.data && e.payload[0] === id) ed.data.filmstrip = e.payload[1];
  });

  onDestroy(() => {
    // A long session's filmstrip and waveforms take a while; nobody needs them now.
    endEditorRound();
    unlisten.then((f) => f());
    unlistenStrip.then((f) => f());
  });

  $effect(() => {
    if (!ed.playing) return;
    let frame = requestAnimationFrame(tick);

    function tick() {
      const video = ed.video;
      if (!ed.playing || !video) return;
      ed.current = video.currentTime;
      // Removed parts are jumped over, as they will be in the export.
      const cut = ed.cuts.find(([a, b]) => ed.current >= a && ed.current < b - 0.02);
      if (cut && cut[1] < ed.playEnd) ed.seek(cut[1]);
      else if (ed.current >= ed.playEnd - 0.02) {
        ed.pause();
        ed.seek(ed.playStart);
        return;
      }
      ed.syncAudio(false);
      frame = requestAnimationFrame(tick);
    }
    return () => cancelAnimationFrame(frame);
  });

  // Hidden in the tray or minimised, nobody is watching: stop decoding video and audio.
  /** The window was hidden since the filmstrip was last asked for. */
  let hiddenSince = false;
  /** This hide already ended the editor's round. */
  let endedForHide = false;

  /**
   * Where the preview was when the window hid: hidden, the video and audio are let go so
   * WebView2 can free their buffers, and coming back they load again from here.
   */
  let resumeAt: number | null = null;

  $effect(() => {
    if (!store.visible) {
      // Once per hide: pausing changes `playing`, which runs this again.
      if (!endedForHide) {
        endEditorRound();
        resumeAt = untrack(() => ed.current);
      }
      endedForHide = true;
      hiddenSince = true;
      if (ed.playing) ed.pause();
      return;
    }
    endedForHide = false;
    // Hiding ended the round and stopped what it was making; back on screen, ask in the new
    // one. The stop may only be reported after the window is back; this runs again then.
    if (interrupted && store.visible) {
      hiddenSince = false;
      load();
    } else if (hiddenSince && ed.data && !ed.data.filmstrip) {
      hiddenSince = false;
      api
        .openEditor(id, currentRound())
        .then((d) => ed.data && !ed.data.filmstrip && (ed.data.filmstrip = d.filmstrip))
        .catch(() => {});
    }
  });

  // Another program in front, perhaps a game: the preview stops, so the page draws nothing
  // until Replix is back.
  $effect(() => {
    if (!store.focused && untrack(() => ed.playing)) ed.pause();
  });

  $effect(() => {
    ed.audios.forEach((audio, i) => {
      if (!audio) return;
      const edit = ed.edits[i];
      // Outside its own start and end the track is silent, as it will be in the export.
      const heard = edit?.enabled && ed.current >= (edit.start ?? 0) && ed.current <= (edit.end ?? Infinity);
      // The preview can't go above full volume; the export can.
      audio.volume = heard ? Math.min(1, edit.volume * ed.masterVolume) : 0;
    });
  });

  // A slow part plays slower in the preview too; the sound keeps its pitch, as in the export.
  $effect(() => {
    const rate = ed.rate;
    if (ed.video) ed.video.playbackRate = rate;
    for (const audio of ed.audios) if (audio) audio.playbackRate = rate;
  });

  // Markers are part of the clip, not of a draft: a change is saved at once.
  $effect(() => {
    const now = JSON.stringify(ed.markers);
    if (!ed.data || now === savedMarkers) return;
    savedMarkers = now;
    const list = $state.snapshot(ed.markers);
    clearTimeout(markerTimer);
    markerTimer = window.setTimeout(() => {
      api.setMarkers(id, list).catch((e) => store.notify(t("Markers not saved"), String(e)));
    }, 300);
  });

  // Undo history. A quick run of changes (dragging a handle, moving a slider) settles into
  // one entry. Markers are in it too, so Ctrl+Z brings back one taken away by mistake.
  const history = new History();
  let settle = 0;
  const snapshot = $derived(ed.data ? JSON.stringify({ ...ed.draft(), markers: $state.snapshot(ed.markers) }) : "");

  $effect(() => {
    const now = snapshot;
    if (!now || now === history.committed) return;
    if (!history.committed) history.reset(now);
    else {
      clearTimeout(settle);
      settle = window.setTimeout(commit, 400);
    }
  });

  function commit() {
    clearTimeout(settle);
    history.commit(snapshot);
  }

  function restore(json: string | null) {
    if (json === null) return;
    const d = JSON.parse(json) as Draft & { markers?: Marker[] };
    if (d.markers) ed.markers = d.markers;
    ed.apply(d);
    if (ed.current < ed.trimStart || ed.current > ed.trimEnd) ed.seek(ed.trimStart);
  }

  function undo() {
    commit();
    restore(history.undo());
  }

  function redo() {
    commit();
    restore(history.redo());
  }

  function togglePart() {
    if (ed.togglePart() === "last") store.notify(t("That's the last part"), t("A clip needs something left in it."));
  }

  function onKey(e: KeyboardEvent) {
    if (fullscreen && e.key === "Escape") {
      e.preventDefault();
      player?.toggleFullscreen();
      return;
    }
    if (!e.ctrlKey && !e.metaKey && !e.altKey && e.key.toLowerCase() === "f" && !(e.target instanceof HTMLInputElement && e.target.type !== "range")) {
      e.preventDefault();
      player?.toggleFullscreen();
      return;
    }
    // Text fields keep their own undo; sliders don't.
    const target = e.target;
    if ((target instanceof Element && target.matches("input:not([type=range]), textarea")) || !ed.data) return;
    if (e.ctrlKey || e.metaKey) {
      const key = e.key.toLowerCase();
      if (key === "z" && !e.shiftKey) undo();
      else if (key === "y" || (key === "z" && e.shiftKey)) redo();
      else if (key === "s") saveDraft();
      else return;
      e.preventDefault();
      return;
    }
    if (e.altKey) return;
    const step = e.shiftKey ? 1 : 1 / 30;
    if (e.code === "Space") {
      e.preventDefault();
      ed.toggle();
    } else if (e.key === "i") ed.setStart(ed.current);
    else if (e.key === "o") ed.setEnd(ed.current);
    else if (e.key === "s") ed.splitHere();
    else if (e.key === "m") ed.toggleMarker();
    else if (e.key === ",") ed.jumpMarker(-1);
    else if (e.key === ".") ed.jumpMarker(1);
    else if (e.key === "Delete" || e.key === "Backspace") togglePart();
    else if (e.key === "ArrowLeft") ed.seek(ed.current - step);
    else if (e.key === "ArrowRight") ed.seek(ed.current + step);
    else if (e.key === "Escape") back();
  }

  function back() {
    ed.pause();
    store.go(store.lastLibrary);
  }

  async function saveDraft() {
    await api.saveDraft(id, ed.title.trim() || t("Untitled clip"), ed.draft());
    store.notify(t("Saved as draft"), t("Find it under Drafts."));
  }

  async function exportAs(preset: Preset) {
    ed.pause();
    progress = 0;
    try {
      ticket = Date.now() * 1000 + Math.floor(Math.random() * 1000);
      const out = await api.exportClip(id, ed.title.trim() || t("Clip"), { ...ed.draft(), preset }, ticket);
      const done = {
        original: t("Saved as MP4."),
        discord: t("Small enough for Discord."),
        gif: t("Saved as GIF."),
        tracks: t("Saved as MKV with every track on its own."),
      }[preset];
      store.notify(t("Export finished"), ed.vertical ? t("{done} Vertical 9:16.", { done }) : done, [
        { label: t("Copy"), run: () => share.copy(out) },
        { label: t("Show in folder"), run: () => api.revealPath(out) },
      ]);
    } catch (e) {
      if (String(e) !== "Export cancelled") store.notify(t("Export failed"), String(e));
    } finally {
      progress = null;
    }
  }

  async function saveFrame() {
    await api.saveFrame(id, ed.current);
    store.notify(t("Screenshot saved"), t("It's in your library."));
  }
</script>

<svelte:window onkeydown={onKey} />

<section class="editor">
  <Header {ed} {progress} onback={back} onsave={saveDraft} onexport={exportAs} oncancel={() => api.cancelExport(ticket)} />

  {#if failure}
    <div class="failure">
      <h2>{t("This clip can't be opened")}</h2>
      <p class="meta">{failure}</p>
    </div>
  {:else if ed.data}
    <Player
      bind:this={player}
      bind:viewer
      bind:fullscreen
      {ed}
      onready={() => {
        ed.seek(resumeAt ?? ed.trimStart);
        resumeAt = null;
      }}
    />
    <Timeline {ed} playerRoom={() => Math.max(0, (viewer?.clientHeight ?? 0) - 120)}>
      {#snippet toolbar()}
        <Toolbar
          {ed}
          canUndo={history.canUndo || snapshot !== history.committed}
          canRedo={history.canRedo}
          onundo={undo}
          onredo={redo}
          ontogglepart={togglePart}
          onframe={saveFrame}
        />
      {/snippet}
      {#snippet lanes()}
        <TextLane {ed} />
      {/snippet}
    </Timeline>
  {:else}
    <div class="loading"><span class="mono meta">{t("Preparing tracks…")}</span></div>
  {/if}
</section>

<style>
  .editor {
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .failure,
  .loading {
    margin: auto;
    text-align: center;
  }
  .failure h2 {
    font-size: 14px;
  }
</style>
