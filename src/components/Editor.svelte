<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { onDestroy, onMount } from "svelte";
  import { api, fileUrl, type Draft, type EditorData, type Preset, type TrackEdit } from "../lib/api";
  import { position, time } from "../lib/format";
  import { share } from "../lib/share";
  import { store } from "../lib/store.svelte";
  import Icon from "./Icon.svelte";
  import { pop } from "../lib/motion";
  import { tip } from "../lib/tooltip";
  import Segmented from "./Segmented.svelte";
  import Slider from "./Slider.svelte";
  import Waveform from "./Waveform.svelte";
  import { currentRound, endEditorRound } from "../lib/editorRound.svelte";

  let { id }: { id: string } = $props();

  let data = $state<EditorData | null>(null);
  let failure = $state<string | null>(null);
  let title = $state("");
  let trimStart = $state(0);
  let trimEnd = $state(0);
  let edits = $state<TrackEdit[]>([]);
  let current = $state(0);
  let playing = $state(false);
  let progress = $state<number | null>(null);
  let exportMenu = $state(false);

  /** Where the clip is split; the parts between can be removed one by one. */
  let splits = $state<number[]>([]);
  /** Removed ranges, in seconds of the original. */
  let cuts = $state<[number, number][]>([]);
  let vertical = $state(false);
  /** Where the 9:16 frame sits: 0 at the left edge, 1 at the right. */
  let cropX = $state(0.5);

  let video = $state<HTMLVideoElement>();
  let audios: HTMLAudioElement[] = $state([]);
  let strip = $state<HTMLDivElement>();
  let preview = $state<HTMLDivElement>();

  type Part = { start: number; end: number; removed: boolean };
  const EPS = 0.001;
  const covers = ([a, b]: [number, number], p: { start: number; end: number }) => a <= p.start + EPS && b >= p.end - EPS;

  const parts = $derived.by<Part[]>(() => {
    const edges = [trimStart, ...splits.filter((s) => s > trimStart + 0.05 && s < trimEnd - 0.05), trimEnd];
    return edges.slice(0, -1).map((start, i) => {
      const part = { start, end: edges[i + 1] };
      return { ...part, removed: cuts.some((c) => covers(c, part)) };
    });
  });
  const kept = $derived(parts.filter((p) => !p.removed));
  const length = $derived(kept.reduce((sum, p) => sum + p.end - p.start, 0));
  const playStart = $derived(kept[0]?.start ?? trimStart);
  const playEnd = $derived(kept[kept.length - 1]?.end ?? trimEnd);
  const here = $derived(parts.find((p) => current >= p.start - EPS && current < p.end - EPS) ?? null);
  /** Share of the picture's width the 9:16 frame takes. */
  const cropWidth = $derived(data && data.width ? Math.min(1, (data.height * 9) / 16 / data.width) : 0.32);

  const pct = (t: number) => (data && data.duration > 0 ? (t / data.duration) * 100 : 0);

  /** Ruler marks: a round step that gives about ten of them. */
  const ticks = $derived.by(() => {
    if (!data) return [];
    const total = data.duration;
    const step = [0.5, 1, 2, 5, 10, 15, 30, 60, 120, 300, 600].find((s) => total / s <= 10) ?? 1200;
    const out: number[] = [];
    for (let t = 0; t < total - step / 4; t += step) out.push(t);
    return out;
  });
  const mark = (t: number) => `${Math.floor(t / 60)}:${String(Math.floor(t % 60)).padStart(2, "0")}`;

  const icons: Record<string, string> = { game: "game", mic: "mic", app: "app", desktop: "desktop" };

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
      data = d;
      title = d.title;
      trimStart = d.draft?.trimStart ?? 0;
      trimEnd = d.draft?.trimEnd ?? d.duration;
      edits = d.tracks.map((_, i) => d.draft?.tracks[i] ?? { enabled: true, volume: 1 });
      splits = d.draft?.splits ?? [];
      cuts = d.draft?.cuts ?? [];
      vertical = d.draft?.vertical ?? false;
      cropX = d.draft?.cropX ?? 0.5;
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
    if (data && e.payload[0] === id) data.filmstrip = e.payload[1];
  });

  onDestroy(() => {
    // A long session's filmstrip and waveforms take a while; nobody needs them now.
    endEditorRound();
    unlisten.then((f) => f());
    unlistenStrip.then((f) => f());
  });

  $effect(() => {
    if (!playing) return;
    let frame = requestAnimationFrame(tick);

    function tick() {
      if (!playing || !video) return;
      current = video.currentTime;
      // Removed parts are jumped over, as they will be in the export.
      const cut = cuts.find(([a, b]) => current >= a && current < b - 0.02);
      if (cut && cut[1] < playEnd) seek(cut[1]);
      else if (current >= playEnd - 0.02) {
        pause();
        seek(playStart);
        return;
      }
      syncAudio(false);
      frame = requestAnimationFrame(tick);
    }
    return () => cancelAnimationFrame(frame);
  });

  // Hidden in the tray or minimised, nobody is watching: stop decoding video and audio.
  /** The window was hidden since the filmstrip was last asked for. */
  let hiddenSince = false;
  /** This hide already ended the editor's round. */
  let endedForHide = false;

  $effect(() => {
    if (!store.visible) {
      // Once per hide: pausing changes `playing`, which runs this again.
      if (!endedForHide) endEditorRound();
      endedForHide = true;
      hiddenSince = true;
      if (playing) pause();
      return;
    }
    endedForHide = false;
    // Hiding ended the round and stopped what it was making; back on screen, ask in the new
    // one. The stop may only be reported after the window is back; this runs again then.
    if (interrupted && store.visible) {
      hiddenSince = false;
      load();
    } else if (hiddenSince && data && !data.filmstrip) {
      hiddenSince = false;
      api.openEditor(id, currentRound()).then((d) => data && !data.filmstrip && (data.filmstrip = d.filmstrip)).catch(() => {});
    }
  });

  $effect(() => {
    audios.forEach((audio, i) => {
      if (!audio) return;
      const edit = edits[i];
      audio.volume = edit?.enabled ? Math.min(1, edit.volume) : 0;
    });
  });

  function syncAudio(force: boolean) {
    if (!video) return;
    audios.forEach((a) => {
      if (!a) return;
      if (force || (playing && Math.abs(a.currentTime - video!.currentTime) > 0.12)) a.currentTime = video!.currentTime;
    });
  }

  async function play() {
    if (!video) return;
    if (video.currentTime < playStart || video.currentTime >= playEnd - 0.05) video.currentTime = playStart;
    syncAudio(true);
    playing = true;
    await Promise.allSettled([video.play(), ...audios.filter(Boolean).map((a) => a.play())]);
  }

  function pause() {
    playing = false;
    if (video) {
      video.pause();
      current = video.currentTime;
    }
    audios.forEach((a) => a?.pause());
  }

  function seek(t: number) {
    if (!video || !data) return;
    video.currentTime = Math.min(Math.max(0, t), data.duration);
    current = video.currentTime;
    syncAudio(true);
  }

  type Drag = "start" | "end" | "head";

  function timeAt(clientX: number): number {
    if (!strip || !data) return 0;
    const rect = strip.getBoundingClientRect();
    return Math.min(Math.max(0, ((clientX - rect.left) / rect.width) * data.duration), data.duration);
  }

  function drag(kind: Drag, e: PointerEvent) {
    e.preventDefault();
    e.stopPropagation();
    const target = e.currentTarget as HTMLElement;
    target.setPointerCapture(e.pointerId);
    const move = (ev: PointerEvent) => {
      const t = timeAt(ev.clientX);
      if (kind === "start") {
        trimStart = Math.min(t, trimEnd - 0.5);
        seek(trimStart);
      } else if (kind === "end") {
        trimEnd = Math.max(t, trimStart + 0.5);
        seek(trimEnd);
      } else {
        seek(t);
      }
    };
    move(e);
    const up = () => {
      target.removeEventListener("pointermove", move);
      target.removeEventListener("pointerup", up);
    };
    target.addEventListener("pointermove", move);
    target.addEventListener("pointerup", up);
  }

  function splitHere() {
    if (current <= trimStart + 0.2 || current >= trimEnd - 0.2) return;
    if (splits.some((s) => Math.abs(s - current) < 0.2)) return;
    splits = [...splits, current].sort((a, b) => a - b);
  }

  /** Takes the part under the playhead out, or puts it back. */
  function togglePart() {
    const part = here;
    if (!part) return;
    if (part.removed) {
      // A cut can span several parts; give back only this one.
      cuts = cuts.flatMap(([a, b]): [number, number][] =>
        covers([a, b], part) ? ([[a, part.start], [part.end, b]] as [number, number][]).filter(([x, y]) => y - x > EPS) : [[a, b]],
      );
    } else if (kept.length > 1) {
      cuts = [...cuts, [part.start, part.end]];
    } else {
      store.notify("That's the last part", "A clip needs something left in it.");
    }
  }

  function join(at: number) {
    const i = parts.findIndex((p) => Math.abs(p.end - at) < EPS);
    const [left, right] = [parts[i], parts[i + 1]];
    splits = splits.filter((s) => s !== at);
    // Joining a removed and a kept part keeps the whole.
    if (left && right && !(left.removed && right.removed)) {
      cuts = cuts.filter(([a, b]) => b <= left.start + EPS || a >= right.end - EPS);
    }
  }

  function dragCrop(e: PointerEvent) {
    if (!preview) return;
    e.preventDefault();
    const target = e.currentTarget as HTMLElement;
    target.setPointerCapture(e.pointerId);
    const width = preview.getBoundingClientRect().width * (1 - cropWidth);
    let lastX = e.clientX;
    const move = (ev: PointerEvent) => {
      if (width > 0) cropX = Math.min(1, Math.max(0, cropX + (ev.clientX - lastX) / width));
      lastX = ev.clientX;
    };
    const up = () => {
      target.removeEventListener("pointermove", move);
      target.removeEventListener("pointerup", up);
    };
    target.addEventListener("pointermove", move);
    target.addEventListener("pointerup", up);
  }

  // Undo history. Each entry is the edit as it was before a change; a quick run of changes
  // (dragging a handle, moving a slider) settles into one entry.
  let undoStack = $state<string[]>([]);
  let redoStack = $state<string[]>([]);
  let committed = $state("");
  let settle = 0;
  const snapshot = $derived(data ? JSON.stringify(draft()) : "");

  $effect(() => {
    const now = snapshot;
    if (!now || now === committed) return;
    if (!committed) committed = now;
    else {
      clearTimeout(settle);
      settle = window.setTimeout(commit, 400);
    }
  });

  function commit() {
    clearTimeout(settle);
    if (!snapshot || snapshot === committed) return;
    undoStack.push(committed);
    if (undoStack.length > 100) undoStack.shift();
    redoStack = [];
    committed = snapshot;
  }

  function restore(json: string) {
    const d = JSON.parse(json) as Draft;
    trimStart = d.trimStart;
    trimEnd = d.trimEnd;
    edits = d.tracks;
    splits = d.splits;
    cuts = d.cuts;
    vertical = d.vertical;
    cropX = d.cropX;
    committed = json;
    if (current < trimStart || current > trimEnd) seek(trimStart);
  }

  function undo() {
    commit();
    const prev = undoStack.pop();
    if (prev === undefined) return;
    redoStack.push(committed);
    restore(prev);
  }

  function redo() {
    commit();
    const next = redoStack.pop();
    if (next === undefined) return;
    undoStack.push(committed);
    restore(next);
  }

  function onKey(e: KeyboardEvent) {
    // Text fields keep their own undo; sliders don't.
    const target = e.target;
    if ((target instanceof Element && target.matches("input:not([type=range]), textarea")) || !data) return;
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
      playing ? pause() : play();
    } else if (e.key === "i") trimStart = Math.min(current, trimEnd - 0.5);
    else if (e.key === "o") trimEnd = Math.max(current, trimStart + 0.5);
    else if (e.key === "s") splitHere();
    else if (e.key === "Delete" || e.key === "Backspace") togglePart();
    else if (e.key === "ArrowLeft") seek(current - step);
    else if (e.key === "ArrowRight") seek(current + step);
    else if (e.key === "Escape") back();
  }

  function back() {
    pause();
    store.go({ name: "library", filter: "all", game: null });
  }

  function draft(): Draft {
    return {
      trimStart,
      trimEnd,
      tracks: $state.snapshot(edits) as TrackEdit[],
      splits: $state.snapshot(splits),
      cuts: $state.snapshot(cuts) as [number, number][],
      vertical,
      cropX,
    };
  }

  async function saveDraft() {
    await api.saveDraft(id, title.trim() || "Untitled clip", draft());
    store.notify("Saved as draft", "Find it under Drafts.");
  }

  async function exportAs(preset: Preset) {
    exportMenu = false;
    pause();
    progress = 0;
    try {
      ticket = Date.now() * 1000 + Math.floor(Math.random() * 1000);
      const out = await api.exportClip(id, title.trim() || "Clip", { ...draft(), preset }, ticket);
      const done = { original: "Saved as MP4.", discord: "Small enough for Discord.", gif: "Saved as GIF." }[preset];
      store.notify("Export finished", vertical ? `${done} Vertical 9:16.` : done, [
        { label: "Copy", run: () => share.copy(out) },
        { label: "Show in folder", run: () => api.revealPath(out) },
      ]);
    } catch (e) {
      if (String(e) !== "Export cancelled") store.notify("Export failed", String(e));
    } finally {
      progress = null;
    }
  }

  async function saveFrame() {
    await api.saveFrame(id, current);
    store.notify("Screenshot saved", "It's in your library.");
  }
</script>

<svelte:window
  onkeydown={onKey}
  onclick={(e) => exportMenu && !(e.target as HTMLElement).closest(".split") && (exportMenu = false)}
/>

<section class="editor">
  <header class="top">
    <button type="button" class="icon-btn back" aria-label="Back to clips" use:tip={"Back to clips · Esc"} onclick={back}>
      <Icon name="back" size={16} />
    </button>
    {#if data}
      <div class="heading">
        <label class="visually-hidden" for="clip-title">Title</label>
        <input id="clip-title" class="title" bind:value={title} spellcheck="false" />
        <span class="meta">
          {data.game ? `${data.game} · ` : ""}{time(data.created)} · {data.width}×{data.height}{data.draft ? " · Draft" : ""}
        </span>
      </div>
      <div class="actions">
        {#if progress !== null}
          <button type="button" class="btn small ghost" onclick={() => api.cancelExport(ticket)}>Cancel export</button>
        {/if}
        <button type="button" class="btn small ghost" onclick={saveDraft} use:tip={"Ctrl + S"}>Save draft</button>
        <div class="split">
          <button type="button" class="btn small primary" disabled={progress !== null} onclick={() => exportAs("original")}>
            {progress !== null ? `Exporting ${Math.round(progress * 100)}%` : "Export"}
          </button>
          <button
            type="button"
            class="btn small primary caret"
            aria-label="More export options"
            use:tip={"More formats"}
            aria-expanded={exportMenu}
            disabled={progress !== null}
            onclick={() => (exportMenu = !exportMenu)}
          >
            <Icon name="chevronDown" size={16} />
          </button>
          {#if exportMenu}
            <div class="menu" role="menu" transition:pop>
              <button type="button" role="menuitem" onclick={() => exportAs("original")}>
                <span>Full quality</span><span class="meta">MP4, as recorded</span>
              </button>
              <button type="button" role="menuitem" onclick={() => exportAs("discord")}>
                <span>For Discord</span><span class="meta">Under 10 MB</span>
              </button>
              <button type="button" role="menuitem" onclick={() => exportAs("gif")}>
                <span>GIF</span>
                <span class="meta">{length > 20 ? "Long GIFs get big. Under 15 s works best." : "Silent, loops, 15 fps"}</span>
              </button>
            </div>
          {/if}
        </div>
      </div>
    {/if}
  </header>

  {#if failure}
    <div class="failure">
      <h2>This clip can't be opened</h2>
      <p class="meta">{failure}</p>
    </div>
  {:else if data}
    <div class="viewer">
      <div class="preview" bind:this={preview} style:--ratio={data.width && data.height ? data.width / data.height : 16 / 9}>
        <!-- svelte-ignore a11y_media_has_caption -->
        <video
          bind:this={video}
          src={fileUrl(data.path)}
          muted
          preload="auto"
          onloadedmetadata={() => seek(trimStart)}
          onseeked={(e) => { if (!playing) current = e.currentTarget.currentTime; }}
          onclick={() => (playing ? pause() : play())}
        ></video>
        {#each data.tracks as track, i (i)}
          <audio bind:this={audios[i]} src={fileUrl(track.audio)} preload="auto"></audio>
        {/each}
        {#if vertical}
          <span class="crop-mask" style:left="0" style:width="{(1 - cropWidth) * cropX * 100}%"></span>
          <span class="crop-mask" style:right="0" style:width="{(1 - cropWidth) * (1 - cropX) * 100}%"></span>
          <button
            type="button"
            class="crop"
            style:left="{(1 - cropWidth) * cropX * 100}%"
            style:width="{cropWidth * 100}%"
            aria-label="9:16 frame. Drag or use the arrow keys to move it"
            onpointerdown={dragCrop}
            onkeydown={(e) => {
              if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
                e.preventDefault();
                e.stopPropagation();
                cropX = Math.min(1, Math.max(0, cropX + (e.key === "ArrowLeft" ? -0.05 : 0.05)));
              }
            }}
          >
            <span class="crop-label mono">9:16</span>
          </button>
        {/if}
        {#if !playing}
          <button type="button" class="play" aria-label="Play" use:tip={"Play · Space"} onclick={play}><Icon name="play" size={20} /></button>
        {/if}
      </div>
    </div>

    <div class="timeline">
      <div class="toolbar">
        <button
          type="button"
          class="tool icon"
          aria-label={playing ? "Pause" : "Play"}
          use:tip={playing ? "Pause · Space" : "Play · Space"}
          onclick={() => (playing ? pause() : play())}
        >
          <Icon name={playing ? "pause" : "play"} size={16} />
        </button>
        <span class="clock mono"><span class="ink">{position(current)}</span> / {position(data.duration)}</span>
        <span class="sep"></span>
        <button type="button" class="tool" use:tip={"Split at the playhead · S"} onclick={splitHere}>
          <Icon name="scissors" size={16} />Split
        </button>
        <button
          type="button"
          class="tool"
          disabled={!here || parts.length < 2}
          use:tip={here?.removed ? "Put this part back · Del" : "Remove the part under the playhead · Del"}
          onclick={togglePart}
        >
          {#if here?.removed}
            <Icon name="restore" size={16} />Put back
          {:else}
            <Icon name="trash" size={16} />Remove
          {/if}
        </button>
        <span class="sep"></span>
        <button type="button" class="tool icon" aria-label="Undo" use:tip={"Undo · Ctrl + Z"} disabled={!undoStack.length && snapshot === committed} onclick={undo}>
          <Icon name="undo" size={16} />
        </button>
        <button type="button" class="tool icon" aria-label="Redo" use:tip={"Redo · Ctrl + Y"} disabled={!redoStack.length} onclick={redo}>
          <Icon name="redo" size={16} />
        </button>
        <span class="spacer"></span>
        <span class="length mono">
          {parts.length > 1 ? `${kept.length} of ${parts.length} parts · ` : ""}<span class="ink">{position(length)}</span> long
        </span>
        <div class="format">
          <Segmented
            label="Format"
            track="var(--black)"
            value={vertical ? "tall" : "wide"}
            options={[
              { value: "wide", label: "16:9" },
              { value: "tall", label: "9:16" },
            ]}
            onchange={(v) => (vertical = v === "tall")}
          />
        </div>
        <button type="button" class="tool icon" aria-label="Save this frame as a screenshot" use:tip={"Save this frame as a screenshot"} onclick={saveFrame}>
          <Icon name="camera" size={16} />
        </button>
      </div>

      <div class="tracks">
        <div class="row ruler-row">
          <span class="head"></span>
          <div class="lane ruler" role="presentation" onpointerdown={(e) => drag("head", e)}>
            {#each ticks as t (t)}
              <span class="tick" style:left="{pct(t)}%"><span class="mono">{mark(t)}</span></span>
            {/each}
          </div>
        </div>

        <div class="row">
          <div class="head">
            <span class="kind"><Icon name="clips" size={16} /></span>
            <span class="name">Video</span>
          </div>
          <div
            class="lane strip"
            bind:this={strip}
            role="slider"
            tabindex="0"
            aria-label="Position"
            aria-valuemin={0}
            aria-valuemax={data.duration}
            aria-valuenow={current}
            onpointerdown={(e) => drag("head", e)}
            style:background-image={data.filmstrip ? `url("${fileUrl(data.filmstrip)}")` : undefined}
          >
            <span class="range" style:left="{pct(trimStart)}%" style:width="{pct(trimEnd) - pct(trimStart)}%"></span>
            {#each splits.filter((s) => s > trimStart && s < trimEnd) as s (s)}
              <span class="split-mark" style:left="{pct(s)}%"></span>
              <button
                type="button"
                class="join"
                style:left="{pct(s)}%"
                aria-label="Join the parts at {position(s)}"
                use:tip={"Join the parts"}
                onpointerdown={(e) => e.stopPropagation()}
                onclick={() => join(s)}
              >
                <Icon name="close" size={10} />
              </button>
            {/each}
            <button
              type="button"
              class="handle"
              style:left="{pct(trimStart)}%"
              aria-label="Start of the clip"
              use:tip={"Drag to set the start · I"}
              onpointerdown={(e) => drag("start", e)}
            ></button>
            <button
              type="button"
              class="handle end"
              style:left="{pct(trimEnd)}%"
              aria-label="End of the clip"
              use:tip={"Drag to set the end · O"}
              onpointerdown={(e) => drag("end", e)}
            ></button>
          </div>
        </div>

        {#each data.tracks as track, i (i)}
          {#if edits[i]}
            <div class="row" class:off={!edits[i].enabled}>
              <div class="head">
                <button
                  type="button"
                  class="kind mute"
                  aria-pressed={!edits[i].enabled}
                  aria-label={edits[i].enabled ? `Mute ${track.name}` : `Unmute ${track.name}`}
                  use:tip={edits[i].enabled ? "Mute" : "Unmute"}
                  onclick={() => (edits[i].enabled = !edits[i].enabled)}
                >
                  <Icon name={edits[i].enabled ? (icons[track.kind] ?? "speaker") : "mute"} size={16} />
                </button>
                <div class="head-body">
                  <span class="name">{track.name}</span>
                  <Slider
                    label="{track.name} volume"
                    value={Math.round(edits[i].volume * 100)}
                    valuetext="{Math.round(edits[i].volume * 100)}%"
                    disabled={!edits[i].enabled}
                    oninput={(v) => (edits[i].volume = v / 100)}
                  />
                </div>
              </div>
              <div class="lane audio-lane" role="presentation" onpointerdown={(e) => drag("head", e)}>
                <Waveform {id} track={i} duration={data.duration} muted={!edits[i].enabled} />
              </div>
            </div>
          {/if}
        {/each}

        <div class="overlay" aria-hidden="true">
          <span class="dim" style:left="0" style:width="{pct(trimStart)}%"></span>
          <span class="dim" style:left="{pct(trimEnd)}%" style:right="0"></span>
          {#each parts.filter((p) => p.removed) as p (p.start)}
            <span class="cut" style:left="{pct(p.start)}%" style:width="{pct(p.end) - pct(p.start)}%"></span>
          {/each}
          <span class="playhead" style:left="{pct(current)}%"></span>
        </div>
      </div>
      {#if !data.tracks.length}
        <p class="meta none">This clip has no sound.</p>
      {/if}
    </div>
  {:else}
    <div class="loading"><span class="mono meta">Preparing tracks…</span></div>
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

  /* Title row */
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

  /* Player: as large as the room allows, at the clip's own shape. */
  .viewer {
    flex: 1;
    min-height: 120px;
    container-type: size;
    display: grid;
    place-items: center;
  }
  .preview {
    position: relative;
    width: min(100cqw, 100cqh * var(--ratio));
    aspect-ratio: var(--ratio);
    border-radius: 20px;
    overflow: hidden;
    background: var(--black);
  }
  video {
    width: 100%;
    height: 100%;
    object-fit: contain;
    display: block;
    cursor: pointer;
  }
  .play {
    position: absolute;
    top: 50%;
    left: 50%;
    width: 44px;
    height: 44px;
    margin: -22px 0 0 -22px;
    padding-left: 2px;
    border: 0;
    border-radius: 50%;
    background: rgb(0 0 0 / 0.5);
    color: #fff;
    backdrop-filter: blur(8px);
    display: grid;
    place-items: center;
    transition: transform 160ms var(--travel), background-color 160ms ease;
  }
  .play:hover {
    background: rgb(0 0 0 / 0.7);
    transform: scale(1.06);
  }
  .crop-mask {
    position: absolute;
    top: 0;
    bottom: 0;
    background: rgb(10 10 9 / 0.8);
    pointer-events: none;
  }
  .crop {
    position: absolute;
    top: 0;
    bottom: 0;
    padding: 0;
    border: 2px solid var(--accent);
    border-radius: 10px;
    background: transparent;
    cursor: grab;
    touch-action: none;
  }
  .crop:active {
    cursor: grabbing;
  }
  .crop-label {
    position: absolute;
    top: 8px;
    left: 50%;
    transform: translateX(-50%);
    padding: 2px 6px;
    border-radius: 8px;
    background: var(--accent);
    color: var(--on-accent);
    font-size: 11px;
  }

  /* Timeline */
  .timeline {
    flex: none;
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 8px 12px 12px;
    border-radius: 24px;
    background: var(--overlay);
  }
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

  .tracks {
    --head: 168px;
    position: relative;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .row {
    display: grid;
    grid-template-columns: var(--head) 1fr;
    gap: 10px;
    align-items: stretch;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .kind {
    width: 28px;
    height: 28px;
    flex: none;
    display: grid;
    place-items: center;
    border: 0;
    border-radius: 10px;
    background: var(--surface);
    color: var(--accent);
  }
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
  .head-body {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .name {
    font-size: 13px;
    font-weight: 700;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .off .name {
    color: var(--ghost);
  }
  .lane {
    position: relative;
    border-radius: 10px;
    background: var(--black);
    cursor: pointer;
    touch-action: none;
  }
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
  .strip {
    height: 46px;
    background-size: 100% 100%;
  }
  .audio-lane {
    height: 38px;
    overflow: hidden;
  }
  .range {
    position: absolute;
    top: 0;
    bottom: 0;
    border: 2px solid var(--accent);
    border-radius: 10px;
    pointer-events: none;
  }
  .handle {
    position: absolute;
    top: 0;
    bottom: 0;
    z-index: 2;
    width: 9px;
    padding: 0;
    border: 0;
    border-radius: 10px 0 0 10px;
    background: var(--accent);
    cursor: ew-resize;
    touch-action: none;
  }
  .handle.end {
    margin-left: -9px;
    border-radius: 0 10px 10px 0;
  }
  .handle::after {
    content: "";
    position: absolute;
    top: 50%;
    left: 3.5px;
    width: 2px;
    height: 12px;
    margin-top: -6px;
    border-radius: 1px;
    background: var(--on-accent);
    opacity: 0.55;
  }
  .split-mark {
    position: absolute;
    top: 2px;
    bottom: 2px;
    width: 2px;
    margin-left: -1px;
    background: var(--accent);
    pointer-events: none;
  }
  .join {
    position: absolute;
    top: 50%;
    z-index: 3;
    width: 18px;
    height: 18px;
    margin: -9px 0 0 -9px;
    padding: 0;
    display: grid;
    place-items: center;
    border: 0;
    border-radius: 50%;
    background: var(--accent);
    color: var(--on-accent);
    opacity: 0;
    transition: opacity 140ms ease;
  }
  .strip:hover .join,
  .join:focus-visible {
    opacity: 1;
  }

  /* Shading and the playhead run across every lane. */
  .overlay {
    position: absolute;
    top: 0;
    bottom: 0;
    left: calc(var(--head) + 10px);
    right: 0;
    pointer-events: none;
  }
  .dim {
    position: absolute;
    top: 20px;
    bottom: 0;
    background: rgb(10 10 9 / 0.66);
  }
  .cut {
    position: absolute;
    top: 20px;
    bottom: 0;
    background: repeating-linear-gradient(-45deg, rgb(10 10 9 / 0.8) 0 5px, rgb(10 10 9 / 0.55) 5px 10px);
  }
  .playhead {
    position: absolute;
    top: 0;
    bottom: -4px;
    width: 2px;
    margin-left: -1px;
    background: #fff;
    box-shadow: 0 0 0 1px rgb(0 0 0 / 0.35);
  }
  .playhead::before {
    content: "";
    position: absolute;
    top: 0;
    left: -4px;
    width: 10px;
    height: 10px;
    border-radius: 3px 3px 50% 50%;
    background: #fff;
  }
  .none {
    margin: 4px 0 0;
    padding-left: calc(var(--head, 168px) + 10px);
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
