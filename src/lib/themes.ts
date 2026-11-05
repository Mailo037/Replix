// Background colours. Each steps up in lightness like the default plum:
// canvas → overlay → surface → raised → hover. Text stays the same light ink on all of them.

export interface Theme {
  id: string;
  name: string;
  colors: {
    canvas: string;
    overlay: string;
    surface: string;
    raised: string;
    hover: string;
    line: string;
    black: string;
    muted: string;
    ghost: string;
  };
}

export const themes: Theme[] = [
  {
    id: "plum",
    name: "Plum",
    colors: { canvas: "#17111f", overlay: "#211a2c", surface: "#2a2038", raised: "#33284a", hover: "#3b2f52", line: "#3a2f4f", black: "#0e0a14", muted: "#c4bad6", ghost: "#a397b8" },
  },
  {
    id: "midnight",
    name: "Midnight",
    colors: { canvas: "#10141f", overlay: "#181e2c", surface: "#1f2738", raised: "#29324a", hover: "#313b55", line: "#2e384f", black: "#0a0d14", muted: "#b9c3d8", ghost: "#97a2bb" },
  },
  {
    id: "graphite",
    name: "Graphite",
    colors: { canvas: "#141416", overlay: "#1c1c1f", surface: "#242428", raised: "#2e2e33", hover: "#36363c", line: "#333338", black: "#0c0c0d", muted: "#c3c3c9", ghost: "#9d9da6" },
  },
  {
    id: "forest",
    name: "Forest",
    colors: { canvas: "#101813", overlay: "#17221b", surface: "#1e2c23", raised: "#27382d", hover: "#2f4235", line: "#2c3d31", black: "#0a100c", muted: "#bacdc0", ghost: "#97ad9e" },
  },
  {
    id: "wine",
    name: "Wine",
    colors: { canvas: "#1c1015", overlay: "#27171e", surface: "#321e27", raised: "#3e2632", hover: "#482d3a", line: "#432a36", black: "#120a0e", muted: "#d6bcc8", ghost: "#b598a6" },
  },
  {
    id: "ocean",
    name: "Ocean",
    colors: { canvas: "#0e181b", overlay: "#152226", surface: "#1b2c31", raised: "#23383e", hover: "#2a4248", line: "#294047", black: "#091012", muted: "#b5ccd1", ghost: "#92abb1" },
  },
];

export function themeOf(id: string | undefined): Theme {
  return themes.find((t) => t.id === id) ?? themes[0];
}

/** Puts a theme's colours on the page. */
export function applyTheme(id: string | undefined) {
  const root = document.documentElement.style;
  for (const [name, value] of Object.entries(themeOf(id).colors)) root.setProperty(`--${name}`, value);
}
