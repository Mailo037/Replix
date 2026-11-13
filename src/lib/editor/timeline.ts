// The editor's timeline as plain numbers: where the clip is split, which parts are taken out,
// how long what is left plays. Seconds are always seconds of the original clip.

/** Two times this close are the same place. */
export const EPS = 0.001;

/** A stretch between two splits (or a trim edge); a removed one is left out of the export. */
export interface Part {
  start: number;
  end: number;
  removed: boolean;
}

export type Range = [number, number];

/** Whether the removed range takes in the whole part. */
export const covers = ([a, b]: Range, p: { start: number; end: number }) => a <= p.start + EPS && b >= p.end - EPS;

/** The parts between the trim edges, cut at every split inside them. */
export function partsOf(trimStart: number, trimEnd: number, splits: number[], cuts: Range[]): Part[] {
  const edges = [trimStart, ...splits.filter((s) => s > trimStart + 0.05 && s < trimEnd - 0.05), trimEnd];
  return edges.slice(0, -1).map((start, i) => {
    const part = { start, end: edges[i + 1] };
    return { ...part, removed: cuts.some((c) => covers(c, part)) };
  });
}

/** How long the parts that are kept play together. */
export const keptLength = (parts: Part[]) => parts.reduce((sum, p) => (p.removed ? sum : sum + p.end - p.start), 0);

/** The part under the playhead, if it is between the trim edges. */
export const partAt = (parts: Part[], at: number) => parts.find((p) => at >= p.start - EPS && at < p.end - EPS) ?? null;

/** The splits with one more at `at`, or null where a split makes no sense (too close to an edge or another split). */
export function withSplit(splits: number[], at: number, trimStart: number, trimEnd: number): number[] | null {
  if (at <= trimStart + 0.2 || at >= trimEnd - 0.2) return null;
  if (splits.some((s) => Math.abs(s - at) < 0.2)) return null;
  return [...splits, at].sort((a, b) => a - b);
}

/** The cuts with this part given back. A cut can span several parts; only this one returns. */
export function putBack(cuts: Range[], part: Part): Range[] {
  return cuts.flatMap(([a, b]): Range[] =>
    covers([a, b], part) ? ([[a, part.start], [part.end, b]] as Range[]).filter(([x, y]) => y - x > EPS) : [[a, b]],
  );
}

/**
 * Takes away the split at `at`. Joining a removed and a kept part keeps the whole; two
 * removed parts stay removed.
 */
export function joinAt(parts: Part[], splits: number[], cuts: Range[], at: number): { splits: number[]; cuts: Range[] } {
  const i = parts.findIndex((p) => Math.abs(p.end - at) < EPS);
  const [left, right] = [parts[i], parts[i + 1]];
  const nextSplits = splits.filter((s) => s !== at);
  if (left && right && !(left.removed && right.removed)) {
    return { splits: nextSplits, cuts: cuts.filter(([a, b]) => b <= left.start + EPS || a >= right.end - EPS) };
  }
  return { splits: nextSplits, cuts };
}

/** A slower stretch, in seconds of the original. */
export interface Slow {
  start: number;
  end: number;
  factor: number;
}

/** The slowest a part can play, as the export can keep the pitch down to a quarter. */
export const SLOWEST = 0.25;

/** How fast the clip plays at `t`: 1, or the factor of a slow stretch there. */
export const speedAt = (speeds: Slow[], t: number) =>
  speeds.find((s) => t >= s.start - EPS && t < s.end - EPS)?.factor ?? 1;

/** How long the kept parts play, slow stretches taking longer. */
export function playLength(parts: Part[], speeds: Slow[]): number {
  let total = 0;
  for (const p of parts) {
    if (p.removed) continue;
    const edges = [p.start, ...speeds.flatMap((s) => [s.start, s.end]).filter((t) => t > p.start && t < p.end), p.end].sort((a, b) => a - b);
    for (let i = 0; i + 1 < edges.length; i++) total += (edges[i + 1] - edges[i]) / speedAt(speeds, (edges[i] + edges[i + 1]) / 2);
  }
  return total;
}

/** The speeds with `part` at `factor`: what overlapped it is cut back to its edges; 1 leaves it normal. */
export function withSpeed(speeds: Slow[], part: { start: number; end: number }, factor: number): Slow[] {
  const out: Slow[] = [];
  for (const s of speeds) {
    if (s.end <= part.start + EPS || s.start >= part.end - EPS) out.push(s);
    else {
      if (s.start < part.start - EPS) out.push({ ...s, end: part.start });
      if (s.end > part.end + EPS) out.push({ ...s, start: part.end });
    }
  }
  if (factor < 1) out.push({ start: part.start, end: part.end, factor: Math.max(SLOWEST, factor) });
  return out.sort((a, b) => a.start - b.start);
}

/** The next speed for a part: normal, half, a quarter, then normal again. */
export const nextSpeed = (factor: number) => (factor > 0.5 + EPS ? 0.5 : factor > SLOWEST + EPS ? SLOWEST : 1);

/** A speed as people read it: 1×, ½×, ¼×. */
export const speedLabel = (factor: number) => (factor >= 1 - EPS ? "1×" : Math.abs(factor - 0.5) < EPS ? "½×" : Math.abs(factor - 0.25) < EPS ? "¼×" : `${factor}×`);

/** Ruler marks: a round step that gives about ten of them. */
export function rulerTicks(total: number): number[] {
  if (!(total > 0)) return [];
  const step = [0.5, 1, 2, 5, 10, 15, 30, 60, 120, 300, 600].find((s) => total / s <= 10) ?? 1200;
  const out: number[] = [];
  for (let t = 0; t < total - step / 4; t += step) out.push(t);
  return out;
}

/** A ruler mark's label: minutes and seconds. */
export const rulerLabel = (t: number) => `${Math.floor(t / 60)}:${String(Math.floor(t % 60)).padStart(2, "0")}`;
