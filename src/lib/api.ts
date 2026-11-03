import { convertFileSrc, invoke } from "@tauri-apps/api/core";

export type Kind = "clip" | "session" | "screenshot";
export type TrackKind = "game" | "app" | "mic" | "desktop";
export type Source = "game" | "screen" | "app";
export type Quality = "low" | "medium" | "high";

export interface TrackMeta {
  name: string;
  kind: TrackKind;
}

export interface TrackEdit {
  enabled: boolean;
  volume: number;
}

export interface Draft {
  trimStart: number;
  trimEnd: number;
  tracks: TrackEdit[];
  /** Ranges taken out between start and end. */
  cuts: [number, number][];
  splits: number[];
  vertical: boolean;
  cropX: number;
}

export interface Item {
  id: string;
  path: string;
  size: number;
  thumb: string | null;
  kind: Kind;
  title: string;
  game: string;
  created: number;
  duration: number;
  width: number;
  height: number;
  tracks: TrackMeta[];
  draft: Draft | null;
  favorite: boolean;
  seen: boolean;
}

export interface Hotkeys {
  clip: string;
  screenshot: string;
  session: string;
}

/** One game or app's own settings; null keeps the general one. */
export interface Profile {
  exe: string;
  name: string;
  ignore: boolean;
  bufferSeconds: number | null;
  fps: number | null;
  quality: Quality | null;
  recordMic: boolean | null;
}

export interface StorageInfo {
  clips: number;
  exports: number;
  free: number;
}

export interface InputDevice {
  id: string;
  name: string;
  default: boolean;
}

export interface Settings {
  enabled: boolean;
  bufferSeconds: number;
  source: Source;
  appExe: string | null;
  monitor: number;
  fps: number;
  quality: Quality;
  height: number;
  encoder: string;
  recordMic: boolean;
  micDevice: string | null;
  micVolume: number;
  micNoiseSuppression: boolean;
  captureCursor: boolean;
  clipsDir: string;
  storageLimitGb: number;
  hotkeys: Hotkeys;
  asciiThumbs: boolean;
  accent: string;
  launchAtStartup: boolean;
  closeToTray: boolean;
  showToast: boolean;
  profiles: Profile[];
  onboarded: boolean;
}

export type RecState = "starting" | "off" | "waiting" | "recording" | "error";

export interface Status {
  state: RecState;
  target: string | null;
  sessionSince: number | null;
  encoder: string | null;
  error: string | null;
  /** Clip length of what is being recorded, which a game's own settings can change. */
  clipSeconds: number | null;
}

export interface EditorTrack extends TrackMeta {
  audio: string;
}

export interface EditorData {
  id: string;
  path: string;
  title: string;
  game: string;
  created: number;
  duration: number;
  width: number;
  height: number;
  tracks: EditorTrack[];
  filmstrip: string | null;
  draft: Draft | null;
}

export interface AppWindow {
  exe: string;
  name: string;
  title: string;
}

export interface Monitor {
  index: number;
  width: number;
  height: number;
  primary: boolean;
}

export interface EncoderOption {
  id: string;
  label: string;
}

export type Preset = "original" | "discord" | "gif";

export const api = {
  getSettings: () => invoke<Settings>("get_settings"),
  setSettings: (settings: Settings) => invoke<void>("set_settings", { settings }),
  getStatus: () => invoke<Status>("get_status"),
  getEncoders: () => invoke<EncoderOption[]>("get_encoders"),
  listApps: () => invoke<AppWindow[]>("list_apps"),
  listMonitors: () => invoke<Monitor[]>("list_monitors"),
  listLibrary: () => invoke<Item[]>("list_library"),
  openEditor: (id: string) => invoke<EditorData>("open_editor", { id }),
  saveDraft: (id: string, title: string, draft: Draft) => invoke<void>("save_draft", { id, title, draft }),
  renameItem: (id: string, title: string) => invoke<void>("rename_item", { id, title }),
  setFavorite: (id: string, favorite: boolean) => invoke<void>("set_favorite", { id, favorite }),
  deleteItems: (ids: string[]) => invoke<void>("delete_items", { ids }),
  revealItem: (id: string) => invoke<void>("reveal_item", { id }),
  revealPath: (path: string) => invoke<void>("reveal_path", { path }),
  openClipsFolder: () => invoke<void>("open_clips_folder"),
  exportClip: (id: string, title: string, request: Draft & { preset: Preset }) =>
    invoke<string>("export_clip", { id, title, request }),
  saveFrame: (id: string, time: number) => invoke<string>("save_frame", { id, time }),
  saveClipNow: () => invoke<void>("save_clip_now"),
  takeScreenshot: () => invoke<void>("take_screenshot"),
  toggleSession: () => invoke<void>("toggle_session"),
  hotkeyConflicts: () => invoke<string[]>("hotkey_conflicts"),
  listMics: () => invoke<InputDevice[]>("list_mics"),
  storageInfo: () => invoke<StorageInfo>("storage_info"),
  /** Puts the files on the clipboard, ready to paste into Discord or a folder. */
  copyItems: (ids: string[]) => invoke<void>("copy_items", { ids }),
  copyPath: (path: string) => invoke<void>("copy_path", { path }),
  /** Uploads to a temporary host and copies the link; returns the link. */
  shareLink: (path: string) => invoke<string>("share_link", { path }),
  startMicTest: (device: string | null) => invoke<void>("start_mic_test", { device }),
  stopMicTest: () => invoke<void>("stop_mic_test"),
};

/** A local file as a URL the webview may load. */
export const fileUrl = (path: string) => convertFileSrc(path);
