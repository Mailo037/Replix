import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { expect, test, type Browser, type Page } from "@playwright/test";
import type { Draft, Item, Settings } from "../../src/lib/api";
import { editorData, fixture, mockTauri, settings as testSettings } from "../e2e/tauri";
import { scene, type Game } from "./scenes";

// The product shots in docs/images, taken from the page in a browser with Tauri stood in for
// (the same stand-in as the e2e tests). The library holds made-up games with drawn pictures,
// so no real game or desktop ends up in the README. `npm run shots` takes them again.

const MEDIA = resolve("tests/shots/media");
const URL = "/tests/shots/media";
const OUT = resolve("docs/images");

// A fixed afternoon, so the times and day headings read the same on every run.
const NOW = new Date(2026, 10, 13, 15, 20).getTime();
const at = (daysAgo: number, hour: number, minute: number) => {
  const d = new Date(NOW);
  d.setDate(d.getDate() - daysAgo);
  d.setHours(hour, minute, 0, 0);
  return d.getTime();
};

const games: Game[] = ["Neon Rally", "Hollow Peak", "Deep Orbit", "Skyline Drift", "Thornwood", "Ember Wastes"];
const slug = (g: string) => g.toLowerCase().replace(/\s+/g, "-");

const TRACKS = [
  { name: "Neon Rally", kind: "game" as const },
  { name: "Discord", kind: "app" as const },
  { name: "Spotify", kind: "app" as const },
  { name: "Microphone", kind: "mic" as const },
];

/** The clip opened in the editor: two parts kept, Discord muted, the music shortened. */
const DRAFT: Draft = {
  trimStart: 1.5,
  trimEnd: 27.5,
  splits: [11, 16.5],
  cuts: [[11, 16.5]],
  vertical: false,
  cropX: 0.45,
  tracks: [
    { enabled: true, volume: 1 },
    { enabled: false, volume: 1 },
    { enabled: true, volume: 0.35, start: 4, end: 24.5 },
    { enabled: true, volume: 1 },
  ],
};

function item(id: string, title: string, game: Game, created: number, extra: Partial<Item> = {}, variant = 0): Item {
  const screenshot = extra.kind === "screenshot";
  return {
    id,
    path: screenshot ? `${URL}/${slug(game)}-${variant}.png` : `${URL}/clip.webm`,
    size: screenshot ? 4_000_000 : 180_000_000,
    thumb: `${URL}/${slug(game)}-${variant}.png`,
    kind: "clip",
    title,
    game,
    created,
    duration: screenshot ? 0 : 30,
    width: 1920,
    height: 1080,
    tracks: screenshot ? [] : TRACKS.map((t, i) => (i === 0 ? { ...t, name: game } : t)),
    draft: null,
    favorite: false,
    seen: true,
    unread: false,
    markers: [],
    tags: [],
    ...extra,
  };
}

const items: Item[] = [
  item("a", "Last-lap overtake", "Neon Rally", at(0, 15, 12), { seen: false, duration: 60, markers: [{ at: 21.5, name: "Overtake" }] }),
  item("b", "Triple on the ridge", "Hollow Peak", at(0, 14, 48), { favorite: true }),
  item("c", "Ring at sunrise", "Deep Orbit", at(0, 13, 5), { kind: "screenshot" }),
  item("d", "Clutch 1v3", "Ember Wastes", at(0, 12, 31), { draft: { ...DRAFT, tracks: DRAFT.tracks.map((t) => ({ ...t })) } }),
  item("e", "Evening session", "Skyline Drift", at(1, 21, 4), { kind: "session", duration: 18 * 60 + 42 }),
  item("f", "Drift into the tunnel", "Skyline Drift", at(1, 20, 37), { favorite: true, duration: 45, tags: ["funny"] }, 1),
  item("g", "First try on the warden", "Thornwood", at(1, 18, 15), { duration: 60 }),
  item("h", "Peak at dawn", "Hollow Peak", at(1, 17, 2), { kind: "screenshot" }, 1),
  item("i", "Photo finish", "Neon Rally", at(12, 22, 40), {}, 1),
  item("j", "Docking on the first try", "Deep Orbit", at(12, 21, 10), {}, 1),
  item("k", "Lost in the fog", "Thornwood", at(12, 19, 55), {}, 1),
];

const settings: Settings = {
  ...testSettings,
  clipsDir: "C:\\Users\\you\\Videos\\Replix",
  quality: "high",
};

/** Draws every game's pictures, the filmstrip and a 30 s clip, unless they are there already. */
async function makeMedia(browser: Browser) {
  if (existsSync(join(MEDIA, "clip.webm"))) return;
  mkdirSync(MEDIA, { recursive: true });
  const page = await browser.newPage({ viewport: { width: 1920, height: 1080 } });
  const draw = async (svg: string, file: string, width: number, height: number) => {
    await page.setViewportSize({ width, height });
    await page.setContent(`<style>*{margin:0}svg{display:block;width:${width}px;height:${height}px}</style>${svg}`);
    writeFileSync(join(MEDIA, file), await page.screenshot({ type: "png" }));
  };
  for (const game of games) {
    for (const variant of [0, 1]) await draw(scene(game, variant ? 0.8 : 0), `${slug(game)}-${variant}.png`, 960, 540);
  }
  await draw(scene("Neon Rally"), "frame.png", 1920, 1080);
  // The filmstrip: frames side by side, the view drifting a little as in a moving clip.
  const frames = Array.from({ length: 16 }, (_, i) => `<div style="width:160px;height:90px;overflow:hidden">${scene("Neon Rally", (i % 5) * 0.15).replace(/width="1920" height="1080"/, 'width="160" height="90"')}</div>`);
  await page.setViewportSize({ width: 2560, height: 90 });
  await page.setContent(`<style>*{margin:0}body{display:flex}</style>${frames.join("")}`);
  writeFileSync(join(MEDIA, "strip.png"), await page.screenshot({ type: "png" }));
  await page.close();

  execFileSync(ffmpeg(), [
    "-y", "-loglevel", "error",
    "-loop", "1", "-framerate", "10", "-i", join(MEDIA, "frame.png"),
    "-f", "lavfi", "-i", "anoisesrc=color=pink:amplitude=0.05:d=30",
    "-t", "30", "-vf", "scale=1280:720,format=yuv420p",
    "-c:v", "libvpx-vp9", "-b:v", "0", "-crf", "40", "-deadline", "realtime", "-c:a", "libopus",
    join(MEDIA, "clip.webm"),
  ]);
}

/** FFmpeg from FFMPEG, the PATH, or the copy Replix downloaded on Windows. */
function ffmpeg(): string {
  if (process.env.FFMPEG) return process.env.FFMPEG;
  const local = process.env.LOCALAPPDATA && join(process.env.LOCALAPPDATA, "app.replix", "ffmpeg");
  if (local && existsSync(local)) {
    for (const version of readdirSync(local).sort().reverse()) {
      const exe = join(local, version, "ffmpeg.exe");
      if (existsSync(exe)) return exe;
    }
  }
  return "ffmpeg";
}

/** A track's loudness: the game busy, Discord quiet, the music steady, the mic in bursts. */
function loudness(track: number): number[] {
  let s = 7 + track * 13;
  const rnd = () => ((s = (s * 1664525 + 1013904223) >>> 0) / 2 ** 32);
  return Array.from({ length: 400 }, (_, i) => {
    const r = rnd();
    if (track === 0) return 0.35 + 0.45 * r * (0.6 + 0.4 * Math.abs(Math.sin(i / 23)));
    if (track === 1) return 0.05 + 0.15 * r * (Math.sin(i / 9) > 0.4 ? 1 : 0.2);
    if (track === 2) return 0.3 + 0.2 * r;
    return Math.sin(i / 17) > 0.3 ? 0.2 + 0.5 * r : 0.03 * r;
  });
}

async function open(page: Page, overrides: Partial<Settings> = {}) {
  await page.clock.setFixedTime(NOW);
  const data = fixture({
    settings: { ...settings, ...overrides },
    items,
    monitors: [
      { index: 0, width: 2560, height: 1440, primary: true },
      { index: 1, width: 1920, height: 1080, primary: false },
    ],
    mics: [{ id: "mic", name: "Microphone (USB Headset)", default: true }],
  });
  data.editors = Object.fromEntries(
    items.map((i) => [i.id, { ...editorData(i), duration: 30, filmstrip: `${URL}/strip.png`, tracks: i.tracks.map((t) => ({ ...t, audio: "/tests/fixtures/tone.webm" })) }]),
  );
  await mockTauri(page, data);
  // Answers the stand-in gives differently for the shots: recording a game, real waveforms.
  await page.addInitScript(
    ({ url, previews }) => {
      const internals = (window as any).__TAURI_INTERNALS__;
      const invoke = internals.invoke;
      internals.invoke = async (cmd: string, args: Record<string, any> = {}) => {
        switch (cmd) {
          case "get_status":
            return { state: "recording", target: "Neon Rally", sessionSince: null, encoder: "NVIDIA NVENC", error: null, clipSeconds: 30 };
          case "monitor_preview":
            return `${url}/${previews[args.index] ?? previews[0]}`;
          case "storage_info":
            return { clips: 2_100_000_000, exports: 0, free: 600_000_000_000 };
          case "waveform":
            return (window as any).__loudness[args.track];
          case "list_apps":
            return [];
        }
        return invoke(cmd, args);
      };
    },
    { url: URL, previews: ["neon-rally-0.png", "hollow-peak-1.png"] },
  );
  await page.addInitScript((waves: number[][]) => ((window as any).__loudness = waves), [0, 1, 2, 3].map(loudness));
  await page.goto("/");
}

async function shoot(page: Page, name: string) {
  // Pictures and fonts settle before the shot.
  await page.evaluate(() => document.fonts.ready);
  await page.waitForTimeout(600);
  await page.screenshot({ path: join(OUT, `${name}.png`) });
}

async function openEditor(page: Page, vertical: boolean) {
  const draft = { ...DRAFT, vertical, tracks: DRAFT.tracks.map((t) => ({ ...t })) };
  items[0].draft = draft;
  await open(page);
  await page.getByRole("button", { name: "Open Last-lap overtake" }).click();
  await expect(page.locator(".clock")).toContainText("/ 0:30.0");
  // The playhead in the first part.
  await page.locator("body").click({ position: { x: 5, y: 400 } });
  for (let i = 0; i < 5; i++) await page.keyboard.press("Shift+ArrowRight");
  items[0].draft = null;
}

test.beforeAll(async ({ browser }) => {
  await makeMedia(browser);
});

test("library", async ({ page }) => {
  await open(page);
  await expect(page.getByRole("button", { name: "Open Last-lap overtake" })).toBeVisible();
  await shoot(page, "library");
});

test("status", async ({ page }) => {
  await open(page);
  await page.getByRole("button", { name: /^Recording: / }).click();
  await page.getByRole("button", { name: /Whole screen/ }).click();
  await expect(page.getByRole("menu", { name: "Whole screen" }).locator("img")).toHaveCount(2);
  await shoot(page, "status");
});

test("editor", async ({ page }) => {
  await openEditor(page, false);
  await shoot(page, "editor");
});

test("vertical", async ({ page }) => {
  await openEditor(page, true);
  await expect(page.getByRole("button", { name: /9:16 frame/ })).toBeVisible();
  await shoot(page, "vertical");
});

test("settings", async ({ page }) => {
  await open(page);
  await page.getByRole("button", { name: "Settings" }).click();
  await expect(page.getByRole("heading", { name: "Recording" })).toBeVisible();
  await shoot(page, "settings");
});

test("setup", async ({ page }) => {
  // Without motion the frames of the welcome picture stand still where they line up.
  await page.emulateMedia({ reducedMotion: "reduce" });
  await open(page, { onboarded: false });
  await expect(page.getByRole("button", { name: "Let's set it up" })).toBeVisible();
  await shoot(page, "setup-welcome");
  await page.getByRole("button", { name: "Let's set it up" }).click();
  for (let i = 0; i < 2; i++) await page.getByRole("button", { name: "Next", exact: true }).click();
  await shoot(page, "setup-tracks");
});
