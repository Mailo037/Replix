<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { onDestroy, onMount } from "svelte";
  import { api, fileUrl, type Draft, type EditorData, type Preset, type TrackEdit } from "../lib/api";
  import { position, time } from "../lib/format";
  import { share } from "../lib/share";
  import { store } from "../lib/store.svelte";
  import Icon from "./Icon.svelte";
  import { tip } from "../lib/tooltip";
  import Segmented from "./Segmented.svelte";
  import Slider from "./Slider.svelte";
  import Switch from "./Switch.svelte";

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
  let frame = 0;

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

  const icons: Record<string, string> = { game: "game", mic: "mic", app: "app", desktop: "desktop" };

  onMount(async () => {
    try {
      const d = await api.openEditor(id);
      data = d;
      title = d.title;
      trimStart = d.draft?.trimStart ?? 0;
      trimEnd = d.draft?.trimEnd ?? d.duration;
      edits = d.tracks.map((_, i) => d.draft?.tracks[i] ?? { enabled: true, volume: 1 });
      splits = d.draft?.splits ?? [];
      cuts = d.draft?.cuts ?? [];
      vertical = d.draft?.vertical ?? false;
      cropX = d.draft?.cropX ?? 0.5;
      loop();
    } catch (e) {
      failure = String(e);
    }
  });

  const unlisten = listen<number>("export-progress", (e) => {
    if (progress !== null) progress = e.payload;
  });

  onDestroy(() => {
    cancelAnimationFrame(frame);
    unlisten.then((f) => f());
  });

  function loop() {
    frame = requestAnimationFrame(loop);
    if (!video) return;
    current = video.currentTime;
    if (playing) {
      // Removed parts are jumped over, as they will be in the export.
      const cut = cuts.find(([a, b]) => current >= a && current < b - 0.02);
      if (cut && cut[1] < playEnd) seek(cut[1]);
      else if (current >= playEnd - 0.02) {
        pause();
        seek(playStart);
      }
    }
    syncAudio(false);
  }

  function syncAudio(force: boolean) {
    if (!video) return;
    audios.forEach((a, i) => {
      if (!a) return;
      const e = edits[i];
      a.volume = e && e.enabled ? Math.min(1, e.volume) : 0;
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
    video?.pause();
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
      const out = await api.exportClip(id, title.trim() || "Clip", { ...draft(), preset });
      const done = { original: "Saved as MP4.", discord: "Small enough for Discord.", gif: "Saved as GIF." }[preset];
      store.notify("Export finished", vertical ? `${done} Vertical 9:16.` : done, [
        { label: "Copy", run: () => share.copy(out) },
        { label: "Show in folder", run: () => api.revealPath(out) },
      ]);
    } catch (e) {
      store.notify("Export failed", String(e));
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
  <header>
    <button type="button" class="btn small back" onclick={back}><Icon name="back" />Clips</button>
    {#if data}
      <div class="heading">
        <label class="visually-hidden" for="clip-title">Title</label>
        <input id="clip-title" class="title" bind:value={title} spellcheck="false" />
        <div class="meta">
          {#if data.draft}<span class="badge">[ DRAFT ]</span>{/if}
          {data.game ? `${data.game} · ` : ""}{time(data.created)} · {data.width}×{data.height}
        </div>
      </div>
      <div class="actions">
        <button type="button" class="btn small" onclick={saveDraft} use:tip={"Ctrl + S"}>Save draft</button>
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
            <div class="menu" role="menu">
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
    <div class="workspace">
      <div class="stage">
        <div class="column" style:--ratio={data.width && data.height ? data.width / data.height : 16 / 9}>
        <div class="preview" bind:this={preview}>
          <!-- svelte-ignore a11y_media_has_caption -->
          <video bind:this={video} src={fileUrl(data.path)} muted preload="auto" onloadedmetadata={() => seek(trimStart)} onclick={() => (playing ? pause() : play())}></video>
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
            <button type="button" class="play" aria-label="Play" use:tip={"Play · Space"} onclick={play}><Icon name="play" size={24} /></button>
          {/if}
          <span class="pos badge">{position(current)} / {position(data.duration)}</span>
        </div>

        <div class="transport">
          <button type="button" class="icon-btn" aria-label={playing ? "Pause" : "Play"} use:tip={playing ? "Pause · Space" : "Play · Space"} onclick={() => (playing ? pause() : play())}>
            <Icon name={playing ? "pause" : "play"} size={16} />
          </button>
          <div
            class="strip"
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
            <span class="dim" style:left="0" style:width="{pct(trimStart)}%"></span>
            <span class="dim" style:left="{pct(trimEnd)}%" style:right="0"></span>
            {#each parts.filter((p) => p.removed) as p (p.start)}
              <span class="cut" style:left="{pct(p.start)}%" style:width="{pct(p.end) - pct(p.start)}%"></span>
            {/each}
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
              >×</button>
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
            <span class="head" style:left="{pct(current)}%"></span>
          </div>
        </div>
        <div class="cutbar">
          <div class="history">
            <button type="button" class="tool icon" aria-label="Undo" use:tip={"Undo · Ctrl + Z"} disabled={!undoStack.length && snapshot === committed} onclick={undo}>
              <Icon name="undo" size={16} />
            </button>
            <button type="button" class="tool icon" aria-label="Redo" use:tip={"Redo · Ctrl + Y"} disabled={!redoStack.length} onclick={redo}>
              <Icon name="redo" size={16} />
            </button>
          </div>
          <button type="button" class="tool" onclick={splitHere}>
            <Icon name="scissors" size={16} />Split here<kbd>S</kbd>
          </button>
          <button type="button" class="tool" disabled={!here || parts.length < 2} onclick={togglePart}>
            {#if here?.removed}
              <Icon name="restore" size={16} />Put this part back
            {:else}
              <Icon name="trash" size={16} />Remove this part
            {/if}<kbd>Del</kbd>
          </button>
          <span class="times mono">
            {parts.length > 1 ? `${kept.length} of ${parts.length} parts · ` : ""}<span class="ink">{position(length)}</span>
            of {position(data.duration)}
          </span>
        </div>
        </div>
      </div>

      <aside class="panel audio" aria-labelledby="audio-title">
        <div class="format">
          <span class="format-label">Format</span>
          <Segmented
            label="Format"
            track="var(--canvas)"
            value={vertical ? "tall" : "wide"}
            options={[
              { value: "wide", label: "16:9" },
              { value: "tall", label: "9:16 · TikTok" },
            ]}
            onchange={(v) => (vertical = v === "tall")}
          />
        </div>
        <div class="divider"></div>
        <div class="audio-head">
          <h2 id="audio-title">Audio</h2>
          <p class="meta">Every source is its own track. Turn one off and it's gone from the export.</p>
        </div>
        {#each data.tracks as track, i (i)}
          {#if edits[i]}
            <div class="track" class:off={!edits[i].enabled}>
              <span class="tile"><Icon name={icons[track.kind] ?? "speaker"} /></span>
              <div class="track-body">
                <div class="track-top">
                  <span class="name">{track.name}</span>
                  <Switch checked={edits[i].enabled} label={track.name} onchange={(v) => (edits[i].enabled = v)} />
                </div>
                <div class="level">
                  <Slider
                    label="{track.name} volume"
                    value={Math.round(edits[i].volume * 100)}
                    valuetext="{Math.round(edits[i].volume * 100)}%"
                    disabled={!edits[i].enabled}
                    oninput={(v) => (edits[i].volume = v / 100)}
                  />
                </div>
              </div>
            </div>
          {/if}
        {:else}
          <p class="meta none">This clip has no sound.</p>
        {/each}
        <div class="divider"></div>
        <button type="button" class="text-action" onclick={saveFrame}>
          <Icon name="screenshots" />Save this frame as a screenshot
        </button>
      </aside>
    </div>
  {:else}
    <div class="loading"><span class="mono meta">Preparing tracks…</span></div>
  {/if}
</section>

<style>
  .editor {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 18px;
    min-height: 0;
  }
  header {
    display: flex;
    align-items: center;
    gap: 14px;
    flex-wrap: wrap;
  }
  .back {
    padding-left: 8px;
    gap: 4px;
  }
  .heading {
    flex: 1;
    min-width: 200px;
  }
  .title {
    width: 100%;
    padding: 2px 6px;
    margin-left: -6px;
    border: 0;
    border-radius: 8px;
    background: transparent;
    font-size: 15px;
    font-weight: 600;
    outline: none;
  }
  .title:hover,
  .title:focus {
    background: var(--overlay);
  }
  .heading .meta {
    display: flex;
    gap: 6px;
    align-items: center;
  }
  .heading .badge {
    color: var(--muted);
  }
  .actions {
    display: flex;
    gap: 8px;
  }
  .split {
    position: relative;
    display: flex;
    gap: 2px;
  }
  .split .btn.primary:first-child {
    border-radius: 8px 3px 3px 8px;
    min-width: 84px;
  }
  .caret {
    width: 28px;
    padding: 0;
    border-radius: 3px 8px 8px 3px;
  }
  .menu {
    position: absolute;
    right: 0;
    top: calc(100% + 6px);
    z-index: 20;
    width: 230px;
    padding: 4px;
    border-radius: 14px;
    background: var(--raised);
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
    border-radius: 10px;
    background: transparent;
    text-align: left;
  }
  .menu button:hover {
    background: var(--hover);
  }
  .workspace {
    flex: 1;
    min-height: 0;
    display: flex;
    gap: 20px;
    justify-content: center;
  }
  /* The player takes all the room it can at the clip's own shape; the timeline follows its width. */
  .stage {
    flex: 1;
    min-width: 0;
    min-height: 0;
    container-type: size;
    display: flex;
    flex-direction: column;
    align-items: center;
  }
  .column {
    --below: 124px;
    width: min(100cqw, calc((100cqh - var(--below)) * var(--ratio)));
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .preview {
    position: relative;
    aspect-ratio: var(--ratio);
    border-radius: 16px;
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
    width: 60px;
    height: 60px;
    margin: -30px 0 0 -30px;
    border: 0;
    border-radius: 50%;
    background: var(--accent);
    color: var(--on-accent);
    display: grid;
    place-items: center;
    padding-left: 3px;
    transition: transform 160ms var(--travel);
  }
  .play:hover {
    transform: scale(1.06);
  }
  .play:active {
    transform: scale(0.9);
  }
  .pos {
    position: absolute;
    right: 10px;
    bottom: 10px;
    padding: 3px 8px;
    border-radius: 6px;
    background: rgba(0, 0, 0, 0.65);
    color: #fff;
    font-size: 12px;
  }
  .transport {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .strip {
    position: relative;
    flex: 1;
    height: 60px;
    border-radius: 12px;
    background-color: var(--black);
    background-size: 100% 100%;
    cursor: pointer;
    touch-action: none;
  }
  .dim {
    position: absolute;
    top: 0;
    bottom: 0;
    background: rgba(10, 10, 9, 0.72);
  }
  .dim:first-child {
    border-radius: 12px 0 0 12px;
  }
  .range {
    position: absolute;
    top: 0;
    bottom: 0;
    border: 3px solid var(--accent);
    border-radius: 12px;
    pointer-events: none;
  }
  .handle {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 14px;
    margin-left: 0;
    padding: 0;
    border: 0;
    border-radius: 12px 0 0 12px;
    background: var(--accent);
    cursor: ew-resize;
    touch-action: none;
  }
  .handle.end {
    margin-left: -14px;
    border-radius: 0 12px 12px 0;
  }
  .handle::after {
    content: "";
    position: absolute;
    top: 22px;
    left: 5px;
    width: 4px;
    height: 16px;
    border-radius: 2px;
    background: var(--on-accent);
    opacity: 0.55;
  }
  .head {
    position: absolute;
    top: -4px;
    bottom: -4px;
    width: 3px;
    margin-left: -1.5px;
    border-radius: 2px;
    background: #fff;
    pointer-events: none;
    box-shadow: 0 0 0 1px rgba(0, 0, 0, 0.4);
  }
  .cut {
    position: absolute;
    top: 0;
    bottom: 0;
    background: repeating-linear-gradient(-45deg, rgb(10 10 9 / 0.86) 0 6px, rgb(10 10 9 / 0.6) 6px 12px);
    pointer-events: none;
  }
  .split-mark {
    position: absolute;
    top: 3px;
    bottom: 3px;
    width: 2px;
    margin-left: -1px;
    background: var(--accent);
    pointer-events: none;
  }
  .join {
    position: absolute;
    top: -11px;
    width: 18px;
    height: 18px;
    margin-left: -9px;
    padding: 0;
    border: 0;
    border-radius: 50%;
    background: var(--accent);
    color: var(--on-accent);
    font-size: 12.5px;
    line-height: 18px;
    opacity: 0;
    transition: opacity 140ms ease;
  }
  .strip:hover .join,
  .join:focus-visible {
    opacity: 1;
  }
  .cutbar {
    display: flex;
    align-items: center;
    gap: 6px;
    padding-left: 50px;
  }
  .tool {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 32px;
    padding: 0 10px;
    border: 0;
    border-radius: 10px;
    background: transparent;
    color: var(--muted);
    font-size: 12.5px;
    white-space: nowrap;
  }
  .tool:hover:not(:disabled) {
    background: var(--surface);
    color: var(--ink);
  }
  .tool:disabled {
    opacity: 0.45;
    cursor: default;
  }
  kbd {
    font-family: var(--mono);
    font-size: 11px;
    padding: 1px 5px;
    border-radius: 5px;
    background: var(--surface);
    color: var(--ghost);
  }
  /* Undo and redo sit at the far end, after the length. */
  .history {
    order: 1;
    display: flex;
    gap: 2px;
    padding-left: 6px;
    border-left: 1px solid var(--line);
  }
  .tool.icon {
    width: 32px;
    padding: 0;
    justify-content: center;
  }
  .times {
    margin-left: auto;
    padding-right: 4px;
    font-size: 12px;
    color: var(--ghost);
    white-space: nowrap;
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
    border-radius: 6px;
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
    border-radius: 5px;
    background: var(--accent);
    color: var(--on-accent);
    font-size: 11px;
  }
  .format {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px 12px 6px;
  }
  .format-label {
    font-size: 14px;
    font-weight: 600;
  }
  .ink {
    color: var(--ink);
  }
  .audio {
    width: 300px;
    flex: none;
    align-self: flex-start;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .audio-head {
    padding: 12px 12px 8px;
  }
  .audio-head h2 {
    margin: 0;
    font-size: 14px;
    font-weight: 600;
  }
  .audio-head p {
    margin: 2px 0 0;
  }
  .track {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    padding: 10px 12px;
    border-radius: var(--r-row);
    background: var(--surface);
    transition: background-color 220ms ease;
  }
  .track.off {
    background: transparent;
  }
  .tile {
    width: 32px;
    height: 32px;
    flex: none;
    border-radius: 9px;
    background: var(--raised);
    color: var(--accent);
    display: grid;
    place-items: center;
  }
  .track.off .tile {
    color: var(--ghost);
    background: var(--surface);
  }
  .track-body {
    flex: 1;
    min-width: 0;
  }
  .track-top {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    min-height: 26px;
  }
  .name {
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .track.off .name {
    color: var(--muted);
  }
  .level {
    margin: 6px 0 0;
  }
  .none {
    padding: 0 12px 8px;
  }
  .divider {
    height: 1px;
    margin: 6px 12px;
    background: var(--line);
  }
  .text-action {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 44px;
    padding: 0 12px;
    border: 0;
    border-radius: var(--r-row);
    background: transparent;
    color: var(--accent);
    font-weight: 500;
    text-align: left;
  }
  .text-action:hover {
    background: var(--surface);
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
