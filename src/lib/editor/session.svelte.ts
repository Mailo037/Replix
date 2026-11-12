import type { Draft, EditorData, Marker, Settings, Speed, TextOverlay, TrackEdit } from "../api";
import { startingEdit } from "../trackDefaults";
import { frameShare, type Crop } from "./crop";
import { drawText } from "./textImage";
import { joinAt, nextSpeed, partAt, partsOf, playLength, putBack, speedAt, withSpeed, withSplit, type Range } from "./timeline";

/** A marker this close to the playhead counts as on it. */
export const NEAR = 0.25;

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

  /** Each track plays from its own element; this keeps them with the picture. */
  syncAudio(force: boolean) {
    const video = this.video;
    if (!video) return;
    for (const a of this.audios) {
      if (a && (force || (this.playing && Math.abs(a.currentTime - video.currentTime) > 0.12))) a.currentTime = video.currentTime;
    }
  }

  async play() {
    const video = this.video;
    if (!video) return;
    if (video.currentTime < this.playStart || video.currentTime >= this.playEnd - 0.05) video.currentTime = this.playStart;
    this.syncAudio(true);
    this.playing = true;
    await Promise.allSettled([video.play(), ...this.audios.filter(Boolean).map((a) => a.play())]);
  }

  pause() {
    this.playing = false;
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

  splitHere() {
    const next = withSplit(this.splits, this.current, this.trimStart, this.trimEnd);
    if (next) this.splits = next;
  }

  /**
   * Takes the part under the playhead out, or puts it back. Says what happened; "last" when
   * it is the only part left, which stays.
   */
  togglePart(): "removed" | "restored" | "last" | null {
    const part = this.here;
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

  /** Plays the part under the playhead at the next speed: normal, half, a quarter. */
  cycleSpeed() {
    const part = this.here;
    if (!part || part.removed) return;
    this.speeds = withSpeed(this.speeds, part, nextSpeed(speedAt(this.speeds, (part.start + part.end) / 2)));
  }

  /** Puts words at the playhead for three seconds, low in the middle, and selects them. */
  async addText(text: string) {
    const start = Math.min(this.current, Math.max(0, this.duration - 0.5));
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

  /** Sets a marker at the playhead, or takes away the one there. */
  toggleMarker() {
    const on = this.markers.find((m) => Math.abs(m.at - this.current) < NEAR);
    if (on) this.removeMarker(on.at);
    else this.markers = [...this.markers, { at: this.current }].sort((a, b) => a.at - b.at);
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
