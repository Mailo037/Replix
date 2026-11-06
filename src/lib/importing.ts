import { open } from "@tauri-apps/plugin-dialog";
import { api } from "./api";
import { t } from "./i18n.svelte";
import { isInside } from "./platform";
import { store } from "./store.svelte";

/** What the library can show, as the file picker offers it. */
const EXTENSIONS = ["mp4", "mkv", "mov", "webm", "png", "jpg", "jpeg", "webp"];

/**
 * Copies videos and pictures into the library. Files already in the clips folder are left
 * out quietly: dragging a tile and letting go over Replix itself is no import.
 */
export async function importPaths(paths: string[]) {
  const root = store.settings?.clipsDir ?? "";
  const outside = paths.filter((p) => !isInside(p, root));
  if (!outside.length) return;
  store.notify(t("Importing…"), t("{n} files", { n: outside.length }));
  try {
    const { count, skipped } = await api.importFiles(outside);
    if (!count && skipped.length) {
      store.notify(t("Nothing imported"), `${skipped[0][0]}: ${skipped[0][1]}`);
    } else if (skipped.length) {
      store.notify(t("Import finished"), `${t("{n} added to the library", { n: count })} · ${t("{n} skipped", { n: skipped.length })}`);
    } else if (count) {
      store.notify(t("Import finished"), t("{n} added to the library", { n: count }));
    } else {
      store.notify(t("Nothing imported"), t("No videos or pictures Replix can show."));
    }
  } catch (e) {
    store.notify(t("Import failed"), String(e));
  }
}

/** Lets someone pick videos and pictures to bring into the library. */
export async function chooseAndImport() {
  const picked = await open({
    multiple: true,
    title: t("Import videos and pictures"),
    filters: [{ name: t("Videos and pictures"), extensions: EXTENSIONS }],
  });
  if (picked) await importPaths(Array.isArray(picked) ? picked : [picked]);
}
