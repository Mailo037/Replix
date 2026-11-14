import type { Draft, EditorData, Marker, Settings, Speed, TextOverlay, TrackEdit } from "../api";
import { startingEdit } from "../trackDefaults";
import { frameShare, type Crop } from "./crop";
import { drawText } from "./textImage";
import {
  addRange,
  coveredBy,
  joinAt,
  nextSpeed,
  partAt,
  partsOf,
  playLength,
  putBack,
  removeRange,
  speedAt,
  withSpeed,
  withSplit,
  type Part,
  type Range,
} from "./timeline";

/** A marker this close to the playhead counts as on it. */
export const NEAR = 0.25;

/** How far, in seconds, a track may drift from the picture before it is put back in step. */
const DRIFT = 0.2;
/**
 * How long a track that was just put back in step is left alone, in milliseconds: its own
 * seek takes a moment to land, and correcting it again meanwhile would only restart it.
 */
const SETTLE = 600;
/** `HTMLMediaElement.HAVE_FUTURE_DATA`: the element can play on from where it is. */
const HAVE_FUTURE_DATA = 3;
/** How loud the preview plays, as last set; kept for the next clip. */
const LISTEN_KEY = "replix.editor.listen";

function readListen(): { volume: number; muted: boolean } {
  try {
    const saved = JSON.parse(localStorage.getItem(LISTEN_KEY) ?? "null");
    if (saved && typeof saved.volume === "number") return { volume: Math.min(1, Math.max(0, saved.volume)), muted: !!saved.muted };
  } catch {
    // Storage may be off, or hold something else; full volume works as well.
  }
  return { volume: 1, muted: false };
}

/**
 * One clip open in the editor: what is being changed, where the playhead is, and the player
 * that shows it. The editor's parts share one of these; what it holds is reactive.
 */
export class EditorSession {
  /** The library item being edited. */
  readonly id: string;
  data = $state<EditorData | null>(null);
  title = $state("");
  trimStart = $state(0);
  trimEnd = $state(0);
  edits = $state<TrackEdit[]>([]);
  /** Where the clip is split; the parts between can be removed one by one. */
  splits = $state<number[]>([]);
  /** Removed ranges. */
  cuts = $state<Range[]>([]);
  vertical = $state(false);
  /** Where the 9:16 frame sits and how large it is. */
  crop = $state<Crop>({ x: 0.5, y: 0.5, scale: 1 });
  /** All sound together, 0 to 2. */
  masterVolume = $state(1);
  /** Moments marked while recording or here. Saved with the clip as they change, not with a draft. */
  markers = $state<Marker[]>([]);
  /** Parts that play slower. */
  speeds = $state<Speed[]>([]);
  /** Words over the picture. */
  texts = $state<TextOverlay[]>([]);
  /** The text being worked on: moved in the picture and edited in its panel. */
  selectedText = $state<number | null>(null);

  current = $state(0);
  playing = $state(false);
  /**
   * How loud the preview plays, 0 to 1, and whether it is silent: the player's own volume,
   * as in any video player. Not part of the edit; the export keeps its own sound.
   */
  listenVolume = $state(readListen().volume);
  listenMuted = $state(readListen().muted);
  /** What the preview's sound is multiplied by. */
  hearing = $derived(this.listenMuted ? 0 : this.listenVolume);
  video = $state<HTMLVideoElement>();
  audios = $state<HTMLAudioElement[]>([]);

  duration = $derived(this.data?.duration ?? 0);
  parts = $derived(partsOf(this.trimStart, this.trimEnd, this.splits, this.cuts));
  kept = $derived(this.parts.filter((p) => !p.removed));
  /** How long the export will be; slow parts take longer. */
  length = $derived(playLength(this.parts, this.speeds));
  /** How fast the preview plays where the playhead is. */
  rate = $derived(speedAt(this.speeds, this.current));
  playStart = $derived(this.kept[0]?.start ?? this.trimStart);
  playEnd = $derived(this.kept[this.kept.length - 1]?.end ?? this.trimEnd);
  /** The part under the playhead. */
  here = $derived(partAt(this.parts, this.current));
  /** Share of the picture's width the tallest 9:16 frame takes. */
  cropShare = $derived(this.data ? frameShare(this.data.width, this.data.height) : 0.32);

  onMarker = $derived(this.markers.some((m) => Math.abs(m.at - this.current) < NEAR));
  hasNextMarker = $derived(this.markers.some((m) => m.at > this.current + 0.05));
  hasPrevMarker = $derived(this.markers.some((m) => m.at < this.current - 0.05));
  removedTracks = $derived(this.edits.filter((e) => e.removed).length);

  constructor(id: string) {
    this.id = id;
  }

  /** Sets how loud the preview plays; turning it up from silence unmutes it. */
  setListenVolume(volume: number) {
    this.listenVolume = Math.min(1, Math.max(0, Math.round(volume * 100) / 100));
    this.listenMuted = this.listenVolume === 0;
    this.keepListen();
  }

  /** Silences the preview, or brings its sound back. */
  toggleListen() {
    this.listenMuted = !this.listenMuted;
    // Unmuting at no volume would stay silent.
    if (!this.listenMuted && this.listenVolume === 0) this.listenVolume = 1;
    this.keepListen();
  }

  private keepListen() {
    try {
      localStorage.setItem(LISTEN_KEY, JSON.stringify({ volume: this.listenVolume, muted: this.listenMuted }));
    } catch {
      // Not kept; it still applies while the editor is open.
    }
  }

  /** A time as a share of the clip, in percent, for placing things on the timeline. */
  pct = (t: number) => (this.duration > 0 ? (t / this.duration) * 100 : 0);

  /** Starts editing a clip: from its draft, or else as the settings say tracks start. */
  load(d: EditorData, settings: Settings | null) {
    this.data = d;
    this.title = d.title;
    this.markers = d.markers ?? [];
    this.apply({
      trimStart: d.draft?.trimStart ?? 0,
      trimEnd: d.draft?.trimEnd ?? d.duration,
      tracks: d.tracks.map((track, i) => d.draft?.tracks[i] ?? startingEdit(settings, track)),
      splits: d.draft?.splits ?? [],
      cuts: d.draft?.cuts ?? [],
      vertical: d.draft?.vertical ?? false,
      cropX: d.draft?.cropX ?? 0.5,
      cropY: d.draft?.cropY,
      cropScale: d.draft?.cropScale,
      masterVolume: d.draft?.masterVolume,
      speeds: d.draft?.speeds,
      texts: d.draft?.texts,
    });
    this.selectedText = null;
  }

  /** The edit as a draft, to save or export. */
  draft(): Draft {
    return {
      trimStart: this.trimStart,
      trimEnd: this.trimEnd,
      tracks: $state.snapshot(this.edits) as TrackEdit[],
      splits: $state.snapshot(this.splits),
      cuts: $state.snapshot(this.cuts) as Range[],
      vertical: this.vertical,
      cropX: this.crop.x,
      cropY: this.crop.y,
      cropScale: this.crop.scale,
      masterVolume: this.masterVolume,
      speeds: $state.snapshot(this.speeds),
      texts: $state.snapshot(this.texts),
    };
  }

  /** Puts a draft in place, as when undoing. */
  apply(d: Draft) {
    this.trimStart = d.trimStart;
    this.trimEnd = d.trimEnd;
    this.edits = d.tracks;
    this.splits = d.splits;
    this.cuts = d.cuts;
    this.vertical = d.vertical;
    this.crop = { x: d.cropX, y: d.cropY ?? 0.5, scale: d.cropScale ?? 1 };
    this.masterVolume = d.masterVolume ?? 1;
    this.speeds = d.speeds ?? [];
    this.texts = d.texts ?? [];
    if (this.selectedText !== null && !this.texts[this.selectedText]) this.selectedText = null;
  }

  seek(t: number) {
    if (!this.video || !this.data) return;
    this.video.currentTime = Math.min(Math.max(0, t), this.data.duration);
    this.current = this.video.currentTime;
    this.syncAudio(true);
  }

  /**
   * The tracks are held still because the picture is: it is seeking or waiting for data.
   * Left running, they would be pulled back to it again and again, and the same moment
   * would be heard over and over.
   */
  private held = false;
  /** When each track was last put back in step, as `performance.now()`. */
  private nudged: number[] = [];

  /**
   * Each track plays from its own element; this keeps them with the picture. `force` puts
   * every track where the picture is (after a seek); otherwise only a track that drifted
   * away is moved, and none that is still landing from the last move.
   */
  syncAudio(force: boolean) {
    const video = this.video;
    if (!video) return;
    const now = performance.now();
    this.audios.forEach((a, i) => {
      if (!a) return;
      if (!force) {
        if (!this.playing || this.held || a.seeking || now - (this.nudged[i] ?? -Infinity) < SETTLE) return;
        if (Math.abs(a.currentTime - video.currentTime) <= DRIFT) return;
      }
      a.currentTime = video.currentTime;
      this.nudged[i] = now;
    });
  }

  /**
   * Called on every frame while playing. While the picture stands still (seeking after a
   * jump over a removed part, or waiting for the disk) the tracks wait with it, and they
   * start again together with it.
   */
  follow() {
    const video = this.video;
    if (!video || !this.playing) return;
    const still = video.seeking || video.paused || video.readyState < HAVE_FUTURE_DATA;
    if (still) {
      if (!this.held) {
        this.held = true;
        for (const a of this.audios) a?.pause();
      }
      return;
    }
    if (this.held) {
      this.held = false;
      this.syncAudio(true);
      for (const a of this.audios) a?.play().catch(() => {});
      return;
    }
    this.syncAudio(false);
  }

  async play() {
    const video = this.video;
    if (!video) return;
    if (video.currentTime < this.playStart || video.currentTime >= this.playEnd - 0.05) video.currentTime = this.playStart;
    // The tracks start with the picture, once it is ready to play on; `follow` lets them go.
    this.held = true;
    for (const a of this.audios) a?.pause();
    this.syncAudio(true);
    this.playing = true;
    await video.play().catch(() => {});
    this.follow();
  }

  pause() {
    this.playing = false;
    this.held = false;
    if (this.video) {
      this.video.pause();
      this.current = this.video.currentTime;
    }
    for (const a of this.audios) a?.pause();
  }

  toggle() {
    if (this.playing) this.pause();
    else this.play();
  }

  /** Sets the start, at least half a second before the end. */
  setStart(t: number) {
    this.trimStart = Math.min(t, this.trimEnd - 0.5);
  }

  setEnd(t: number) {
    this.trimEnd = Math.max(t, this.trimStart + 0.5);
  }

  /** Splits the clip at `at`, the playhead unless said otherwise. */
  splitHere(at = this.current) {
    const next = withSplit(this.splits, at, this.trimStart, this.trimEnd);
    if (next) this.splits = next;
  }

  /** Whether the clip can be split at `at`: not at an edge or next to another split. */
  canSplit(at: number) {
    return withSplit(this.splits, at, this.trimStart, this.trimEnd) !== null;
  }

  /** The part at `at`, if it is between the start and the end. */
  partAt(at: number): Part | null {
    return partAt(this.parts, at);
  }

  /**
   * Takes the part under the playhead (or at `at`) out, or puts it back. Says what happened;
   * "last" when it is the only part left, which stays.
   */
  togglePart(at = this.current): "removed" | "restored" | "last" | null {
    const part = partAt(this.parts, at);
    if (!part) return null;
    if (part.removed) {
      this.cuts = putBack(this.cuts, part);
      return "restored";
    }
    if (this.kept.length > 1) {
      this.cuts = [...this.cuts, [part.start, part.end]];
      return "removed";
    }
    return "last";
  }

  /** Plays the part under the playhead (or at `at`) at the next speed: normal, half, a quarter. */
  cycleSpeed(at = this.current) {
    const part = partAt(this.parts, at);
    if (!part || part.removed) return;
    this.speeds = withSpeed(this.speeds, part, nextSpeed(speedAt(this.speeds, (part.start + part.end) / 2)));
  }

  /** Puts words at the playhead (or at `at`) for three seconds, low in the middle, and selects them. */
  async addText(text: string, at = this.current) {
    const start = Math.min(at, Math.max(0, this.duration - 0.5));
    const size = 0.08;
    const drawn = await drawText(text);
    this.texts = [
      ...this.texts,
      { text, x: 0.5, y: 0.8, size, height: size * drawn.ratio, start, end: Math.min(this.duration, start + 3), png: drawn.png },
    ];
    this.selectedText = this.texts.length - 1;
  }

  /** Changes the words of a text and draws them again. */
  async setText(i: number, text: string) {
    const drawn = await drawText(text);
    const t = this.texts[i];
    // Gone, or changed again while this drawing was made.
    if (!t || t.text !== text) return;
    t.png = drawn.png;
    t.height = t.size * drawn.ratio;
  }

  /** Sets a text's letter size; the drawing scales with it. */
  setTextSize(i: number, size: number) {
    const t = this.texts[i];
    if (!t) return;
    t.height = (t.height / t.size) * size;
    t.size = size;
  }

  removeText(i: number) {
    this.texts = this.texts.filter((_, k) => k !== i);
    this.selectedText = null;
  }

  /** Takes away the split at `at`. */
  join(at: number) {
    const next = joinAt(this.parts, this.splits, this.cuts, at);
    this.splits = next.splits;
    this.cuts = next.cuts;
  }

  /** Sets a marker at the playhead (or at `at`), or takes away the one there. */
  toggleMarker(at = this.current) {
    const on = this.markerNear(at);
    if (on) this.removeMarker(on.at);
    else this.markers = [...this.markers, { at }].sort((a, b) => a.at - b.at);
  }

  /** The marker at `at`, give or take a little. */
  markerNear(at: number): Marker | undefined {
    return this.markers.find((m) => Math.abs(m.at - at) < NEAR);
  }

  /** Whether track `i` is silent through the whole of `part`. */
  trackSilentIn(i: number, part: { start: number; end: number }) {
    return coveredBy(this.edits[i]?.silent, [part.start, part.end]);
  }

  /** Silences track `i` in `part` while the other tracks play on, or lets it be heard there again. */
  silenceTrack(i: number, part: { start: number; end: number }, silent: boolean) {
    const edit = this.edits[i];
    if (!edit) return;
    const range: Range = [part.start, part.end];
    const next = silent ? addRange(edit.silent ?? [], range) : removeRange(edit.silent ?? [], range);
    if (next.length) edit.silent = next;
    else delete edit.silent;
  }

  /** Where track `i` starts being heard; it keeps at least a fifth of a second. */
  setTrackStart(i: number, at: number) {
    const edit = this.edits[i];
    if (!edit) return;
    const v = Math.min(at, (edit.end ?? this.duration) - 0.2);
    edit.start = v <= 0.05 ? undefined : v;
  }

  setTrackEnd(i: number, at: number) {
    const edit = this.edits[i];
    if (!edit) return;
    const v = Math.max(at, (edit.start ?? 0) + 0.2);
    edit.end = v >= this.duration - 0.05 ? undefined : v;
  }

  removeMarker(at: number) {
    this.markers = this.markers.filter((m) => m.at !== at);
  }

  /** Changes the marker at `at`; its time stays. */
  editMarker(at: number, change: Partial<Marker>) {
    this.markers = this.markers.map((m) => (m.at === at ? { ...m, ...change } : m));
  }

  /** Jumps to the next marker after the playhead, or the one before it. */
  jumpMarker(direction: 1 | -1) {
    const target =
      direction > 0
        ? this.markers.find((m) => m.at > this.current + 0.05)
        : [...this.markers].reverse().find((m) => m.at < this.current - 0.05);
    if (target !== undefined) this.seek(target.at);
  }
}
