import type { Page } from "@playwright/test";
import type { Codec, EditorData, Item, Settings } from "../../src/lib/api";

// The page talks to Replix's Rust side through Tauri. In these tests a stand-in answers
// instead: it is put into the page before Replix's own scripts run, answers the commands
// from the fixtures below, records every call, and can send events the way Replix would.

const FIXTURES = "/tests/fixtures";

export const settings: Settings = {
  enabled: true,
  bufferSeconds: 30,
  source: "game",
  appExe: null,
  monitor: 0,
  fps: 60,
  quality: "medium",
  height: 0,
  encoder: "auto",
  codec: "h264",
  recordMic: true,
  micDevice: null,
  micVolume: 1,
  micNoiseSuppression: true,
  captureCursor: false,
  clipsDir: "/home/test/Videos/Replix",
  storageLimitGb: 0,
  hotkeys: { clip: "Alt+C", screenshot: "Alt+S", session: "Alt+X", marker: "Alt+M" },
  accent: "#C5F26B",
  theme: "plum",
  launchAtStartup: false,
  closeToTray: true,
  showToast: true,
  saveSound: true,
  toastPosition: "bottomRight",
  toastSize: "medium",
  profiles: [],
  excluded: [],
  language: "en",
  onboarded: true,
  trackDefaults: [],
};

const now = Date.now();

function clip(id: string, title: string, game: string, minutesAgo: number, extra: Partial<Item> = {}): Item {
  return {
    id,
    path: `${FIXTURES}/clip.webm`,
    size: 40_000_000,
    thumb: `${FIXTURES}/thumb.jpg`,
    kind: "clip",
    title,
    game,
    created: now - minutesAgo * 60_000,
    duration: 6,
    width: 320,
    height: 180,
    tracks: [
      { name: "Neon Rally", kind: "game" },
      { name: "Discord", kind: "app" },
      { name: "Microphone", kind: "mic" },
    ],
    draft: null,
    favorite: false,
    seen: true,
    unread: false,
    markers: [],
    tags: [],
    ...extra,
  };
}

export const items: Item[] = [
  clip("a", "Last-lap overtake", "Neon Rally", 5, { seen: false, tags: ["win"] }),
  clip("b", "Triple on the ridge", "Hollow Peak", 30, { favorite: true }),
  clip("c", "Drift into the tunnel", "Neon Rally", 60 * 26, { tags: ["funny", "win"] }),
];

export function editorData(item: Item): EditorData {
  return {
    id: item.id,
    path: item.path,
    title: item.title,
    game: item.game,
    created: item.created,
    duration: item.duration,
    width: item.width,
    height: item.height,
    tracks: item.tracks.map((t) => ({ ...t, audio: `${FIXTURES}/tone.webm` })),
    filmstrip: `${FIXTURES}/strip.jpg`,
    draft: item.draft,
    markers: item.markers ?? [],
  };
}

export interface Fixture {
  settings: Settings;
  items: Item[];
  editors: Record<string, EditorData>;
  encoders: { id: string; label: string }[];
  codecs: Codec[];
}

export function fixture(overrides: Partial<Fixture> = {}): Fixture {
  const list = overrides.items ?? items;
  return {
    settings,
    items: list,
    editors: Object.fromEntries(list.map((i) => [i.id, editorData(i)])),
    encoders: [{ id: "nvenc", label: "NVIDIA NVENC" }],
    codecs: ["h264"],
    ...overrides,
  };
}

/** A command the page sent, with its arguments. */
export interface Call {
  cmd: string;
  args: Record<string, unknown>;
}

declare global {
  interface Window {
    __calls: Call[];
    __emit: (event: string, payload: unknown) => void;
  }
}

/** Puts the stand-in into the page; call before `page.goto`. */
export async function mockTauri(page: Page, data: Fixture = fixture()) {
  await page.addInitScript((data: Fixture) => {
    const callbacks = new Map<number, (payload: unknown) => void>();
    const listeners = new Map<string, number[]>();
    let next = 1;
    let settingsRevision = 1;
    const state = structuredClone(data);
    window.__calls = [];
    window.__emit = (event, payload) => {
      for (const id of listeners.get(event) ?? []) callbacks.get(id)?.({ event, id, payload });
    };

    const answer = (cmd: string, args: Record<string, any>): unknown => {
      switch (cmd) {
        case "plugin:event|listen": {
          const list = listeners.get(args.event) ?? [];
          list.push(args.handler);
          listeners.set(args.event, list);
          return args.handler;
        }
        case "ffmpeg_setup":
          return { state: "ready" };
        case "get_settings":
          return [state.settings, settingsRevision];
        case "set_settings":
          // Like Replix: the newest revision it was sent is the one in effect.
          state.settings = args.settings;
          settingsRevision = Math.max(settingsRevision, args.revision);
          return settingsRevision;
        case "get_status":
          return { state: "waiting", target: null, sessionSince: null, encoder: "NVIDIA NVENC", error: null, clipSeconds: 30 };
        case "first_editor_round":
          return 1;
        case "ui_language":
          return "en-US";
        case "hotkey_conflicts":
          return [];
        case "list_library":
          return state.items;
        case "open_editor":
          return state.editors[args.id];
        case "waveform":
          return Array.from({ length: 200 }, (_, i) => 0.3 + 0.3 * Math.abs(Math.sin(i / 7)));
        case "get_encoders":
          return state.encoders;
        case "get_codecs":
          return state.codecs;
        case "list_mics":
          return [{ id: "mic", name: "USB Headset", default: true }];
        case "list_monitors":
          return [{ index: 0, width: 1920, height: 1080, primary: true }];
        case "list_apps":
          return [];
        case "storage_info":
          return { clips: 2_000_000_000, exports: 0, free: 200_000_000_000 };
        case "export_clip":
          return "/home/test/Videos/Replix/Exports/clip.mp4";
        case "tag_items": {
          // Like Replix: the clips change, and the page hears which fields did.
          const patch: [string, { tags: string[] }][] = [];
          for (const item of state.items) {
            if (!args.ids.includes(item.id)) continue;
            const gone = args.remove.map((x: string) => x.toLowerCase());
            const kept = (item.tags ?? []).filter((x) => !gone.includes(x.toLowerCase()));
            for (const x of args.add) if (!kept.some((y) => y.toLowerCase() === x.toLowerCase())) kept.push(x);
            item.tags = kept;
            patch.push([item.id, { tags: kept }]);
          }
          setTimeout(() => window.__emit("items-meta", patch));
          return null;
        }
        case "plugin:app|version":
          return "0.0.0-test";
        default:
          return null;
      }
    };

    (window as any).__TAURI_INTERNALS__ = {
      metadata: { currentWindow: { label: "main" }, currentWebview: { windowLabel: "main", label: "main" } },
      plugins: {},
      convertFileSrc: (path: string) => path,
      transformCallback: (callback: (payload: unknown) => void) => {
        const id = next++;
        callbacks.set(id, callback);
        return id;
      },
      unregisterCallback: (id: number) => callbacks.delete(id),
      invoke: async (cmd: string, args: Record<string, unknown> = {}) => {
        if (!cmd.startsWith("plugin:event")) window.__calls.push({ cmd, args: structuredClone(args) });
        return answer(cmd, args);
      },
    };
    (window as any).__TAURI_EVENT_PLUGIN_INTERNALS__ = {
      unregisterListener: (event: string, id: number) =>
        listeners.set(
          event,
          (listeners.get(event) ?? []).filter((x) => x !== id),
        ),
    };
  }, data);
}

/** The commands sent so far with this name. */
export async function calls(page: Page, cmd: string): Promise<Call[]> {
  return (await page.evaluate(() => window.__calls)).filter((c) => c.cmd === cmd);
}
