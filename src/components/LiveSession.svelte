<script lang="ts">
  import { onDestroy, untrack } from "svelte";
  import { api, fileUrl, type Marker } from "../lib/api";
  import { markerIcon, markerIcons } from "../lib/editor/labels";
  import { duration, time } from "../lib/format";
  import { t } from "../lib/i18n.svelte";
  import { store } from "../lib/store.svelte";
  import { tip } from "../lib/tooltip";
  import Icon from "./Icon.svelte";

  // The session being recorded, opened while it runs: moments are marked (also ones a little
  // while ago), named and given an icon, and they are on the session's timeline when it
  // opens in the editor. Once the session is stopped and saved, it opens from here.

  /** The session this view was opened for; it stays after the session ends, to find its save. */
  const opened = untrack(() => store.status.sessionSince);
  const running = $derived(opened !== null && store.status.sessionSince === opened);

  let now = $state(Date.now());
  $effect(() => {
    if (!store.visible || !running) return;
    now = Date.now();
    const timer = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(timer);
  });

  const elapsed = $derived(opened ? Math.max(0, (now - opened) / 1000) : 0);
  const marks = $derived<Marker[]>(store.liveMarks && store.liveMarks.since === opened ? store.liveMarks.markers : []);
  const picture = $derived(store.sessionPicture && store.sessionPicture.since === opened ? fileUrl(store.sessionPicture.path) : null);
  const title = $derived(store.status.target ? `${store.status.target} ${t("session")}` : t("Session"));

  // Pictures of the session come more often while someone watches this view.
  $effect(() => {
    if (!running) return;
    api.watchSession(true).catch(() => {});
    return () => void api.watchSession(false).catch(() => {});
  });

  /** The marker whose name and icon are being given; its time as the session has it. */
  let chosen = $state<number | null>(null);
  /** A marker asked for and not yet back: chosen once it arrives. */
  let awaited: number | null = null;

  $effect(() => {
    // Read first: the effect runs again whenever the markers change.
    const list = marks;
    if (awaited === null) return;
    const near = list.find((m) => Math.abs(m.at - awaited!) < 1.5);
    if (near) {
      chosen = near.at;
      awaited = null;
    }
  });
  // A marker taken away (here or by undoing elsewhere) is no longer chosen.
  $effect(() => {
    if (chosen !== null && !marks.some((m) => m.at === chosen)) chosen = null;
  });

  function mark(ago: number) {
    if (!running) return;
    awaited = Math.max(0, elapsed - ago);
    api.markSession(ago).catch((e) => store.notify(t("Can't set a marker"), String(e)));
  }

  /** Names typed are sent after a short pause, not on every key. */
  const typing = new Map<number, number>();
  function rename(m: Marker, name: string) {
    clearTimeout(typing.get(m.at));
    typing.set(
      m.at,
      window.setTimeout(() => {
        typing.delete(m.at);
        api.editSessionMarker(m.at, name.trim(), m.icon ?? "").catch(() => {});
      }, 300),
    );
  }
  onDestroy(() => {
    for (const timer of typing.values()) clearTimeout(timer);
  });

  function setIcon(m: Marker, icon: string) {
    api.editSessionMarker(m.at, m.name ?? "", icon === "flag" ? "" : icon).catch(() => {});
  }

  function remove(m: Marker) {
    clearTimeout(typing.get(m.at));
    api.removeSessionMarker(m.at).catch(() => {});
  }

  /**
   * Shows a marker's name, and its newer name when it comes back from Replix, but never
   * over what is being typed into the field.
   */
  function shows(node: HTMLInputElement, text: string) {
    node.value = text;
    return {
      update(next: string) {
        if (document.activeElement !== node) node.value = next;
      },
    };
  }

  // Once stopped, the session is saved; its placeholder says which item it became.
  let saveId = $state<string | null>(null);
  let savedId = $state<string | null>(null);
  let failed = $state(false);
  $effect(() => {
    if (running || opened === null) return;
    if (!saveId) {
      const save = store.pending.filter((p) => p.kind === "session" && p.created >= opened).sort((a, b) => b.created - a.created)[0];
      if (save) saveId = save.id;
      return;
    }
    const save = store.pending.find((p) => p.id === saveId);
    if (save?.item) savedId = save.item;
    else if (!save && !savedId) failed = true;
  });
  /** The saved session is in the library, so the editor can open it. */
  const ready = $derived(savedId !== null && store.items.some((i) => i.id === savedId));

  let stopping = $state(false);
  function stop() {
    stopping = true;
    api.toggleSession().catch(() => (stopping = false));
  }

  function back() {
    store.go(store.lastLibrary);
  }

  function onKey(e: KeyboardEvent) {
    const typingHere = e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement;
    if (e.key === "Escape") {
      if (typingHere) (e.target as HTMLElement).blur();
      else back();
      return;
    }
    if (typingHere || e.ctrlKey || e.metaKey || e.altKey) return;
    if (e.key.toLowerCase() === "m") {
      e.preventDefault();
      mark(0);
    }
  }

  /** How many markers the session had when it was last seen running; they go with it. */
  let kept = $state(0);
  $effect(() => {
    if (running) kept = marks.length;
  });

  /** Marks of the newest first, as they were set. */
  const newestFirst = $derived([...marks].reverse());
  const pct = (at: number) => (elapsed > 0 ? Math.min(100, (at / elapsed) * 100) : 0);
</script>

<svelte:window onkeydown={onKey} />

<section class="live">
  <header class="top">
    <button type="button" class="icon-btn back" aria-label={t("Back to clips")} use:tip={t("Back to clips · Esc")} onclick={back}>
      <Icon name="back" size={16} />
    </button>
    <div class="heading">
      <h1 class="title">{title}</h1>
      {#if opened}<span class="meta">{t("Started {time}", { time: time(opened) })}</span>{/if}
    </div>
    {#if running}
      <span class="rec" role="timer" aria-label={t("Recording for {time}", { time: duration(elapsed) })}>
        <span class="dot" class:dim={store.focused && Math.floor(elapsed) % 2 === 1} aria-hidden="true"></span>
        <span class="mono">{duration(elapsed)}</span>
      </span>
      <button type="button" class="btn small ghost stop" disabled={stopping} onclick={stop}>
        <span class="square" aria-hidden="true"></span>{t("Stop")}
      </button>
    {/if}
  </header>

  {#if running}
    <div class="stage">
      <div class="picture">
        {#if picture && store.visible}
          <img src={picture} alt={t("The session a few seconds ago")} />
        {:else}
          <p class="meta empty-picture">{t("A picture of the session shows up here after a few seconds.")}</p>
        {/if}
      </div>

      <aside class="panel">
        <button type="button" class="btn primary mark-now" onclick={() => mark(0)} use:tip={t("Mark this moment · M")}>
          <Icon name="flag" size={18} />{t("Mark this moment")}
        </button>
        <div class="ago" role="group" aria-label={t("Missed it? Mark a moment before now")}>
          <span class="meta">{t("Missed it?")}</span>
          {#each [10, 30, 60] as ago (ago)}
            <button type="button" class="chip" disabled={elapsed < 1} onclick={() => mark(ago)}>
              {ago < 60 ? t("{n} s ago", { n: ago }) : t("{n} min ago", { n: ago / 60 })}
            </button>
          {/each}
        </div>

        {#if newestFirst.length}
          <ol class="list" aria-label={t("Marked moments")}>
            {#each newestFirst as m (m.at)}
              <li class:chosen={chosen === m.at}>
                <div class="item">
                  <button
                    type="button"
                    class="icon"
                    aria-label={t("Icon")}
                    aria-expanded={chosen === m.at}
                    use:tip={t("Choose an icon")}
                    onclick={() => (chosen = chosen === m.at ? null : m.at)}
                  >
                    <Icon name={markerIcon(m)} size={16} />
                  </button>
                  <span class="at mono">{duration(m.at)}</span>
                  <input
                    class="name"
                    aria-label={t("Marker name")}
                    placeholder={t("Name this moment")}
                    maxlength="80"
                    spellcheck="false"
                    use:shows={m.name ?? ""}
                    onfocus={() => (chosen = m.at)}
                    oninput={(e) => rename(m, e.currentTarget.value)}
                    onkeydown={(e) => e.key === "Enter" && e.currentTarget.blur()}
                  />
                  <button type="button" class="remove" aria-label={t("Remove marker")} use:tip={t("Remove marker")} onclick={() => remove(m)}>
                    <Icon name="trash" size={16} />
                  </button>
                </div>
                {#if chosen === m.at}
                  <div class="icons" role="radiogroup" aria-label={t("Icon")}>
                    {#each markerIcons() as icon (icon.name)}
                      <button
                        type="button"
                        role="radio"
                        aria-checked={markerIcon(m) === icon.name}
                        aria-label={icon.label}
                        use:tip={icon.label}
                        onclick={() => setIcon(m, icon.name)}
                      >
                        <Icon name={icon.name} size={16} />
                      </button>
                    {/each}
                  </div>
                {/if}
              </li>
            {/each}
          </ol>
        {:else}
          <p class="meta hint">{t("Marked moments show up here, and on the session's timeline in the editor.")}</p>
        {/if}
      </aside>
    </div>

    <div class="strip" aria-hidden="true">
      <span class="start mono">0:00</span>
      <div class="lane">
        {#each marks as m (m.at)}
          <span class="flag" class:chosen={chosen === m.at} style:left="{pct(m.at)}%"><Icon name={markerIcon(m)} size={12} /></span>
        {/each}
        <span class="nowline"></span>
      </div>
      <span class="end mono">{duration(elapsed)}</span>
    </div>
  {:else}
    <div class="done">
      {#if opened === null}
        <h2>{t("No session is recording")}</h2>
        <p class="meta">{t("Start one with the session key; it shows up here while it runs.")}</p>
        <button type="button" class="btn small ghost" onclick={back}>{t("Back to clips")}</button>
      {:else if failed}
        <h2>{t("The session could not be saved")}</h2>
        <button type="button" class="btn small ghost" onclick={back}>{t("Back to clips")}</button>
      {:else if ready && savedId}
        <h2>{t("Session saved")}</h2>
        <p class="meta">
          {kept ? t("Your markers are on its timeline.") : t("Find it in your library under Sessions.")}
        </p>
        <div class="done-actions">
          <button type="button" class="btn small ghost" onclick={back}>{t("Back to clips")}</button>
          <button type="button" class="btn small primary" onclick={() => store.go({ name: "editor", id: savedId! })}>{t("Open in the editor")}</button>
        </div>
      {:else}
        <h2>{t("Saving session…")}</h2>
        <div class="bar" aria-hidden="true"><span class="busy pending-loop"></span></div>
        <p class="meta">{t("It opens in the editor from here once it's saved.")}</p>
      {/if}
    </div>
  {/if}
</section>

<style>
  .live {
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .top {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 40px;
  }
  .back {
    background: transparent;
    color: var(--muted);
  }
  .back:hover {
    background: var(--surface);
    color: var(--ink);
  }
  .heading {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: baseline;
    gap: 10px;
  }
  .title {
    margin: 0;
    font-size: 22px;
    font-weight: 800;
    letter-spacing: -0.025em;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .rec {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 12px;
    border-radius: 999px;
    background: var(--overlay);
    font-size: 13px;
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--red);
    transition: opacity 300ms ease;
  }
  .dot.dim {
    opacity: 0.35;
  }
  .stop {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .stop:hover:not(:disabled) {
    background: var(--red);
    color: var(--on-sticker);
  }
  .square {
    width: 9px;
    height: 9px;
    border-radius: 2px;
    background: currentColor;
  }

  .stage {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(0, 1fr) 320px;
    gap: 12px;
  }
  .picture {
    min-height: 0;
    border-radius: 20px;
    overflow: hidden;
    background: var(--black);
    display: grid;
    place-items: center;
    outline: 2px solid var(--red);
    outline-offset: 3px;
  }
  .picture img {
    width: 100%;
    height: 100%;
    object-fit: contain;
  }
  .empty-picture {
    max-width: 260px;
    padding: 0 16px;
    text-align: center;
  }
  .panel {
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 12px;
    border-radius: 24px;
    background: var(--overlay);
  }
  .mark-now {
    height: 44px;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    font-size: 14px;
  }
  .ago {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
  }
  .ago .meta {
    margin-right: 2px;
    font-size: 12.5px;
  }
  .chip {
    height: 28px;
    padding: 0 11px;
    border: 0;
    border-radius: 999px;
    background: var(--surface);
    color: var(--ink);
    font-size: 12.5px;
    font-weight: 600;
  }
  .chip:hover:not(:disabled) {
    background: var(--hover);
  }
  .chip:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .list {
    flex: 1;
    min-height: 0;
    margin: 4px 0 0;
    padding: 0;
    list-style: none;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .list li {
    border-radius: 14px;
  }
  .list li.chosen {
    background: var(--black);
  }
  .item {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 3px;
  }
  .icon,
  .remove {
    width: 30px;
    height: 30px;
    flex: none;
    padding: 0;
    display: grid;
    place-items: center;
    border: 0;
    border-radius: 10px;
    background: transparent;
    color: var(--lavender);
  }
  .remove {
    color: var(--ghost);
  }
  .icon:hover,
  .remove:hover {
    background: var(--hover);
    color: var(--ink);
  }
  .at {
    width: 7ch;
    flex: none;
    font-size: 12px;
    color: var(--muted);
  }
  .name {
    flex: 1;
    min-width: 0;
    height: 30px;
    padding: 0 10px;
    border: 0;
    border-radius: 10px;
    background: transparent;
    color: var(--ink);
    font: inherit;
    font-size: 13px;
    font-weight: 600;
  }
  .name:hover {
    background: var(--surface);
  }
  .name:focus-visible {
    background: var(--surface);
    outline: 2px solid var(--lavender);
    outline-offset: -2px;
  }
  .icons {
    display: grid;
    grid-template-columns: repeat(9, 1fr);
    gap: 2px;
    padding: 0 6px 6px;
  }
  .icons button {
    height: 28px;
    padding: 0;
    display: grid;
    place-items: center;
    border: 0;
    border-radius: 10px;
    background: transparent;
    color: var(--muted);
  }
  .icons button:hover {
    background: var(--hover);
    color: var(--ink);
  }
  .icons button[aria-checked="true"] {
    background: var(--surface);
    color: var(--lavender);
  }
  .hint {
    margin: 6px 4px 0;
    font-size: 12.5px;
  }

  /* The whole session so far, now at the right end; markers sit where they were set. */
  .strip {
    flex: none;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 14px;
    border-radius: 20px;
    background: var(--overlay);
    font-size: 11.5px;
    color: var(--ghost);
  }
  .lane {
    position: relative;
    flex: 1;
    height: 26px;
    border-radius: 10px;
    background: var(--black);
  }
  .flag {
    position: absolute;
    top: 50%;
    width: 20px;
    height: 20px;
    margin: -10px 0 0 -10px;
    display: grid;
    place-items: center;
    border-radius: 6px;
    color: var(--lavender);
  }
  .flag.chosen {
    background: var(--surface);
    color: var(--ink);
  }
  .nowline {
    position: absolute;
    top: 3px;
    bottom: 3px;
    right: 0;
    width: 2px;
    border-radius: 1px;
    background: var(--red);
  }

  .done {
    margin: auto;
    width: min(380px, 100%);
    padding: 22px 24px;
    border-radius: 24px;
    background: var(--overlay);
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .done h2 {
    margin: 0;
    font-size: 15px;
  }
  .done p {
    margin: 0;
  }
  .done-actions {
    display: flex;
    justify-content: flex-end;
    gap: 6px;
    margin-top: 4px;
  }
  .bar {
    position: relative;
    height: 6px;
    border-radius: 3px;
    background: var(--black);
    overflow: hidden;
  }
  .busy {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 30%;
    border-radius: 3px;
    background: var(--accent);
    animation: slide 1.4s ease-in-out infinite;
  }
  @keyframes slide {
    from {
      left: -30%;
    }
    to {
      left: 100%;
    }
  }

  @media (max-width: 760px) {
    .stage {
      grid-template-columns: minmax(0, 1fr);
      grid-template-rows: minmax(140px, 1fr) auto;
    }
  }
</style>
