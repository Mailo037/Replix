<script lang="ts">
  import { untrack } from "svelte";
  import { api } from "../lib/api";
  import { t } from "../lib/i18n.svelte";

  // Picks a hotkey: shows the keys as they are held, and says right away whether the
  // combination is free, used by another Replix action, or taken by another program.
  let {
    label,
    current,
    others,
    onsave,
    onclose,
  }: {
    /** "Save clip" */
    label: string;
    current: string;
    /** The other Replix hotkeys, by what they do. */
    others: { label: string; keys: string }[];
    onsave: (keys: string) => void;
    onclose: () => void;
  } = $props();

  const KEY_NAMES: Record<string, string> = {
    Space: "Space",
    Enter: "Enter",
    Tab: "Tab",
    Backquote: "`",
    Minus: "-",
    Equal: "=",
    Comma: ",",
    Period: ".",
    Slash: "/",
    Semicolon: ";",
    Quote: "'",
    BracketLeft: "[",
    BracketRight: "]",
    Backslash: "\\",
    Insert: "Insert",
    Delete: "Delete",
    Home: "Home",
    End: "End",
    PageUp: "PageUp",
    PageDown: "PageDown",
    PrintScreen: "PrintScreen",
    ArrowUp: "Up",
    ArrowDown: "Down",
    ArrowLeft: "Left",
    ArrowRight: "Right",
  };

  /** Shortcuts Windows or almost every program already uses. */
  const COMMON: Record<string, string> = $derived({
    "Alt+F4": t("closes the window you're in"),
    "Alt+Tab": t("switches windows"),
    "Alt+Space": t("opens the window menu"),
    "Alt+Enter": t("switches many games to full screen"),
    "Ctrl+C": t("copies"),
    "Ctrl+V": t("pastes"),
    "Ctrl+X": t("cuts"),
    "Ctrl+Z": t("undoes"),
    "Ctrl+Y": t("redoes"),
    "Ctrl+A": t("selects everything"),
    "Ctrl+S": t("saves"),
    "Ctrl+W": t("closes tabs"),
    "Ctrl+T": t("opens tabs"),
    "Ctrl+F": t("searches"),
    "Ctrl+Tab": t("switches tabs"),
    "Ctrl+Shift+Escape": t("opens the Task Manager"),
    "Alt+Z": t("opens the NVIDIA overlay"),
    "Alt+F9": t("records with NVIDIA ShadowPlay"),
    "Alt+F10": t("saves an NVIDIA instant replay"),
    "Shift+Tab": t("opens the Steam overlay"),
    "F12": t("takes a Steam screenshot"),
  });

  function keyName(code: string): string | null {
    if (code.startsWith("Key")) return code.slice(3);
    if (code.startsWith("Digit")) return code.slice(5);
    if (/^F\d{1,2}$/.test(code)) return code;
    if (code.startsWith("Numpad")) return code;
    return KEY_NAMES[code] ?? null;
  }

  let dialog = $state<HTMLDialogElement>();
  /** Modifiers held right now, and the key pressed with them. */
  let held = $state<string[]>([]);
  let combo = $state(untrack(() => current));
  let hint = $state<string | null>(null);
  let check = $state<"checking" | "free" | "taken" | "invalid" | null>(null);
  let checkRun = 0;

  const keys = $derived(held.length ? held : combo ? combo.split("+") : []);
  const twin = $derived(others.find((o) => o.keys && o.keys.toLowerCase() === combo.toLowerCase()));
  const common = $derived(COMMON[combo]);
  const status = $derived.by(() => {
    if (!combo) return { tone: "idle", text: t("Press the keys you want, for example Alt and C together.") };
    if (twin) return { tone: "bad", text: t("Already used for “{label}”.", { label: twin.label }) };
    if (check === "taken") return { tone: "bad", text: t("Another program already uses this. Pick a different one.") };
    if (check === "invalid") return { tone: "bad", text: t("Windows doesn't accept this combination.") };
    if (check === "checking") return { tone: "idle", text: t("Checking…") };
    if (common) return { tone: "warn", text: t("Works, but it also {common}.", { common }) };
    if (combo === current) return { tone: "idle", text: t("This is the current hotkey.") };
    return { tone: "good", text: t("Free to use.") };
  });
  const canSave = $derived(!!combo && combo !== current && status.tone !== "bad" && check !== "checking");

  $effect(() => {
    if (!dialog) return;
    dialog.showModal();
    api.pauseHotkeys(true).catch(() => {});
    return () => {
      api.pauseHotkeys(false).catch(() => {});
    };
  });

  async function probe(keys: string) {
    const run = ++checkRun;
    check = "checking";
    const result = await api.checkHotkey(keys).catch(() => "invalid" as const);
    if (run === checkRun) check = result;
  }

  function modifiers(e: KeyboardEvent) {
    return [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && "Super"].filter(Boolean) as string[];
  }

  function onKeyDown(e: KeyboardEvent) {
    // Tab and Enter on the buttons still work when no modifier is held.
    const mods = modifiers(e);
    if (!mods.length && (e.key === "Tab" || (e.key === "Enter" && e.target instanceof HTMLButtonElement))) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.key === "Escape" && !mods.length) return onclose();
    const key = keyName(e.code);
    if (!key) {
      // Only modifiers so far: show them as they are held.
      held = mods;
      hint = null;
      return;
    }
    held = [];
    // Single letters need a modifier, or typing in a game would trigger them.
    if (!mods.length && !/^F\d/.test(key) && !key.startsWith("Numpad") && key !== "PrintScreen") {
      hint = t("{key} alone would fire while you type. Hold Ctrl, Alt or Shift with it.", { key });
      return;
    }
    hint = null;
    combo = [...mods, key].join("+");
    if (twin) check = null;
    else probe(combo);
  }

  function onKeyUp(e: KeyboardEvent) {
    if (held.length) held = modifiers(e);
  }
</script>

<dialog
  bind:this={dialog}
  class="hotkey-dialog"
  aria-label={t("Hotkey for {label}", { label })}
  onkeydown={onKeyDown}
  onkeyup={onKeyUp}
  oncancel={(e) => {
    e.preventDefault();
    onclose();
  }}
  onclick={(e) => e.target === dialog && onclose()}
>
  <div class="box">
    <div class="head">
      <span class="meta">{t("Hotkey for")}</span>
      <h2>{label}</h2>
    </div>

    <div class="stage" class:live={held.length > 0} aria-live="polite">
      {#if keys.length}
        {#each keys as k, i (i)}
          {#if i > 0}<span class="plus">+</span>{/if}
          <kbd>{k}</kbd>
        {/each}
        {#if held.length}<span class="plus">+</span><kbd class="ghost">…</kbd>{/if}
      {:else}
        <span class="meta">{t("Press keys…")}</span>
      {/if}
    </div>

    <p class="status {hint ? 'warn' : status.tone}">{hint ?? status.text}</p>

    <div class="foot">
      <button type="button" class="btn ghost small" onclick={() => onsave("")} disabled={!current}>{t("Turn off")}</button>
      <span class="spacer"></span>
      <button type="button" class="btn ghost small" onclick={onclose}>{t("Cancel")}</button>
      <button type="button" class="btn small primary" disabled={!canSave} onclick={() => onsave(combo)}>{t("Save")}</button>
    </div>
  </div>
</dialog>

<style>
  .hotkey-dialog {
    padding: 0;
    border: 0;
    border-radius: var(--r-panel);
    background: var(--raised);
    color: var(--ink);
    box-shadow:
      0 0 0 1px rgb(255 255 255 / 0.05),
      0 24px 60px rgb(0 0 0 / 0.55);
    width: min(440px, calc(100vw - 32px));
  }
  .hotkey-dialog::backdrop {
    background: rgb(0 0 0 / 0.55);
  }
  .hotkey-dialog:focus {
    outline: none;
  }
  .box {
    display: flex;
    flex-direction: column;
    gap: 16px;
    padding: 22px;
  }
  .head h2 {
    margin: 0;
    font-size: 18px;
  }
  .stage {
    display: flex;
    align-items: center;
    justify-content: center;
    flex-wrap: wrap;
    gap: 8px;
    min-height: 92px;
    padding: 16px;
    border-radius: var(--r-row);
    background: var(--canvas);
    box-shadow: inset 0 0 0 2px transparent;
    transition: box-shadow 140ms ease;
  }
  .stage.live {
    box-shadow: inset 0 0 0 2px var(--accent);
  }
  kbd {
    min-width: 44px;
    padding: 9px 14px;
    border-radius: 12px;
    background: linear-gradient(var(--hover), var(--surface));
    box-shadow:
      inset 0 -3px 0 rgb(0 0 0 / 0.35),
      0 0 0 1px var(--line);
    font-family: var(--mono);
    font-size: 15px;
    font-weight: 500;
    text-align: center;
  }
  kbd.ghost {
    opacity: 0.4;
  }
  .plus {
    color: var(--ghost);
    font-family: var(--mono);
  }
  .status {
    margin: 0;
    min-height: 2.9em;
    padding: 0 2px;
    color: var(--muted);
  }
  .status.good {
    color: var(--accent);
  }
  .status.warn {
    color: #ffc56b;
  }
  .status.bad {
    color: var(--red);
  }
  .foot {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .spacer {
    flex: 1;
  }
</style>
