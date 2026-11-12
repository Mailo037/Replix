import { describe, expect, it } from "vitest";
import { joinAt, keptLength, partAt, partsOf, putBack, rulerLabel, rulerTicks, withSplit, type Range } from "./timeline";

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
