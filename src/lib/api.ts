import { convertFileSrc, invoke } from "@tauri-apps/api/core";

export type Kind = "clip" | "session" | "screenshot";
export type TrackKind = "game" | "app" | "mic" | "desktop";
export type Source = "game" | "screen" | "app";
export type Quality = "low" | "medium" | "high";
export type ToastPosition = "topLeft" | "topRight" | "bottomLeft" | "bottomRight";

export interface TrackMeta {
  name: string;
  kind: TrackKind;
  /** The program's full path, for its icon. */
  exe?: string;
}

export interface TrackEdit {
  enabled: boolean;
  volume: number;
  /** Where this track is heard, in seconds of the clip; the whole clip when left out. */
  start?: number;
  end?: number;
  /** Taken out of the clip: not shown and not exported. */
  removed?: boolean;
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
  /** Where the frame sits top to bottom, 0 to 1. */
  cropY?: number;
  /** Frame height as a share of the video's. */
  cropScale?: number;
  /** All sound together: 1 as recorded, up to 2. */
  masterVolume?: number;
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
  /** Replix could not read the file yet; its length and size follow on a later scan. */
  unread: boolean;
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
  accent: string;
  /** Background colours; see lib/themes. */
  theme: string;
  launchAtStartup: boolean;
  closeToTray: boolean;
  showToast: boolean;
  toastPosition: ToastPosition;
  toastSize: "small" | "medium" | "large";
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

let changes: Promise<unknown> = Promise.resolve();

/**
 * Library changes go to Replix one at a time, each once the one before it is done, so
 * they land in the order they were made: two quick saves of a draft never swap.
 */
function inOrder<T>(change: () => Promise<T>): Promise<T> {
  const next = changes.then(change, change);
  changes = next.catch(() => {});
  return next;
}

/** Getting FFmpeg onto the PC on first start (and after an update that pins a new one). */
export type FfmpegSetup =
  | { state: "checking" }
  | { state: "downloading"; done: number; total: number }
  | { state: "unpacking" }
  | { state: "ready" }
  | { state: "failed"; error: string };

export const api = {
  ffmpegSetup: () => invoke<FfmpegSetup>("ffmpeg_setup"),
  retryFfmpeg: () => invoke<void>("retry_ffmpeg"),
  /** The settings in effect, with their revision: a page counts on from there. */
  getSettings: () => invoke<[Settings, number]>("get_settings"),
  /** Answers with the revision in effect afterwards: this one or a later one. */
  setSettings: (settings: Settings, revision: number) => invoke<number>("set_settings", { settings, revision }),
  getStatus: () => invoke<Status>("get_status"),
  getEncoders: () => invoke<EncoderOption[]>("get_encoders"),
  listApps: () => invoke<AppWindow[]>("list_apps"),
  listMonitors: () => invoke<Monitor[]>("list_monitors"),
  listLibrary: () => invoke<Item[]>("list_library"),
  /** Opens a clip in the editor; its media work belongs to the editor round given. */
  // After the changes asked for before it, so a draft saved a moment ago opens as saved;
  // it does not hold up changes asked for after it while the clip is prepared.
  openEditor: (id: string, round: number) => changes.then(() => invoke<EditorData>("open_editor", { id, round })),
  /** Gets a clip ready for the editor in the background. */
  warmEditor: (id: string) => invoke<boolean>("warm_editor", { id }),
  /** A track's loudness over time, 0 to 1, for the editor. */
  waveform: (id: string, track: number, duration: number, round: number) =>
    invoke<number[]>("waveform", { id, track, duration, round }),
  /** Stops the media work of this editor round and the ones before it, started or not. */
  endEditorRound: (round: number) => invoke<void>("end_editor_round", { round }),
  /** Where this page load's editor rounds start. */
  firstEditorRound: () => invoke<number>("first_editor_round"),
  saveDraft: (id: string, title: string, draft: Draft) => inOrder(() => invoke<void>("save_draft", { id, title, draft })),
  renameItem: (id: string, title: string) => inOrder(() => invoke<void>("rename_item", { id, title })),
  setFavorite: (ids: string[], favorite: boolean) => inOrder(() => invoke<void>("set_favorite", { ids, favorite })),
  /** Says where the files were, for `restoreItems`. */
  deleteItems: (ids: string[]) => inOrder(() => invoke<string[]>("delete_items", { ids })),
  restoreItems: (paths: string[]) => inOrder(() => invoke<number>("restore_items", { paths })),
  revealItem: (id: string) => invoke<void>("reveal_item", { id }),
  revealPath: (path: string) => invoke<void>("reveal_path", { path }),
  openClipsFolder: () => invoke<void>("open_clips_folder"),
  /** `ticket` names this export in its "export-progress" events. */
  exportClip: (id: string, title: string, request: Draft & { preset: Preset }, ticket: number) =>
    invoke<string>("export_clip", { id, title, request, ticket }),
  /** Tells Replix whether the page has the focus; its work ahead of time goes by this. */
  reportFocus: (focused: boolean) => invoke<void>("report_focus", { focused }),
  /** Stops an export, running or still waiting for its turn. */
  cancelExport: (ticket: number) => invoke<void>("cancel_export", { ticket }),
  saveFrame: (id: string, time: number) => invoke<string>("save_frame", { id, time }),
  saveClipNow: () => invoke<void>("save_clip_now"),
  takeScreenshot: () => invoke<void>("take_screenshot"),
  toggleSession: () => invoke<void>("toggle_session"),
  previewToast: () => invoke<void>("preview_toast"),
  hotkeyConflicts: () => invoke<string[]>("hotkey_conflicts"),
  pauseHotkeys: (paused: boolean) => invoke<void>("pause_hotkeys", { paused }),
  extractTrack: (id: string, track: number, title: string, start: number, end: number) =>
    invoke<string>("extract_track", { id, track, title, start, end }),
  checkHotkey: (keys: string) => invoke<"free" | "taken" | "invalid">("check_hotkey", { keys }),
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
