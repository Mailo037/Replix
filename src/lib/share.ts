import { api } from "./api";
import { store } from "./store.svelte";

/** Sharing a file, with the notices that go with it. */
export const share = {
  async copy(path: string) {
    try {
      await api.copyPath(path);
      store.notify("Copied", "Paste it into Discord, a chat or a folder.");
    } catch (e) {
      store.notify("Couldn't copy", String(e));
    }
  },

  async link(path: string) {
    store.notify("Uploading…", "The link is copied as soon as it's ready.");
    try {
      const url = await api.shareLink(path);
      store.notify("Link copied", `${url} · works for 72 hours`);
    } catch (e) {
      store.notify("No link this time", String(e));
    }
  },
};
