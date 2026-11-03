// A faint character field behind the top of the window.

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
