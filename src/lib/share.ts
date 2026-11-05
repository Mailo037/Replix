import { api } from "./api";
import { links, remaining } from "./links.svelte";
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
    // A link that still works is copied again rather than uploaded twice.
    const existing = links.of(path);
    if (existing) {
      try {
        await navigator.clipboard.writeText(existing.url);
        store.notify("Link copied", `${existing.url} · works for ${remaining(existing)} more`);
      } catch (e) {
        store.notify("Couldn't copy", String(e));
      }
      return;
    }
    if (links.uploading[path]) return;
    links.uploading[path] = true;
    store.notify("Uploading…", "The link is copied as soon as it's ready.");
    try {
      const url = await api.shareLink(path);
      links.add(path, url);
      store.notify("Link copied", `${url} · works for 72 hours`);
    } catch (e) {
      store.notify("No link this time", String(e));
    } finally {
      delete links.uploading[path];
    }
  },
};
