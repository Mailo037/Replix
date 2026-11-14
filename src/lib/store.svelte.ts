import { listen } from "@tauri-apps/api/event";
import { api, type Item, type LiveMarks, type PendingSave, type Settings, type Status } from "./api";
import { onColor } from "./format";
import { applyTheme } from "./themes";
import { syncEditorRound } from "./editorRound.svelte";
import { updates } from "./updates.svelte";
import { i18n, t } from "./i18n.svelte";

export type Filter = "all" | "clips" | "screenshots" | "sessions" | "drafts" | "favorites";

export type View =
  | { name: "library"; filter: Filter; game: string | null; tag?: string | null }
  | { name: "editor"; id: string }
  /** The session being recorded, where moments are marked while it runs. */
  | { name: "live" }
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

/** Replix is hidden or another program has the focus, say a game. */
const away = () => document.visibilityState === "hidden" || !document.hasFocus();

/**
 * Worth preparing on a hunch: a video short enough that its tracks and filmstrip are made
 * in moments. A long session's are made when it is opened. Replix checks this again.
 */
const warmable = (i: Item) => i.kind !== "screenshot" && i.duration > 0 && i.duration <= 300;

/** Whether a library list (its filter, game and tag, before any search) shows the item. */
export function inList(i: Item, filter: Filter, game: string | null, tag: string | null) {
  if (game && i.game !== game) return false;
  if (tag && !(i.tags ?? []).some((x) => x.toLowerCase() === tag.toLowerCase())) return false;
  if (filter === "clips" && i.kind !== "clip") return false;
  if (filter === "screenshots" && i.kind !== "screenshot") return false;
  if (filter === "sessions" && i.kind !== "session") return false;
  if (filter === "drafts" && !i.draft) return false;
  if (filter === "favorites" && !i.favorite) return false;
  return true;
}

class Store {
  settings = $state<Settings | null>(null);
  status = $state<Status>({ state: "starting", target: null, sessionSince: null, encoder: null, error: null, clipSeconds: null });
  items = $state<Item[]>([]);
  /** Saves on their way to the library. */
  pending = $state<PendingSave[]>([]);
  /** The saves not in the library yet, newest first: shown as placeholders. */
  placeholders = $derived.by(() => {
    const ids = new Set(this.items.map((i) => i.id));
    return this.pending.filter((p) => !p.item || !ids.has(p.item)).sort((a, b) => b.created - a.created);
  });
  loaded = $state(false);
  view = $state<View>({ name: "library", filter: "all", game: null });
  notice = $state<Notice | null>(null);
  /** The newest picture of the session being recorded, for its tile in the library. */
  sessionPicture = $state<{ since: number; path: string } | null>(null);
  /** The markers of the session being recorded; null while none records. */
  liveMarks = $state<LiveMarks | null>(null);
  /** Hotkeys another program already owns. */
  conflicts = $state<string[]>([]);
  /** The window is on screen; hidden in the tray or minimised, Replix keeps quiet. */
  visible = $state(document.visibilityState === "visible");
  /** Replix has the focus; without it a game may be running on another monitor. */
  focused = $state(document.hasFocus());
  private noticeTimer: number | undefined;

  games = $derived.by(() => {
    const counts = new Map<string, number>();
    for (const item of this.items) {
      if (item.game) counts.set(item.game, (counts.get(item.game) ?? 0) + 1);
    }
    return [...counts.entries()].sort((a, b) => b[1] - a[1]).map(([name, count]) => ({ name, count }));
  });

  /** Every tag in the library with how many clips wear it, most used first. Compared ignoring case. */
  tags = $derived.by(() => {
    const counts = new Map<string, { name: string; count: number }>();
    for (const item of this.items) {
      for (const tag of item.tags ?? []) {
        const key = tag.toLowerCase();
        const entry = counts.get(key);
        if (entry) entry.count++;
        else counts.set(key, { name: tag, count: 1 });
      }
    }
    return [...counts.values()].sort((a, b) => b.count - a.count || a.name.localeCompare(b.name));
  });

  counts = $derived.by(() => {
    const c = { clips: 0, screenshots: 0, sessions: 0, drafts: 0, favorites: 0 };
    for (const i of this.items) {
      if (i.kind === "clip") c.clips++;
      else if (i.kind === "screenshot") c.screenshots++;
      else c.sessions++;
      if (i.draft) c.drafts++;
      if (i.favorite) c.favorites++;
    }
    return c;
  });

  async init() {
    const [[settings, revision], status] = await Promise.all([api.getSettings(), api.getStatus(), syncEditorRound(), i18n.init()]);
    this.lastRevision = Math.max(this.lastRevision, revision);
    this.appliedRevision = revision;
    this.settings = settings;
    this.applied = $state.snapshot(settings) as Settings;
    this.status = status;
    this.applyAccent();
    const conflicts = api.hotkeyConflicts();
    // In the tray Replix's page is hidden, often with a game running. The library is
    // scanned, and new clips are prepared, once someone opens the window to look at them.
    document.addEventListener("visibilitychange", () => {
      this.visible = document.visibilityState === "visible";
      if (this.visible && this.stale) this.refresh();
    });
    // Files are read only while Replix is in use; coming back to it finishes what a scan
    // had to leave unread.
    // Without the focus, the background animations rest too: Replix may sit beside a game.
    // Replix goes by the same answer for its own work ahead of time.
    const rest = () => {
      this.focused = document.hasFocus();
      document.documentElement.classList.toggle("resting", !this.focused);
      api.reportFocus(this.focused).catch(() => {});
    };
    rest();
    window.addEventListener("blur", rest);
    window.addEventListener("focus", rest);
    window.addEventListener("focus", () => {
      // Only a scan cut short is repeated; a file that cannot be read at all is not
      // scanned again on every return.
      if (this.stale) this.refresh();
    });
    // Listening first, each subscription confirmed: the first scan may already report.
    await listen<Status>("status", (e) => {
      this.status = e.payload;
      if (this.sessionPicture && this.sessionPicture.since !== e.payload.sessionSince) this.sessionPicture = null;
    });
    await listen<[number, string]>("session-picture", (e) => {
      const [since, path] = e.payload;
      if (since === this.status.sessionSince) this.sessionPicture = { since, path };
    });
    let heard = false;
    await listen<LiveMarks | null>("session-marks", (e) => {
      heard = true;
      this.liveMarks = e.payload;
    });
    // Asked after listening, so a change in between is not lost; an event is newer.
    api.sessionMarks().then((marks) => heard || (this.liveMarks = marks), () => {});
    await listen("library-changed", () => this.refresh());
    // Replix says when a scan had to leave files unread. Free again right away (it was a
    // moment of lost focus), scan again now; else when Replix is back or the editor closes.
    await listen("scan-deferred", () => {
      this.stale = true;
      // At most one follow-up every few seconds, should the window keep coming and going;
      // the rest waits for a return.
      const now = performance.now();
      if (!away() && this.view.name !== "editor" && now - this.deferredAt > 5000) {
        this.deferredAt = now;
        this.refresh();
      }
    });
    // A favourite, a rename or a draft changes one item; no need to scan the folder for that.
    await listen<[string, Partial<Item>][]>("items-meta", (e) => {
      this.changes++;
      const byId = new Map(this.items.map((i) => [i.id, i]));
      for (const [id, meta] of e.payload) {
        const item = byId.get(id);
        if (item) Object.assign(item, meta);
      }
    });
    await listen<string[]>("items-removed", (e) => {
      this.changes++;
      const gone = new Set(e.payload);
      for (const id of gone) this.removed.add(id);
      if (gone.size) this.items = this.items.filter((i) => !gone.has(i.id));
    });
    await listen<[string, string]>("thumb", (e) => {
      const [id, path] = e.payload;
      const item = this.items.find((i) => i.id === id);
      if (item) item.thumb = path;
    });
    await listen<Omit<PendingSave, "preview" | "item">>("save-started", (e) => {
      this.pending = [...this.pending, { ...e.payload, preview: null, item: null }];
    });
    await listen<[string, string]>("save-preview", (e) => {
      const [id, path] = e.payload;
      const save = this.pending.find((p) => p.id === id);
      if (save) save.preview = path;
    });
    await listen<{ id: string; item: string | null }>("save-finished", (e) => {
      const { id, item } = e.payload;
      // A saved clip keeps its placeholder until the library lists it; a failed one goes.
      if (!item) this.pending = this.pending.filter((p) => p.id !== id);
      else {
        const save = this.pending.find((p) => p.id === id);
        if (save) save.item = item;
      }
    });
    await listen<{ title: string; detail: string }>("notice", (e) => {
      // The corner toast covers the game; inside the app a quieter note is enough.
      if (document.hasFocus()) this.notify(e.payload.title, e.payload.detail);
    });
    await this.refresh(true);
    updates.init((version) =>
      this.notify(t("Replix {version} is ready", { version }), t("Takes a few seconds, then Replix opens again."), {
        label: t("Update"),
        run: () => updates.install(),
      }),
    );
    this.conflicts = await conflicts;
    if (this.conflicts.length) {
      this.notify(t("Hotkey unavailable"), t("{keys} is used by another program.", { keys: this.conflicts.join(", ") }), {
        label: t("Change"),
        run: () => this.go({ name: "settings" }),
      });
    }

  }

  private refreshing: Promise<void> | null = null;
  private refreshAgain = false;
  /** Clips deleted since the current refresh began; its scans may still list them. */
  private removed = new Set<string>();
  /** The library changed while the page was hidden. */
  private stale = false;
  /** Counts changes to single items, so a scan can tell it raced one. */
  private changes = 0;

  /**
   * Scans the library. Calls while a scan runs fold into one more scan after it. While
   * Replix is hidden or another program has the focus, say a game after a saved clip, the
   * scan waits for Replix to be back; `now` scans anyway, as on start, if the page shows.
   */
  refresh(now = false): Promise<void> {
    if (document.visibilityState === "hidden" || (!now && !document.hasFocus())) {
      this.stale = true;
      return Promise.resolve();
    }
    this.stale = false;
    if (this.refreshing) {
      this.refreshAgain = true;
      return this.refreshing;
    }
    this.refreshing = (async () => {
      try {
        let passes = 0;
        this.removed.clear();
        do {
          // The window may have been hidden, or left for a game, while the last scan ran.
          if (document.visibilityState === "hidden" || (!now && !document.hasFocus())) {
            this.stale = true;
            break;
          }
          this.refreshAgain = false;
          const before = this.changes;
          const items = await api.listLibrary();
          // A change that landed while the scan ran may be missing from it: scan again
          // rather than show the older state. A run of changes does not keep it scanning:
          // after a few passes the patched items stay, and the next refresh catches up.
          if (this.changes !== before) {
            this.refreshAgain = ++passes < 3;
            if (!this.refreshAgain) {
              // Clips the scan found are shown all the same; the ones it may have read
              // before a change stay as patched, and the next refresh catches up.
              // A clip deleted meanwhile is absent too, but not new: it stays gone.
              const known = new Set(this.items.map((i) => i.id));
              const found = items.filter((i) => !known.has(i.id) && !this.removed.has(i.id));
              if (found.length) this.items = [...found, ...this.items].sort((a, b) => b.created - a.created);
              this.stale = true;
            }
          } else this.items = this.reconcile(items);
        } while (this.refreshAgain);
        // Placeholders whose clip the library now lists are done.
        const ids = new Set(this.items.map((i) => i.id));
        if (this.pending.some((p) => p.item && ids.has(p.item))) {
          this.pending = this.pending.filter((p) => !p.item || !ids.has(p.item));
        }
      } finally {
        this.loaded = true;
        this.refreshing = null;
      }
      // The newest clips are the ones most likely to be opened next.
      for (const item of this.items.filter(warmable).slice(0, 2)) this.warm(item);
    })();
    return this.refreshing;
  }

  /**
   * Keeps the objects of items that are still there and changes only what differs, so a
   * scan only updates the tiles whose data changed, not every tile in the library.
   */
  private reconcile(next: Item[]): Item[] {
    const known = new Map(this.items.map((i) => [i.id, i]));
    return next.map((fresh) => {
      const item = known.get(fresh.id);
      if (!item) return fresh;
      for (const key of Object.keys(fresh) as (keyof Item)[]) {
        const [a, b] = [item[key], fresh[key]];
        // A thumbnail that arrived while the scan ran is newer than the scan's "none yet".
        if (key === "thumb" && a && !b) continue;
        const same = a === b || (typeof b === "object" && b !== null && JSON.stringify(a) === JSON.stringify(b));
        if (!same) (item as Record<keyof Item, unknown>)[key] = b;
      }
      return item;
    });
  }

  /** The settings Replix last applied, and the revision they came from. */
  private applied: Settings | null = null;
  private appliedRevision = 0;
  /** Changes on their way, by revision, until Replix says which one is in effect. */
  private sent = new Map<number, Settings>();
  private lastRevision = 0;
  /** When a deferred scan last asked for a follow-up. */
  private deferredAt = -Infinity;

  /** Rises with every change, and across reloads: it counts on from Replix's newest. */
  private nextRevision() {
    const now = Math.floor((performance.timeOrigin + performance.now()) * 1000);
    this.lastRevision = Math.max(now, this.lastRevision + 1);
    return this.lastRevision;
  }

  /**
   * Applies settings at once and hands them to Replix straight away. A slider sends a value
   * per step; Replix applies one at a time and keeps only the newest of those waiting, so
   * the steps cost a few writes rather than one each. Says whether the newest settings were
   * applied.
   */
  save(next: Settings): Promise<boolean> {
    this.settings = next;
    this.applyAccent();
    const settings = $state.snapshot(next) as Settings;
    const change = this.nextRevision();
    this.sent.set(change, settings);
    return api.setSettings(settings, change).then(
      async (effective) => {
        // A later change may have been applied in this one's place, and answers can
        // arrive out of order: what counts is the revision Replix names.
        if (effective > this.appliedRevision && this.sent.has(effective)) {
          [this.applied, this.appliedRevision] = [this.sent.get(effective)!, effective];
        }
        for (const revision of this.sent.keys()) if (revision <= this.appliedRevision) this.sent.delete(revision);
        if (change === this.lastRevision) this.conflicts = await api.hotkeyConflicts();
        // This change or a later one is in effect; an older one means this one was dropped.
        return effective >= change;
      },
      async (e) => {
        this.sent.delete(change);
        if (change !== this.lastRevision) return false;
        // The newest settings could not be applied; say so, and show the ones in effect
        // instead of passing these off as saved. Those come from Replix itself: an earlier
        // change may have gone through while its answer is still on the way.
        this.notify(t("Settings not saved"), String(e));
        try {
          const [current, revision] = await api.getSettings();
          // A newer change made meanwhile shows what was asked for; its own answer decides.
          if (change === this.lastRevision) {
            [this.settings, this.applied] = [current, $state.snapshot(current) as Settings];
            this.appliedRevision = Math.max(this.appliedRevision, revision);
            this.applyAccent();
          }
        } catch {
          if (change === this.lastRevision && this.applied) {
            this.settings = this.applied;
            this.applyAccent();
          }
        }
        return false;
      },
    );
  }

  applyAccent() {
    if (!this.settings) return;
    const root = document.documentElement.style;
    root.setProperty("--accent", this.settings.accent);
    root.setProperty("--on-accent", onColor(this.settings.accent));
    applyTheme(this.settings.theme);
  }

  notify(title: string, detail: string, action?: NoticeAction | NoticeAction[]) {
    const actions = action ? [action].flat() : [];
    this.notice = { title, detail, actions };
    window.clearTimeout(this.noticeTimer);
    this.noticeTimer = window.setTimeout(() => (this.notice = null), actions.length ? 6000 : 2600);
  }

  private warmed = new Set<string>();

  /** Gets a clip ready for the editor in the background, once. */
  warm(item: Item) {
    const id = item.id;
    // Never while hidden: that is when a game is likely running.
    if (!warmable(item) || this.warmed.has(id) || document.visibilityState === "hidden") return;
    this.warmed.add(id);
    // Skipped (Replix in the background) or stopped: free to try again later.
    api.warmEditor(id).then(
      (prepared) => prepared || this.warmed.delete(id),
      () => this.warmed.delete(id),
    );
  }

  /** The library view last shown, so leaving the editor goes back to it. */
  lastLibrary: View = { name: "library", filter: "all", game: null };

  /**
   * The clip before (-1) or after (1) this one in the library list last shown, so the editor
   * can step through them in the order they lie there. Screenshots are passed over: they
   * don't open in the editor.
   */
  neighbour(id: string, step: -1 | 1): string | null {
    const list = this.lastLibrary;
    if (list.name !== "library") return null;
    const clips = this.items.filter((i) => i.kind !== "screenshot" && inList(i, list.filter, list.game, list.tag ?? null));
    const at = clips.findIndex((i) => i.id === id);
    return at < 0 ? null : (clips[at + step]?.id ?? null);
  }

  go(view: View) {
    const leaving = this.view.name === "editor" && view.name !== "editor";
    if (this.view.name === "library") this.lastLibrary = this.view;
    this.view = view;
    // The editor had the media turn; a scan that waited for it can finish now.
    if (leaving && this.stale) this.refresh();
  }
}

export const store = new Store();
