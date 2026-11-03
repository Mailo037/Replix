export function duration(seconds: number): string {
  const s = Math.max(0, Math.round(seconds));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const r = s % 60;
  return h > 0 ? `${h}:${pad(m)}:${pad(r)}` : `${m}:${pad(r)}`;
}

/** 0:12.4 — for positions in the editor. */
export function position(seconds: number): string {
  const s = Math.max(0, seconds);
  const m = Math.floor(s / 60);
  const r = s - m * 60;
  return `${m}:${r.toFixed(1).padStart(4, "0")}`;
}

function pad(n: number): string {
  return n.toString().padStart(2, "0");
}

const timeFormat = new Intl.DateTimeFormat("en-GB", { hour: "2-digit", minute: "2-digit" });
const recentDayFormat = new Intl.DateTimeFormat("en-GB", { weekday: "long", day: "numeric", month: "long" });
const dayFormat = new Intl.DateTimeFormat("en-GB", { day: "numeric", month: "long" });
const oldDayFormat = new Intl.DateTimeFormat("en-GB", { day: "numeric", month: "long", year: "numeric" });
const recentOldDayFormat = new Intl.DateTimeFormat("en-GB", { weekday: "long", day: "numeric", month: "long", year: "numeric" });

export function time(ms: number): string {
  return timeFormat.format(ms);
}

/** "Today", "Yesterday", or a date. */
export function day(ms: number): string {
  const d = new Date(ms);
  const today = new Date();
  const start = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const diff = Math.round((start(today) - start(d)) / 86_400_000);
  if (diff === 0) return "Today";
  if (diff === 1) return "Yesterday";
  if (d.getFullYear() === today.getFullYear()) {
    return (diff < 7 ? recentDayFormat : dayFormat).format(d);
  }
  return (diff < 7 ? recentOldDayFormat : oldDayFormat).format(d);
}

export function size(bytes: number): string {
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
  if (bytes >= 1024 ** 2) return `${Math.round(bytes / 1024 ** 2)} MB`;
  return `${Math.max(1, Math.round(bytes / 1024))} KB`;
}

export function lengthLabel(seconds: number): string {
  return seconds < 60 ? `${seconds} s` : `${seconds / 60} min`;
}

/** Readable text colour on top of an accent colour. */
export function onColor(hex: string): string {
  const v = hex.replace("#", "");
  const [r, g, b] = [0, 2, 4].map((i) => parseInt(v.slice(i, i + 2), 16) / 255);
  const lin = (c: number) => (c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
  const lum = 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
  // Whichever of plum ink or white contrasts more with the accent.
  const dark = (lum + 0.05) / (0.0072 + 0.05);
  const light = 1.05 / (lum + 0.05);
  return dark >= light ? "#17111f" : "#ffffff";
}

const GAME_COLORS = ["#ffb3c7", "#9fd8ff", "#c9b6ff", "#ffc79a", "#a8f0c6", "#ffe58a"];

/** A pastel per game, the same one every time for the same name. */
export function gameColor(name: string): string {
  let h = 0;
  for (const ch of name) h = (h * 31 + ch.charCodeAt(0)) >>> 0;
  return GAME_COLORS[h % GAME_COLORS.length];
}

export function rgba(hex: string, alpha: number): string {
  const v = hex.replace("#", "");
  const [r, g, b] = [0, 2, 4].map((i) => parseInt(v.slice(i, i + 2), 16));
  return `rgba(${r}, ${g}, ${b}, ${alpha})`;
}
