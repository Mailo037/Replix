// ASCII is an accent: thumbnails drawn as coloured characters, and a faint
// character field behind the top of the window.

const RAMP = " .:-=+*#";

function hash(x: number, y: number): number {
  const s = Math.sin(x * 12.9898 + y * 78.233) * 43758.5453;
  return s - Math.floor(s);
}

/** The background raster: sparse characters, the same every time. */
export function raster(cols = 260, rows = 34): string {
  let out = "";
  for (let y = 0; y < rows; y++) {
    let line = "";
    for (let x = 0; x < cols; x++) {
      const h = hash(x, y);
      line += h < 0.03 ? "+" : h < 0.1 ? "·" : " ";
    }
    out += line + "\n";
  }
  return out;
}

const cache = new Map<string, HTMLImageElement>();

function load(src: string): Promise<HTMLImageElement> {
  const hit = cache.get(src);
  if (hit?.complete) return Promise.resolve(hit);
  return new Promise((resolve, reject) => {
    const img = new Image();
    img.crossOrigin = "anonymous";
    img.onload = () => {
      cache.set(src, img);
      resolve(img);
    };
    img.onerror = reject;
    img.src = src;
  });
}

/**
 * Draws `src` into `canvas` as characters. Each character takes the colour of
 * the pixels under it and is denser where the picture is brighter.
 */
export async function drawAscii(canvas: HTMLCanvasElement, src: string, cell = 8): Promise<boolean> {
  let img: HTMLImageElement;
  try {
    img = await load(src);
  } catch {
    return false;
  }
  const dpr = window.devicePixelRatio || 1;
  const w = canvas.clientWidth;
  const h = canvas.clientHeight;
  if (!w || !h) return false;
  canvas.width = Math.round(w * dpr);
  canvas.height = Math.round(h * dpr);
  const charW = cell * 0.6;
  const cols = Math.max(8, Math.floor(w / charW));
  const rows = Math.max(4, Math.floor(h / cell));

  const sample = document.createElement("canvas");
  sample.width = cols;
  sample.height = rows;
  const sctx = sample.getContext("2d", { willReadFrequently: true });
  if (!sctx) return false;
  // Cover-crop the picture into the grid.
  const scale = Math.max(cols / img.width, (rows * (cell / charW)) / img.height);
  const sw = cols / scale;
  const sh = (rows * (cell / charW)) / scale;
  sctx.drawImage(img, (img.width - sw) / 2, (img.height - sh) / 2, sw, sh, 0, 0, cols, rows);
  let data: Uint8ClampedArray;
  try {
    data = sctx.getImageData(0, 0, cols, rows).data;
  } catch {
    return false;
  }

  const ctx = canvas.getContext("2d");
  if (!ctx) return false;
  ctx.scale(dpr, dpr);
  ctx.fillStyle = "#0a0a09";
  ctx.fillRect(0, 0, w, h);
  ctx.font = `500 ${cell}px "JetBrains Mono", monospace`;
  ctx.textBaseline = "top";
  const offsetX = (w - cols * charW) / 2;
  const offsetY = (h - rows * cell) / 2;
  for (let y = 0; y < rows; y++) {
    for (let x = 0; x < cols; x++) {
      const i = (y * cols + x) * 4;
      const r = data[i];
      const g = data[i + 1];
      const b = data[i + 2];
      const lum = (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255;
      const ch = RAMP[Math.min(RAMP.length - 1, Math.floor(Math.pow(lum, 0.8) * RAMP.length))];
      if (ch === " ") continue;
      // Lift dark colours so the characters stay visible on black.
      const lift = 1.25 + (1 - lum) * 0.6;
      ctx.fillStyle = `rgb(${Math.min(255, r * lift + 18)}, ${Math.min(255, g * lift + 18)}, ${Math.min(255, b * lift + 18)})`;
      ctx.fillText(ch, offsetX + x * charW, offsetY + y * cell);
    }
  }
  return true;
}

/** Falling characters for a thumbnail that is not ready yet. */
export function rain(cols: number, rows: number, tick: number): string {
  let out = "";
  for (let y = 0; y < rows; y++) {
    let line = "";
    for (let x = 0; x < cols; x++) {
      const drop = (hash(x, 7) * rows * 2 + tick) % (rows * 2);
      const d = y - drop;
      line += d <= 0 && d > -4 ? (hash(x, y + tick) < 0.5 ? ":" : ".") : " ";
    }
    out += line + "\n";
  }
  return out;
}
