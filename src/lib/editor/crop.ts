// The 9:16 frame over the preview. Every size and place is a share of the preview (0 to 1),
// so the same numbers work at any window size and in the export.

/** The smallest frame, as a share of the video's height. */
export const MIN_CROP = 0.2;

/** Where the frame sits: x and y from 0 (left, top) to 1 (right, bottom); scale is its height as a share of the video's. */
export interface Crop {
  x: number;
  y: number;
  scale: number;
}

/** Share of the picture's width that the tallest 9:16 frame takes. */
export function frameShare(width: number, height: number): number {
  return width && height ? Math.min(1, (height * 9) / 16 / width) : 0.32;
}

/** The frame's box in shares of the preview. */
export function frameBox(share: number, crop: Crop) {
  const w = share * crop.scale;
  return { left: (1 - w) * crop.x, top: (1 - crop.scale) * crop.y, width: w, height: crop.scale };
}

/** The crop that puts the frame's top-left corner at (left, top) with this scale. */
export function placed(share: number, left: number, top: number, scale: number): Crop {
  const w = share * scale;
  return {
    scale,
    x: w < 1 ? Math.min(1, Math.max(0, left / (1 - w))) : 0.5,
    y: scale < 1 ? Math.min(1, Math.max(0, top / (1 - scale))) : 0.5,
  };
}

/** The frame sized around its centre, kept inside the video. */
export function zoomed(share: number, crop: Crop, scale: number): Crop {
  scale = Math.min(1, Math.max(MIN_CROP, scale));
  const box = frameBox(share, crop);
  const cx = box.left + box.width / 2;
  const cy = box.top + box.height / 2;
  const w = share * scale;
  return placed(share, Math.min(1 - w, Math.max(0, cx - w / 2)), Math.min(1 - scale, Math.max(0, cy - scale / 2)), scale);
}

export type Corner = "nw" | "ne" | "sw" | "se";

/**
 * The frame resized by dragging a corner to (px, py); the opposite corner stays put and the
 * frame keeps 9:16. `start` is the crop when the drag began.
 */
export function resized(share: number, start: Crop, corner: Corner, px: number, py: number): Crop {
  const box = frameBox(share, start);
  const left = corner.endsWith("w");
  const top = corner.startsWith("n");
  // The fixed corner.
  const ax = left ? box.left + box.width : box.left;
  const ay = top ? box.top + box.height : box.top;
  const room = Math.min(top ? ay : 1 - ay, (left ? ax : 1 - ax) / share, 1);
  const want = Math.max(Math.abs(py - ay), Math.abs(px - ax) / share);
  const scale = Math.min(room, Math.max(MIN_CROP, want));
  return placed(share, left ? ax - share * scale : ax, top ? ay - scale : ay, scale);
}
