import { invoke } from "@tauri-apps/api/core";
import type { AppWindow } from "./api";

// Program icons for the editor's track heads, read once per program and kept as data URLs.

const icons = $state<Record<string, string | null>>({});
const asked = new Set<string>();

interface AppIcon {
  width: number;
  height: number;
  rgba: number[];
}

async function load(path: string) {
  try {
    const icon = await invoke<AppIcon | null>("app_icon", { path });
    if (!icon) return void (icons[path] = null);
    const canvas = document.createElement("canvas");
    canvas.width = icon.width;
    canvas.height = icon.height;
    const context = canvas.getContext("2d");
    if (!context) return void (icons[path] = null);
    context.putImageData(new ImageData(new Uint8ClampedArray(icon.rgba), icon.width, icon.height), 0, 0);
    icons[path] = canvas.toDataURL("image/png");
  } catch {
    icons[path] = null;
  }
}

/** The icon of the program at `path`, once it has been read; null while there is none. */
export function appIcon(path: string | null | undefined): string | null {
  if (!path) return null;
  if (!asked.has(path)) {
    asked.add(path);
    load(path);
  }
  return icons[path] ?? null;
}

/**
 * An open app as a choice in a list, the same everywhere one is picked: its name, its window's
 * title where that says more than the name, and its icon (null until read, shown as a frame).
 */
export function appChoice(a: AppWindow) {
  return { value: a.exe, label: a.name, hint: a.title && a.title !== a.name ? a.title : undefined, icon: appIcon(a.path) };
}

/**
 * The open apps as choices for picking one, with the one picked before kept in the list even
 * while it is closed. Its entry carries the name exactly as it was saved, so the list marks it
 * whatever the case of the running program's name.
 */
export function appChoices(apps: AppWindow[], chosen: string | null | undefined) {
  const same = (exe: string) => !!chosen && exe.toLowerCase() === chosen.toLowerCase();
  const list = apps.map((a) => ({ ...appChoice(a), value: same(a.exe) ? chosen! : a.exe }));
  if (chosen && !apps.some((a) => same(a.exe))) {
    list.push({ value: chosen, label: chosen.replace(/\.exe$/i, ""), hint: undefined, icon: null });
  }
  return list;
}
