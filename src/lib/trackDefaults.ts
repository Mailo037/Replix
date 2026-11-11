import type { Settings, TrackDefault, TrackEdit, TrackMeta } from "./api";

// How a track starts out in the editor: one program's own default first, then the default
// for its kind of track, then as recorded. A clip's draft always wins over both.

/** The kinds of track that have a default of their own, in the order the settings show them. */
export const KINDS = ["game", "app", "desktop", "mic"] as const;

/** A program's executable name in lower case, from its full path. */
function exeName(path: string): string {
  return (path.split(/[\\/]/).pop() ?? path).toLowerCase();
}

/** The key a program's own default is saved under; none for the desktop and the microphone. */
export function programKey(track: TrackMeta): string | null {
  if (track.kind === "mic" || track.kind === "desktop") return null;
  return track.exe ? exeName(track.exe) : null;
}

/** The default that applies to a track, if any was set. */
export function defaultFor(settings: Settings | null, track: TrackMeta): TrackDefault | undefined {
  const list = settings?.trackDefaults ?? [];
  const program = programKey(track);
  return (program ? list.find((d) => d.key === program) : undefined) ?? list.find((d) => d.key === track.kind);
}

/** How a track starts out when the clip has no draft for it. */
export function startingEdit(settings: Settings | null, track: TrackMeta): TrackEdit {
  const d = defaultFor(settings, track);
  return { enabled: d?.enabled ?? true, volume: d?.volume ?? 1 };
}

/** The list with `next` put in place of the default with the same key. */
export function withDefault(list: TrackDefault[], next: TrackDefault): TrackDefault[] {
  return list.some((d) => d.key === next.key) ? list.map((d) => (d.key === next.key ? next : d)) : [...list, next];
}
