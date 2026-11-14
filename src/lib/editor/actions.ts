// Pointer and keyboard helpers the editor's parts share.

/**
 * Follows a pointer pressed on an element until it is let go, passing on every move. With
 * `now` the press itself counts as the first move; with `stop` the press goes no further,
 * so a handle on a lane does not also move the playhead.
 */
export function follow(e: PointerEvent, move: (ev: PointerEvent) => void, opts: { now?: boolean; stop?: boolean } = {}) {
  // Only the main button drags; the right one opens the timeline's menu where it is.
  if (e.button !== 0) return;
  e.preventDefault();
  if (opts.stop) e.stopPropagation();
  const target = e.currentTarget as HTMLElement;
  target.setPointerCapture(e.pointerId);
  if (opts.now) move(e);
  const up = () => {
    target.removeEventListener("pointermove", move);
    target.removeEventListener("pointerup", up);
  };
  target.addEventListener("pointermove", move);
  target.addEventListener("pointerup", up);
}

/** The time under a pointer on a lane that spans the whole clip. */
export function timeAt(lane: HTMLElement, clientX: number, duration: number): number {
  const rect = lane.getBoundingClientRect();
  if (!rect.width) return 0;
  return Math.min(Math.max(0, ((clientX - rect.left) / rect.width) * duration), duration);
}

/** Closes a menu or panel when the pointer goes down elsewhere or Escape is pressed. */
export function outside(node: HTMLElement, close: () => void) {
  const down = (e: PointerEvent) => !node.contains(e.target as Node) && close();
  const key = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      e.stopImmediatePropagation();
      close();
    }
  };
  window.addEventListener("pointerdown", down, true);
  window.addEventListener("keydown", key, true);
  return {
    destroy() {
      window.removeEventListener("pointerdown", down, true);
      window.removeEventListener("keydown", key, true);
    },
  };
}

/** Calls `turn` for the mouse wheel over the element, without scrolling the page. */
export function wheel(node: HTMLElement, turn: (e: WheelEvent) => void) {
  const onWheel = (e: WheelEvent) => {
    if (!e.deltaY && !e.deltaX) return;
    e.preventDefault();
    turn(e);
  };
  node.addEventListener("wheel", onWheel, { passive: false });
  return {
    update(next: (e: WheelEvent) => void) {
      turn = next;
    },
    destroy: () => node.removeEventListener("wheel", onWheel),
  };
}
