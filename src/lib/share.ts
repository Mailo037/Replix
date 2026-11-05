import { api } from "./api";
import { links, remaining } from "./links.svelte";
import { store } from "./store.svelte";
import { t } from "./i18n.svelte";

/** Sharing a file, with the notices that go with it. */
export const share = {
  async copy(path: string) {
    try {
      await api.copyPath(path);
      store.notify(t("Copied"), t("Paste it into Discord, a chat or a folder."));
    } catch (e) {
      store.notify(t("Couldn't copy"), String(e));
    }
  },

  async link(path: string) {
    // A link that still works is copied again rather than uploaded twice.
    const existing = links.of(path);
    if (existing) {
      try {
        await navigator.clipboard.writeText(existing.url);
        store.notify(t("Link copied"), t("{url} · works for {time} more", { url: existing.url, time: remaining(existing) }));
      } catch (e) {
        store.notify(t("Couldn't copy"), String(e));
      }
      return;
    }
    if (links.uploading[path]) return;
    links.uploading[path] = true;
    store.notify(t("Uploading…"), t("The link is copied as soon as it's ready."));
    try {
      const url = await api.shareLink(path);
      links.add(path, url);
      store.notify(t("Link copied"), t("{url} · works for 72 hours", { url }));
    } catch (e) {
      store.notify(t("No link this time"), String(e));
    } finally {
      delete links.uploading[path];
    }
  },
};
