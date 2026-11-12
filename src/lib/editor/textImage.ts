// Draws words for the picture: white in Replix's display font with a dark outline, so they
// read on any game. The drawing is what the preview shows and what the export lays on top,
// so both look the same and FFmpeg needs no font of its own.

/** Letter size the drawing is made at, in pixels; the export scales it to the frame. */
const FONT_PX = 160;
const FONT = `800 ${FONT_PX}px "Bricolage Grotesque Variable", "Inter Variable", system-ui, sans-serif`;
const LINE = 1.15;

export interface Drawn {
  /** A PNG data URL. */
  png: string;
  /** The drawing's height for letters one unit tall: its height share is `size * ratio`. */
  ratio: number;
}

/** Draws `text` (lines split at line breaks) as a PNG. */
export async function drawText(text: string): Promise<Drawn> {
  try {
    await document.fonts.load(FONT);
  } catch {
    // The fallback fonts draw it as well.
  }
  const lines = (text.trim() || " ").split("\n");
  const canvas = document.createElement("canvas");
  const ctx = canvas.getContext("2d");
  if (!ctx) return { png: "", ratio: 1 };
  ctx.font = FONT;
  const pad = FONT_PX * 0.2;
  const width = Math.max(...lines.map((l) => ctx.measureText(l).width));
  canvas.width = Math.ceil(width + pad * 2);
  canvas.height = Math.ceil(lines.length * FONT_PX * LINE + pad * 2);
  // Resizing the canvas forgets the font.
  ctx.font = FONT;
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  ctx.lineJoin = "round";
  ctx.lineWidth = FONT_PX * 0.14;
  ctx.strokeStyle = "rgba(10, 8, 14, 0.9)";
  ctx.fillStyle = "#ffffff";
  lines.forEach((line, i) => {
    const y = pad + FONT_PX * LINE * (i + 0.5);
    ctx.strokeText(line, canvas.width / 2, y);
    ctx.fillText(line, canvas.width / 2, y);
  });
  return { png: canvas.toDataURL("image/png"), ratio: canvas.height / FONT_PX };
}
