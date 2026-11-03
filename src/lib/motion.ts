import { cubicOut } from "svelte/easing";
import { fly } from "svelte/transition";

/** Menus and popovers fade in and out with a small drop; instant when motion is reduced. */
export function pop(node: Element) {
  const still = matchMedia("(prefers-reduced-motion: reduce)").matches;
  return fly(node, { y: -4, duration: still ? 0 : 140, easing: cubicOut });
}
