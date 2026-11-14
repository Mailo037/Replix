<script lang="ts">
  import { tick } from "svelte";
  import { api, type AppWindow, type Monitor, type Settings } from "../lib/api";
  import { appIcon } from "../lib/appIcons.svelte";
  import { lengthLabel } from "../lib/format";
  import { store } from "../lib/store.svelte";
  import { ticker } from "../lib/ticker.svelte";
  import { pop } from "../lib/motion";
  import Icon from "./Icon.svelte";
  import Segmented from "./Segmented.svelte";
  import Switch from "./Switch.svelte";
  import { t } from "../lib/i18n.svelte";

  // Recording state as a small pill in the title bar; the menu holds the quick controls. What to
  // record is a short list of three; screens and apps show their choices in a submenu beside it,
  // screens with a picture of what each one shows, so they can be told apart.
  type Sub = "screen" | "app";
  let open = $state(false);
  /** The submenu shown beside the menu, if any. */
  let sub = $state<Sub | null>(null);
  let apps = $state<AppWindow[] | null>(null);
  let monitors = $state<Monitor[]>([]);
  /** What each display showed when last looked at, by index; kept so a submenu opened again shows them at once. */
  const pictures = $state<Record<number, string>>({});
  let picturesAt = 0;
  const rows = $state<Partial<Record<Sub, HTMLButtonElement>>>({});
  let flyout = $state<HTMLElement>();
  let hovering = 0;

  // Displays are read each time the menu opens, so a plugged-in screen shows up.
  $effect(() => {
    if (!open) return;
    api
      .listMonitors()
      .then((m) => {
        monitors = m;
        if (sub === "screen") lookAtScreens();
      })
      .catch(() => (monitors = []));
  });

  /**
   * Takes a fresh picture of every display. Each one runs FFmpeg for a moment, so moving the
   * pointer back and forth asks at most every few seconds.
   */
  function lookAtScreens() {
    if (!monitors.length || Date.now() - picturesAt < 3000) return;
    picturesAt = Date.now();
    for (const m of monitors) {
      api
        .monitorPreview(m.index)
        .then((url) => {
          if (url) pictures[m.index] = url;
        })
        .catch(() => {});
    }
  }

  async function openSub(which: Sub | null, focusFirst = false) {
    clearTimeout(hovering);
    sub = which;
    if (which === "screen") lookAtScreens();
    if (which === "app") api.listApps().then((a) => (apps = a)).catch(() => (apps = []));
    if (which && focusFirst) {
      await tick();
      flyout?.querySelector<HTMLElement>("button")?.focus();
    }
  }

  /** Resting on a row opens its submenu (or closes another one) after a moment. */
  function hover(which: Sub | null) {
    clearTimeout(hovering);
    if (which === "screen") lookAtScreens();
    if (sub === which) return;
    hovering = window.setTimeout(() => openSub(which), sub ? 150 : 250);
  }

  function closeSub() {
    const which = sub;
    sub = null;
    if (which) rows[which]?.focus();
  }

  function onRowKey(which: Sub, e: KeyboardEvent) {
    if (e.key !== "ArrowLeft") return;
    e.preventDefault();
    openSub(which, true);
  }

  function onFlyoutKey(e: KeyboardEvent) {
    const items = [...(flyout?.querySelectorAll<HTMLElement>("button") ?? [])];
    const at = items.indexOf(document.activeElement as HTMLElement);
    if (e.key === "ArrowDown") items[(at + 1) % items.length]?.focus();
    else if (e.key === "ArrowUp") items[(at - 1 + items.length) % items.length]?.focus();
    else if (e.key === "Home") items[0]?.focus();
    else if (e.key === "End") items[items.length - 1]?.focus();
    else if (e.key === "ArrowRight" || e.key === "Escape") closeSub();
    else return;
    e.preventDefault();
    e.stopPropagation();
  }

  /** Puts the submenu level with its row, moved up if it would run out of the window. */
  function beside(node: HTMLElement, row: HTMLElement | undefined) {
    let anchor = row;
    const place = () => {
      if (!anchor) return;
      const top = anchor.offsetTop - 8;
      node.style.top = `${top}px`;
      const over = node.getBoundingClientRect().bottom - (innerHeight - 8);
      if (over > 0) node.style.top = `${top - over}px`;
    };
    place();
    const resize = new ResizeObserver(place);
    resize.observe(node);
    return {
      update(next: HTMLElement | undefined) {
        anchor = next;
        place();
      },
      destroy: () => resize.disconnect(),
    };
  }

  const displayName = (m: Monitor) => (m.primary ? t("Display {n} (main)", { n: m.index + 1 }) : t("Display {n}", { n: m.index + 1 }));
  const sameExe = (a: string | null | undefined, b: string) => !!a && a.toLowerCase() === b.toLowerCase();
  let now = $state(Date.now());
  let root = $state<HTMLElement>();

  $effect(() => {
    // The clock is read off the start time, so it is right again when the window returns.
    if (!store.status.sessionSince || !store.visible) return;
    now = Date.now();
    const timer = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(timer);
  });

  // The level bars and the session dot move a few times a second, like a real meter.
  /** Screens are recorded under the English name "Desktop" or "Desktop 2"; shown in the page's language. */
  const shown = (target: string) => target.replace(/^Desktop(?= |$)/, t("Desktop"));

  const LEVELS = [40, 55, 70, 85, 100, 85, 70, 55];
  const OFFSETS = [2, 6, 3, 1];
  const beat = ticker(200, () => store.status.state === "recording" || !!store.status.sessionSince);

  const settings = $derived(store.settings);
  const appLabel = $derived(settings?.appExe?.replace(/\.exe$/i, "") ?? t("the app"));

  const headline = $derived.by(() => {
    const s = store.status;
    if (s.sessionSince) {
      const secs = Math.max(0, Math.floor((now - s.sessionSince) / 1000));
      return t("Session {time}", { time: `${Math.floor(secs / 60)}:${String(secs % 60).padStart(2, "0")}` });
    }
    switch (s.state) {
      case "starting":
        return t("Getting ready");
      case "off":
        return t("Paused");
      case "waiting":
        return settings?.source === "app" ? t("Waiting for the app") : t("Waiting for a game");
      case "recording":
        return s.target ? t("Recording {target}", { target: shown(s.target) }) : t("Recording");
      case "error":
        return t("Can't record");
    }
  });

  const detail = $derived.by(() => {
    const s = store.status;
    if (s.sessionSince) {
      return s.target
        ? t("Recording {target}. Press the session key to stop.", { target: shown(s.target) })
        : t("Recording. Press the session key to stop.");
    }
    switch (s.state) {
      case "starting":
        return t("Testing your graphics card");
      case "off":
        return t("Turn recording on to keep clips");
      case "waiting":
        return settings?.source === "app" ? t("Open {app} to start", { app: appLabel }) : t("Start a game to begin");
      case "recording":
        return s.target
          ? t("Keeping the last {length} of {target}", { length: lengthLabel(s.clipSeconds ?? settings?.bufferSeconds ?? 30), target: shown(s.target) })
          : t("Keeping the last {length} of the screen", { length: lengthLabel(s.clipSeconds ?? settings?.bufferSeconds ?? 30) });
      case "error":
        return s.error ?? t("Something went wrong");
    }
  });

  async function update(patch: Partial<Settings>) {
    if (!store.settings) return;
    await store.save({ ...store.settings, ...patch });
  }

  /** A choice from a submenu: it closes, the menu stays open with the choice in its row. */
  function choose(patch: Partial<Settings>) {
    closeSub();
    update(patch);
  }

  const chosenMonitor = $derived(monitors.find((m) => m.index === settings?.monitor));

  function close() {
    clearTimeout(hovering);
    open = false;
    sub = null;
  }
</script>

<svelte:window
  onkeydown={(e) => e.key === "Escape" && (sub ? closeSub() : close())}
  onpointerdown={(e) => open && !root?.contains(e.target as Node) && close()}
/>

<div class="status-menu" bind:this={root}>
  <button
    type="button"
    class="pill"
    class:open
    aria-expanded={open}
    aria-label={t("Recording: {headline}", { headline })}
    onclick={() => (open ? close() : (open = true))}
  >
    {#if store.status.state === "recording" && !store.status.sessionSince}
      <span class="bars" aria-hidden="true"
        >{#each OFFSETS as offset, i (i)}<i style:height="{LEVELS[(beat.n + offset) % LEVELS.length]}%"></i>{/each}</span
      >
    {:else}
      <span
        class="dot"
        class:session={!!store.status.sessionSince}
        class:dim={!!store.status.sessionSince && beat.n % 6 >= 3}
        class:error={store.status.state === "error"}
      ></span>
    {/if}
    <span>{headline}</span>
    <Icon name="chevronDown" size={14} />
  </button>

  {#if open && settings}
    <div class="popover" role="dialog" aria-label={t("Recording")} transition:pop>
      <div class="pop-row">
        <span class="pop-title">
          <span>{t("Recording")}</span>
          <span class="meta">{detail}</span>
        </span>
        <Switch checked={settings.enabled} label={t("Recording")} onchange={(v) => update({ enabled: v })} />
      </div>
      <div>
        <div class="pop-label">{t("Clip length")}</div>
        <Segmented
          label={t("Clip length")}
          track="var(--overlay)"
          value={settings.bufferSeconds}
          options={[15, 30, 60, 120].map((n) => ({ value: n, label: lengthLabel(n) }))}
          onchange={(v) => update({ bufferSeconds: v })}
        />
      </div>
      <div>
        <div class="pop-label">{t("Record")}</div>
        <div class="sources">
          <button
            type="button"
            class="source"
            class:chosen={settings.source === "game"}
            aria-pressed={settings.source === "game"}
            onpointerenter={() => hover(null)}
            onclick={() => {
              openSub(null);
              update({ source: "game" });
            }}
          >
            <span class="source-icon"><Icon name="game" size={16} /></span>
            <span class="source-text"><span>{t("Games")}</span><span class="meta">{t("Automatic")}</span></span>
            <span class="tick">{#if settings.source === "game"}<Icon name="check" size={16} />{/if}</span>
            <span class="more"></span>
          </button>
          <button
            bind:this={rows.screen}
            type="button"
            class="source"
            class:chosen={settings.source === "screen"}
            aria-haspopup="menu"
            aria-expanded={sub === "screen"}
            onpointerenter={() => hover("screen")}
            onkeydown={(e) => onRowKey("screen", e)}
            onclick={(e) => (sub === "screen" ? (sub = null) : openSub("screen", e.detail === 0))}
          >
            <span class="source-icon"><Icon name="desktop" size={16} /></span>
            <span class="source-text">
              <span>{t("Whole screen")}</span>
              <span class="meta">{chosenMonitor ? displayName(chosenMonitor) : t("Display {n}", { n: settings.monitor + 1 })}</span>
            </span>
            <span class="tick">{#if settings.source === "screen"}<Icon name="check" size={16} />{/if}</span>
            <span class="more"><Icon name="chevronRight" size={14} /></span>
          </button>
          <button
            bind:this={rows.app}
            type="button"
            class="source"
            class:chosen={settings.source === "app"}
            aria-haspopup="menu"
            aria-expanded={sub === "app"}
            onpointerenter={() => hover("app")}
            onkeydown={(e) => onRowKey("app", e)}
            onclick={(e) => (sub === "app" ? (sub = null) : openSub("app", e.detail === 0))}
          >
            <span class="source-icon"><Icon name="app" size={16} /></span>
            <span class="source-text">
              <span>{t("One app")}</span>
              <span class="meta">{settings.appExe ? appLabel : t("Choose an open app…")}</span>
            </span>
            <span class="tick">{#if settings.source === "app"}<Icon name="check" size={16} />{/if}</span>
            <span class="more"><Icon name="chevronRight" size={14} /></span>
          </button>
        </div>
      </div>

      {#if sub === "screen"}
        <div
          bind:this={flyout}
          class="flyout"
          role="menu"
          tabindex="-1"
          aria-label={t("Whole screen")}
          use:beside={rows.screen}
          onkeydown={onFlyoutKey}
          transition:pop
        >
          <div class="pop-label">{t("Whole screen")}</div>
          {#each monitors.length ? monitors : [{ index: 0, width: 16, height: 9, primary: true }] as m (m.index)}
            {@const picked = settings.source === "screen" && settings.monitor === m.index}
            <button type="button" class="screen" class:chosen={picked} role="menuitemradio" aria-checked={picked} onclick={() => choose({ source: "screen", monitor: m.index })}>
              <span class="shot">
                {#if pictures[m.index]}
                  <img src={pictures[m.index]} alt="" />
                {:else}
                  <span class="outline" style:aspect-ratio="{m.width} / {m.height}"></span>
                {/if}
                <span class="number mono">{m.index + 1}</span>
              </span>
              <span class="source-text">
                <span>{displayName(m)}</span>
                {#if monitors.length}<span class="meta mono">{m.width} × {m.height}</span>{/if}
              </span>
              <span class="tick">{#if picked}<Icon name="check" size={16} />{/if}</span>
            </button>
          {/each}
        </div>
      {:else if sub === "app"}
        <div
          bind:this={flyout}
          class="flyout"
          role="menu"
          tabindex="-1"
          aria-label={t("One app")}
          use:beside={rows.app}
          onkeydown={onFlyoutKey}
          transition:pop
        >
          <div class="pop-label">{t("One app")}</div>
          {#if apps === null}
            <div class="waiting" aria-hidden="true"><i></i><i></i><i></i></div>
          {:else}
            <div class="scroll app-list">
              {#each apps as a (a.exe)}
                {@const picked = settings.source === "app" && sameExe(settings.appExe, a.exe)}
                {@const icon = appIcon(a.path)}
                <button type="button" class="app" class:chosen={picked} role="menuitemradio" aria-checked={picked} onclick={() => choose({ source: "app", appExe: a.exe })}>
                  <span class="app-icon">
                    {#if icon}<img src={icon} alt="" />{:else}<Icon name="app" size={16} />{/if}
                  </span>
                  <span class="source-text">
                    <span>{a.name}</span>
                    {#if a.title && a.title !== a.name}<span class="meta">{a.title}</span>{/if}
                  </span>
                  <span class="tick">{#if picked}<Icon name="check" size={16} />{/if}</span>
                </button>
              {:else}
                <div class="meta empty">{t("No open windows found.")}</div>
              {/each}
            </div>
          {/if}
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .status-menu {
    position: relative;
  }
  .pill {
    display: flex;
    align-items: center;
    gap: 9px;
    height: 30px;
    padding: 0 10px 0 12px;
    border: 0;
    border-radius: 15px;
    background: var(--surface);
    color: var(--ink);
    font-size: 13px;
    font-weight: 600;
    white-space: nowrap;
    transition: background-color 140ms ease, color 140ms ease;
  }
  .pill:hover,
  .pill.open {
    background: var(--raised);
  }
  .dot {
    width: 7px;
    height: 7px;
    flex: none;
    border-radius: 50%;
    background: var(--ghost);
  }
  /* The buffer is running: four bars that rise and fall like a level meter. */
  .bars {
    display: flex;
    align-items: flex-end;
    gap: 2px;
    height: 12px;
  }
  .bars i {
    width: 3px;
    border-radius: 2px;
    background: var(--accent);
    height: 70%;
  }
  .dot.session {
    background: #ff5a4e;
  }
  .dot.dim {
    opacity: 0.35;
  }
  .dot.error {
    background: var(--red);
  }
  .popover {
    position: absolute;
    top: calc(100% + 6px);
    right: 0;
    z-index: 30;
    width: 280px;
    padding: 14px;
    border-radius: 24px;
    background: var(--raised);
    box-shadow: 0 16px 40px rgb(0 0 0 / 0.45);
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .pop-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 0 2px;
  }
  .pop-title {
    display: flex;
    flex-direction: column;
    min-width: 0;
    font-weight: 700;
  }
  .pop-title .meta {
    font-weight: 400;
  }
  .pop-label {
    margin: 0 2px 6px;
    font-size: 12px;
    color: var(--muted);
  }
  /* What to record: one row each, with the current choice under its name. */
  .sources {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .source,
  .screen,
  .app {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px 8px 6px 6px;
    border: 0;
    border-radius: 14px;
    background: transparent;
    color: var(--ink);
    font-weight: 600;
    text-align: left;
    transition: background-color 120ms ease;
  }
  .source:hover,
  .source[aria-expanded="true"],
  .screen:hover,
  .screen:focus-visible,
  .app:hover,
  .app:focus-visible {
    background: var(--menu-point);
  }
  .source-icon,
  .app-icon {
    width: 30px;
    height: 30px;
    flex: none;
    display: grid;
    place-items: center;
    border-radius: 10px;
    background: var(--surface);
    color: var(--muted);
  }
  .source:hover .source-icon,
  .source[aria-expanded="true"] .source-icon {
    background: var(--overlay);
  }
  .chosen .source-icon {
    color: var(--accent);
  }
  .source-text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .source-text > span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .source-text .meta {
    font-weight: 400;
  }
  .tick {
    width: 16px;
    flex: none;
    display: flex;
    color: var(--accent);
  }
  .more {
    width: 14px;
    flex: none;
    display: flex;
    color: var(--ghost);
  }
  .source[aria-expanded="true"] .more {
    color: var(--accent);
  }

  /* The submenu sits beside the menu, level with its row. */
  .flyout {
    position: absolute;
    right: calc(100% + 8px);
    width: 316px;
    max-height: calc(100vh - 60px);
    padding: 10px 8px 8px;
    border-radius: 20px;
    background: var(--raised);
    box-shadow: 0 16px 40px rgb(0 0 0 / 0.45);
    display: flex;
    flex-direction: column;
    gap: 2px;
    overflow-y: auto;
  }
  .flyout .pop-label {
    margin: 0 8px 4px;
  }
  /* A display: a picture of what it shows now, so it is clear which one is which. */
  .shot {
    position: relative;
    width: 112px;
    height: 63px;
    flex: none;
    display: grid;
    place-items: center;
    border-radius: 9px;
    background: var(--black);
    overflow: hidden;
    box-shadow: inset 0 0 0 1px var(--line);
    transition: box-shadow 120ms ease;
  }
  .shot img {
    width: 100%;
    height: 100%;
    object-fit: contain;
  }
  /* Before the picture is there: the display's shape. */
  .outline {
    height: 62%;
    max-width: 80%;
    border: 1.5px solid var(--ghost);
    border-radius: 4px;
  }
  .number {
    position: absolute;
    left: 5px;
    bottom: 5px;
    min-width: 18px;
    height: 18px;
    padding: 0 5px;
    display: grid;
    place-items: center;
    border-radius: 9px;
    background: rgb(0 0 0 / 0.65);
    color: #fff;
    font-size: 11px;
    line-height: 1;
  }
  .screen.chosen .shot {
    box-shadow: 0 0 0 2px var(--accent);
  }
  /* A display's name may take two lines ("Anzeige 1 (Hauptanzeige)"); the picture is taller anyway. */
  .screen .source-text > span:first-child {
    white-space: normal;
    overflow-wrap: anywhere;
  }
  .app-list {
    display: flex;
    flex-direction: column;
    gap: 2px;
    max-height: 320px;
  }
  .app-icon img {
    width: 20px;
    height: 20px;
  }
  .empty {
    padding: 8px 10px;
  }
  /* The list of apps is on its way: three soft bars where it will be. */
  .waiting {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 6px 8px 8px;
  }
  .waiting i {
    height: 30px;
    border-radius: 10px;
    background: var(--surface);
  }
</style>
