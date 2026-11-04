import { store } from "./store.svelte";

const query = typeof matchMedia === "function" ? matchMedia("(prefers-reduced-motion: reduce)") : null;
const motion = $state({ still: query?.matches ?? false });
query?.addEventListener("change", (e) => (motion.still = e.matches));

/**
 * A counter that goes up every `every` ms while `active()` holds and Replix is in front.
 * For slow, decorative motion: a CSS animation keeps the page drawing every frame, even
 * in steps, where this wakes it only when something changes. Call it while a component
 * is set up.
 */
export function ticker(every: number, active: () => boolean = () => true): { readonly n: number } {
  let n = $state(0);
  $effect(() => {
    if (motion.still || !active() || !store.visible || !store.focused) return;
    const timer = setInterval(() => n++, every);
    return () => clearInterval(timer);
  });
  return {
    get n() {
      return n;
    },
  };
}
