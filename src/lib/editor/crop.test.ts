import { describe, expect, it } from "vitest";
import { MIN_CROP, frameBox, frameShare, placed, resized, zoomed } from "./crop";

const share = frameShare(1920, 1080);

describe("crop", () => {
  it("takes the tallest 9:16 frame of a wide picture", () => {
    expect(share).toBeCloseTo(0.3164, 3);
    expect(frameBox(share, { x: 0.5, y: 0.5, scale: 1 })).toMatchObject({ top: 0, height: 1 });
  });

  it("keeps the frame inside the picture when placed beyond it", () => {
    expect(placed(share, -1, 0, 1)).toMatchObject({ x: 0 });
    expect(placed(share, 5, 0, 1)).toMatchObject({ x: 1 });
  });

  it("zooms around the centre and never below the smallest size", () => {
    const crop = zoomed(share, { x: 0.5, y: 0.5, scale: 1 }, 0.5);
    const box = frameBox(share, crop);
    expect(box.left + box.width / 2).toBeCloseTo(0.5);
    expect(box.top + box.height / 2).toBeCloseTo(0.5);
    expect(zoomed(share, crop, 0).scale).toBe(MIN_CROP);
  });

  it("resizes from a corner while the opposite corner stays", () => {
    const start = { x: 0.5, y: 0.5, scale: 0.5 };
    const before = frameBox(share, start);
    const after = frameBox(share, resized(share, start, "se", 1, 1));
    expect(after.left).toBeCloseTo(before.left);
    expect(after.top).toBeCloseTo(before.top);
    expect(after.height).toBeGreaterThan(before.height);
  });
});
