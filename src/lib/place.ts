const PAD = 8;

/**
 * Puts a fixed-position element next to an anchor (a rect, or a point such as the cursor), moved or
 * flipped so it stays inside the window. Returns true when it ended up above the anchor.
 */
export function fit(el: HTMLElement, anchor: DOMRect, { align = "start", gap = 6 }: { align?: "start" | "end"; gap?: number } = {}) {
  const r = el.getBoundingClientRect();
  let left = align === "end" ? anchor.right - r.width : anchor.left;
  left = Math.max(PAD, Math.min(left, innerWidth - r.width - PAD));
  let top = anchor.bottom + gap;
  const up = top + r.height > innerHeight - PAD && anchor.top - gap - r.height >= PAD;
  if (up) top = anchor.top - gap - r.height;
  top = Math.max(PAD, Math.min(top, innerHeight - r.height - PAD));
  el.style.left = `${Math.round(left)}px`;
  el.style.top = `${Math.round(top)}px`;
  return up;
}
