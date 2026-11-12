import type { Marker, TrackMeta } from "../api";
import { t } from "../i18n.svelte";

// Names and icons the editor shows for tracks and markers. Call these where the text is
// shown, so they follow a change of language.

/** The icon for a kind of track. */
export const TRACK_ICONS: Record<string, string> = { game: "game", mic: "mic", app: "app", desktop: "desktop" };

/** A track's name as shown: the mic and desktop tracks are named in the language in use. */
export const trackName = (track: Pick<TrackMeta, "kind" | "name">) =>
  track.kind === "mic" ? t("Microphone") : track.kind === "desktop" ? t("Desktop") : track.name;

/** The icons a marker can wear; the first is the plain one. */
export const markerIcons = () => [
  { name: "flag", label: t("Flag") },
  { name: "star", label: t("Star") },
  { name: "heart", label: t("Heart") },
  { name: "trophy", label: t("Trophy") },
  { name: "skull", label: t("Skull") },
  { name: "target", label: t("Crosshair") },
  { name: "bolt", label: t("Lightning") },
  { name: "smile", label: t("Funny") },
  { name: "alert", label: t("Warning") },
];

const KNOWN = new Set(["flag", "star", "heart", "trophy", "skull", "target", "bolt", "smile", "alert"]);

/** The icon a marker wears; an unknown one falls back to the flag. */
export const markerIcon = (m: Marker) => (m.icon && KNOWN.has(m.icon) ? m.icon : "flag");
