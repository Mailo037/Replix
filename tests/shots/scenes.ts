// Pictures for the made-up games in the product shots. Each game has its own scene, drawn as
// SVG from a fixed seed, so every run draws the same pictures and no real game shows up.

export type Game = "Neon Rally" | "Hollow Peak" | "Deep Orbit" | "Skyline Drift" | "Thornwood" | "Ember Wastes";

/** A small seeded random source, so the stars and windows sit in the same place every run. */
function random(seed: number) {
  let s = seed >>> 0;
  return () => {
    s = (s * 1664525 + 1013904223) >>> 0;
    return s / 2 ** 32;
  };
}

const W = 1920;
const H = 1080;

function stars(rnd: () => number, count: number, maxY: number, opacity = 0.8) {
  let out = "";
  for (let i = 0; i < count; i++) {
    const r = rnd() < 0.15 ? 2.4 : 1.4;
    out += `<circle cx="${(rnd() * W).toFixed(0)}" cy="${(rnd() * maxY).toFixed(0)}" r="${r}" fill="#fff" opacity="${(0.3 + rnd() * opacity).toFixed(2)}"/>`;
  }
  return out;
}

/** A jagged ridge from left to right, filled down to the bottom. */
function ridge(rnd: () => number, base: number, height: number, step: number, fill: string, extra = "") {
  let d = `M0 ${H} L0 ${base}`;
  for (let x = 0; x <= W + step; x += step) d += ` L${x} ${(base - rnd() * height).toFixed(0)}`;
  d += ` L${W} ${H} Z`;
  return `<path d="${d}" fill="${fill}" ${extra}/>`;
}

function glow(cx: number, cy: number, r: number, color: string, id: string) {
  return `<defs><radialGradient id="${id}"><stop offset="0" stop-color="${color}" stop-opacity="0.45"/><stop offset="1" stop-color="${color}" stop-opacity="0"/></radialGradient></defs>
    <circle cx="${cx}" cy="${cy}" r="${r * 4}" fill="url(#${id})"/>`;
}

function sky(id: string, top: string, bottom: string) {
  return `<defs><linearGradient id="${id}" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="${top}"/><stop offset="1" stop-color="${bottom}"/></linearGradient></defs>
    <rect width="${W}" height="${H}" fill="url(#${id})"/>`;
}

/** A road running to the horizon with a car on it, for the racing games. */
function road(horizon: number, car: boolean) {
  let out = `<path d="M${W / 2 - 90} ${horizon} L${W / 2 + 90} ${horizon} L${W - 240} ${H} L240 ${H} Z" fill="#0d0a16"/>`;
  for (let i = 0; i < 6; i++) {
    const t = i / 6;
    const y = horizon + (H - horizon) * t * t + 20;
    const h = 14 + 50 * t * t;
    out += `<rect x="${W / 2 - 3 - 4 * t}" y="${y}" width="${6 + 8 * t}" height="${h}" rx="3" fill="#2fb7d0" opacity="${0.5 + t / 2}"/>`;
  }
  if (car) {
    out += `<rect x="${W / 2 - 230}" y="${H - 300}" width="460" height="150" rx="26" fill="#1a1424"/>
      <rect x="${W / 2 - 170}" y="${H - 380}" width="340" height="100" rx="30" fill="#1f1830"/>
      <rect x="${W / 2 - 200}" y="${H - 250}" width="80" height="20" rx="10" fill="#ff4b55"/>
      <rect x="${W / 2 + 120}" y="${H - 250}" width="80" height="20" rx="10" fill="#ff4b55"/>`;
  }
  return out;
}

function city(rnd: () => number, horizon: number) {
  const colors = ["#ff5fa8", "#ffd166", "#2fd3e0", "#b38cff"];
  let out = "";
  for (let x = -20; x < W; ) {
    const w = 90 + rnd() * 120;
    const h = 180 + rnd() * 520;
    out += `<rect x="${x.toFixed(0)}" y="${(horizon - h).toFixed(0)}" width="${w.toFixed(0)}" height="${(h + 40).toFixed(0)}" rx="6" fill="${rnd() < 0.5 ? "#231938" : "#1c142e"}"/>`;
    for (let wy = horizon - h + 26; wy < horizon - 20; wy += 44) {
      for (let wx = x + 16; wx < x + w - 24; wx += 34) {
        if (rnd() < 0.42) out += `<rect x="${wx.toFixed(0)}" y="${wy.toFixed(0)}" width="14" height="18" rx="2" fill="${colors[Math.floor(rnd() * 4)]}" opacity="${(0.55 + rnd() * 0.45).toFixed(2)}"/>`;
      }
    }
    x += w + 8 + rnd() * 30;
  }
  return out;
}

function trees(rnd: () => number, base: number, color: string, size: number, count: number) {
  let out = "";
  for (let i = 0; i < count; i++) {
    const x = rnd() * W;
    const s = size * (0.6 + rnd() * 0.6);
    const y = base + rnd() * 60;
    out += `<path d="M${x} ${y - s * 2.2} L${x + s * 0.7} ${y} L${x - s * 0.7} ${y} Z" fill="${color}" stroke="${color}" stroke-width="10" stroke-linejoin="round"/>`;
  }
  return out;
}

/**
 * A game's scene. `shift` moves the view a little to the side, so the frames of a filmstrip
 * and the clips of one game do not all look the same.
 */
export function scene(game: Game, shift = 0): string {
  const seeds: Record<Game, number> = { "Neon Rally": 11, "Hollow Peak": 23, "Deep Orbit": 37, "Skyline Drift": 41, Thornwood: 53, "Ember Wastes": 67 };
  const rnd = random(seeds[game]);
  let body = "";
  switch (game) {
    case "Neon Rally":
      body = sky("s", "#0b0716", "#2a1840") + stars(rnd, 140, 500) + city(rnd, 760) + road(720, true);
      break;
    case "Hollow Peak":
      body =
        sky("s", "#24324a", "#7d93ab") +
        glow(560, 260, 50, "#ffffff", "g") +
        `<circle cx="560" cy="260" r="48" fill="#eef2f6"/>` +
        ridge(rnd, 560, 260, 90, "#c9d4df") +
        ridge(rnd, 700, 200, 70, "#8ea0b3") +
        ridge(rnd, 860, 140, 60, "#4c5d72");
      break;
    case "Deep Orbit":
      body =
        sky("s", "#05060d", "#141a33") +
        stars(rnd, 260, 900, 1) +
        glow(1300, 360, 150, "#4b6bff", "g") +
        `<circle cx="1300" cy="380" r="190" fill="#3f5bd8"/>
         <ellipse cx="1300" cy="400" rx="330" ry="56" fill="none" stroke="#a9b8ff" stroke-width="7" opacity="0.8"/>` +
        ridge(rnd, 760, 260, 110, "#1b2134") +
        ridge(rnd, 920, 120, 80, "#10141f");
      break;
    case "Skyline Drift":
      body =
        sky("s", "#2b1840", "#a8566a") +
        glow(1300, 600, 70, "#ffc26b", "g") +
        `<circle cx="1300" cy="600" r="70" fill="#ffd27d"/>` +
        ridge(rnd, 620, 200, 80, "#6e3a63") +
        ridge(rnd, 720, 140, 70, "#4b2647") +
        road(720, true);
      break;
    case "Thornwood":
      body =
        sky("s", "#1d3326", "#6f9a68") +
        glow(480, 300, 60, "#fff6c9", "g") +
        `<circle cx="480" cy="300" r="62" fill="#f6efc4"/>` +
        ridge(rnd, 620, 160, 100, "#3f7a4a") +
        trees(rnd, 760, "#2c5a36", 90, 40) +
        ridge(rnd, 900, 60, 90, "#1c3a24") +
        trees(rnd, 980, "#15301d", 110, 30);
      break;
    case "Ember Wastes":
      body =
        sky("s", "#3a1610", "#d0693a") +
        glow(1200, 560, 120, "#ffb15c", "g") +
        `<circle cx="1200" cy="560" r="120" fill="#ffb867" opacity="0.9"/>` +
        ridge(rnd, 600, 220, 110, "#9a3d22") +
        ridge(rnd, 760, 160, 90, "#6e2a18") +
        ridge(rnd, 920, 90, 80, "#43190f");
      break;
  }
  const dx = (shift * 160).toFixed(0);
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${W} ${H}" width="${W}" height="${H}" preserveAspectRatio="xMidYMid slice">
    <g transform="translate(${-dx} 0) scale(${1 + Math.abs(shift) * 0.1})">${body}</g></svg>`;
}
