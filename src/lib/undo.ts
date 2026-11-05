import { api } from "./api";
import { store } from "./store.svelte";

// Ctrl+Z in the library: the last few things done to clips, newest last.

interface Step {
  /** What comes back, for the notice: "Desktop clip", "3 items". */
  what: string;
  run: () => Promise<void>;
}

const steps: Step[] = [];

export const undo = {
  push(what: string, run: () => Promise<void>) {
    steps.push({ what, run });
    if (steps.length > 30) steps.shift();
  },

  async last() {
    const step = steps.pop();
    if (!step) return;
    try {
      await step.run();
    } catch (e) {
      store.notify("Couldn't undo", String(e));
    }
  },
};

/** Sends clips to the recycle bin, with a notice that can bring them back. */
export async function recycle(ids: string[], what: string) {
  const paths = await api.deleteItems(ids);
  const restore = async () => {
    await api.restoreItems(paths);
    store.notify("Restored", what);
  };
  undo.push(what, restore);
  store.notify("Moved to the recycle bin", `${what} · Ctrl+Z to undo`, {
    label: "Undo",
    run: () => {
      // The button and Ctrl+Z undo the same step; whichever comes first takes it.
      const i = steps.findIndex((s) => s.run === restore);
      if (i >= 0) steps.splice(i, 1);
      return restore().catch((e) => store.notify("Couldn't undo", String(e)));
    },
  });
}

/** Renames a clip, undoable. */
export async function rename(id: string, from: string, to: string) {
  if (!to || to === from) return;
  await api.renameItem(id, to);
  undo.push(to, async () => {
    await api.renameItem(id, from);
    store.notify("Name restored", from);
  });
}
