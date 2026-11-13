/**
 * The editor's undo history. Each entry is the whole edit as it was before a change, as
 * JSON; the editor decides when a run of changes (dragging a handle, moving a slider) has
 * settled into one entry. Reactive, so buttons follow whether there is anything to undo.
 */
export class History {
  private undos = $state<string[]>([]);
  private redos = $state<string[]>([]);
  /** The state the history last settled on. */
  committed = $state("");

  constructor(private readonly limit = 100) {}

  get canUndo() {
    return this.undos.length > 0;
  }

  get canRedo() {
    return this.redos.length > 0;
  }

  /** Starts from `state` with nothing to undo. */
  reset(state: string) {
    this.undos = [];
    this.redos = [];
    this.committed = state;
  }

  /** Settles on `state`: the one before it can be undone, and what was undone is gone. */
  commit(state: string) {
    if (!state || state === this.committed) return;
    if (!this.committed) {
      this.committed = state;
      return;
    }
    this.undos.push(this.committed);
    if (this.undos.length > this.limit) this.undos.shift();
    this.redos = [];
    this.committed = state;
  }

  /** The state to go back to, or null. */
  undo(): string | null {
    const prev = this.undos.pop();
    if (prev === undefined) return null;
    this.redos.push(this.committed);
    this.committed = prev;
    return prev;
  }

  /** The state undone last, or null. */
  redo(): string | null {
    const next = this.redos.pop();
    if (next === undefined) return null;
    this.undos.push(this.committed);
    this.committed = next;
    return next;
  }
}
