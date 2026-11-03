import { listen } from "@tauri-apps/api/event";
import { api, type Item, type Settings, type Status } from "./api";
import { onColor } from "./format";
import { updates } from "./updates.svelte";

export type Filter = "all" | "clips" | "screenshots" | "sessions" | "drafts" | "favorites";

export type View =
  | { name: "library"; filter: Filter; game: string | null }
  | { name: "editor"; id: string }
  | { name: "settings" };

export interface NoticeAction {
  label: string;
  run: () => void;
}

export interface Notice {
  title: string;
  detail: string;
  actions: NoticeAction[];
}

class Store {
  settings = $state<Settings | null>(null);
  status = $state<Status>({ state: "starting", target: null, sessionSince: null, encoder: null, error: null, clipSeconds: null });
  items = $state<Item[]>([]);
  loaded = $state(false);
  view = $state<View>({ name: "library", filter: "all", game: null });
  notice = $state<Notice | null>(null);
  /** Hotkeys another program already owns. */
  conflicts = $state<string[]>([]);
  private noticeTimer: number | undefined;

  games = $derived.by(() => {
    const counts = new Map<string, number>();
    for (const item of this.items) {
      if (item.game) counts.set(item.game, (counts.get(item.game) ?? 0) + 1);
    }
    return [...counts.entries()].sort((a, b) => b[1] - a[1]).map(([name, count]) => ({ name, count }));
  });

  counts = $derived({
    clips: this.items.filter((i) => i.kind === "clip").length,
    screenshots: this.items.filter((i) => i.kind === "screenshot").length,
    sessions: this.items.filter((i) => i.kind === "session").length,
    drafts: this.items.filter((i) => i.draft).length,
    favorites: this.items.filter((i) => i.favorite).length,
  });

  async init() {
    const [settings, status] = await Promise.all([api.getSettings(), api.getStatus()]);
    this.settings = settings;
    this.status = status;
    this.applyAccent();
    await this.refresh();
    updates.init((version) =>
      this.notify(`Replix ${version} is ready`, "Takes a few seconds, then Replix opens again.", {
        label: "Update",
        run: () => updates.install(),
      }),
    );
    this.conflicts = await api.hotkeyConflicts();
    if (this.conflicts.length) {
      this.notify("Hotkey unavailable", `${this.conflicts.join(", ")} is used by another program.`, {
        label: "Change",
        run: () => this.go({ name: "settings" }),
      });
    }

    listen<Status>("status", (e) => (this.status = e.payload));
    listen("library-changed", () => this.refresh());
    listen<[string, string]>("thumb", (e) => {
      const [id, path] = e.payload;
      const item = this.items.find((i) => i.id === id);
      if (item) item.thumb = path;
    });
    listen<{ title: string; detail: string }>("notice", (e) => {
      // The corner toast covers the game; inside the app a quieter note is enough.
      if (document.hasFocus()) this.notify(e.payload.title, e.payload.detail);
    });
  }

  async refresh() {
    try {
      this.items = await api.listLibrary();
    } finally {
      this.loaded = true;
    }
  }

  async save(next: Settings) {
    this.settings = next;
    this.applyAccent();
    await api.setSettings($state.snapshot(next) as Settings);
    this.conflicts = await api.hotkeyConflicts();
  }

  applyAccent() {
    if (!this.settings) return;
    const root = document.documentElement.style;
    root.setProperty("--accent", this.settings.accent);
    root.setProperty("--on-accent", onColor(this.settings.accent));
  }

  notify(title: string, detail: string, action?: NoticeAction | NoticeAction[]) {
    const actions = action ? [action].flat() : [];
    this.notice = { title, detail, actions };
    window.clearTimeout(this.noticeTimer);
    this.noticeTimer = window.setTimeout(() => (this.notice = null), actions.length ? 6000 : 2600);
  }

  go(view: View) {
    this.view = view;
  }
}

export const store = new Store();
