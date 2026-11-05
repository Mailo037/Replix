import { invoke } from "@tauri-apps/api/core";

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
