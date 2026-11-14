import { describe, expect, it } from "vitest";
import { addRange, coveredBy, inRanges, joinAt, keptLength, nextSpeed, partAt, partsOf, playLength, putBack, removeRange, rulerLabel, rulerTicks, speedAt, speedLabel, withSpeed, withSplit, type Range } from "./timeline";

describe("partsOf", () => {
  it("is one part without splits", () => {
    expect(partsOf(0, 10, [], [])).toEqual([{ start: 0, end: 10, removed: false }]);
  });

  it("cuts at splits between the trim edges and ignores the rest", () => {
    expect(partsOf(2, 8, [1, 4, 6, 9], []).map((p) => [p.start, p.end])).toEqual([
      [2, 4],
      [4, 6],
      [6, 8],
    ]);
  });

  it("marks parts a cut covers as removed", () => {
    const parts = partsOf(0, 9, [3, 6], [[3, 6]]);
    expect(parts.map((p) => p.removed)).toEqual([false, true, false]);
    expect(keptLength(parts)).toBe(6);
  });
});

describe("withSplit", () => {
  it("adds a split in order", () => {
    expect(withSplit([5], 2, 0, 10)).toEqual([2, 5]);
  });

  it("refuses a split at an edge or next to another", () => {
    expect(withSplit([], 0.1, 0, 10)).toBeNull();
    expect(withSplit([], 9.9, 0, 10)).toBeNull();
    expect(withSplit([5], 5.1, 0, 10)).toBeNull();
  });
});

describe("putBack", () => {
  it("gives back one part of a cut that spans several", () => {
    const cuts: Range[] = [[2, 8]];
    const parts = partsOf(0, 10, [2, 4, 6, 8], cuts);
    const middle = partAt(parts, 5)!;
    expect(putBack(cuts, middle)).toEqual([
      [2, 4],
      [6, 8],
    ]);
  });
});

describe("joinAt", () => {
  it("keeps the whole when a removed part joins a kept one", () => {
    const cuts: Range[] = [[4, 10]];
    const parts = partsOf(0, 10, [4], cuts);
    expect(joinAt(parts, [4], cuts, 4)).toEqual({ splits: [], cuts: [] });
  });

  it("keeps two removed parts removed", () => {
    const cuts: Range[] = [
      [2, 4],
      [4, 6],
    ];
    const parts = partsOf(0, 8, [2, 4, 6], cuts);
    expect(joinAt(parts, [2, 4, 6], cuts, 4)).toEqual({ splits: [2, 6], cuts });
  });
});

describe("ruler", () => {
  it("has about ten marks at a round step", () => {
    expect(rulerTicks(30)).toEqual([0, 5, 10, 15, 20, 25]);
    expect(rulerTicks(600).length).toBeLessThanOrEqual(10);
    expect(rulerTicks(0)).toEqual([]);
  });

  it("labels marks in minutes and seconds", () => {
    expect(rulerLabel(75)).toBe("1:15");
  });
});

describe("slow parts", () => {
  it("make the kept parts play longer", () => {
    const parts = partsOf(0, 10, [2, 4, 6], [[6, 10]]);
    expect(playLength(parts, [])).toBe(6);
    expect(playLength(parts, [{ start: 2, end: 4, factor: 0.5 }])).toBe(8);
    // Only what is kept counts, and a quarter takes four times as long.
    expect(playLength(parts, [{ start: 5, end: 8, factor: 0.25 }])).toBe(9);
  });

  it("set one part's speed and leave the rest", () => {
    const speeds = [{ start: 0, end: 6, factor: 0.5 }];
    expect(withSpeed(speeds, { start: 2, end: 4 }, 0.25)).toEqual([
      { start: 0, end: 2, factor: 0.5 },
      { start: 2, end: 4, factor: 0.25 },
      { start: 4, end: 6, factor: 0.5 },
    ]);
    expect(withSpeed(speeds, { start: 0, end: 6 }, 1)).toEqual([]);
    expect(speedAt(speeds, 3)).toBe(0.5);
    expect(speedAt(speeds, 7)).toBe(1);
  });

  it("go normal, half, a quarter and back", () => {
    expect([1, 0.5, 0.25].map(nextSpeed)).toEqual([0.5, 0.25, 1]);
    expect([1, 0.5, 0.25].map(speedLabel)).toEqual(["1×", "½×", "¼×"]);
  });
});

describe("silent stretches", () => {
  it("join when they overlap or touch, and come apart again", () => {
    let r: Range[] = addRange([], [2, 4]);
    r = addRange(r, [6, 8]);
    r = addRange(r, [4, 5]);
    expect(r).toEqual([
      [2, 5],
      [6, 8],
    ]);
    expect(addRange(r, [1, 9])).toEqual([[1, 9]]);
    // A part in the middle taken back cuts the stretch in two.
    expect(removeRange([[1, 9]], [3, 4])).toEqual([
      [1, 3],
      [4, 9],
    ]);
    expect(removeRange([[1, 9]], [0, 10])).toEqual([]);
  });

  it("say what they take in", () => {
    const r: Range[] = [
      [1, 3],
      [5, 6],
    ];
    expect(inRanges(r, 2)).toBe(true);
    expect(inRanges(r, 3)).toBe(false);
    expect(inRanges(undefined, 2)).toBe(false);
    expect(coveredBy(r, [1, 3])).toBe(true);
    expect(coveredBy(r, [2, 4])).toBe(false);
  });
});
