<script lang="ts">
  import { api, type AppWindow, type Monitor, type Source } from "../lib/api";
  import { lengthLabel } from "../lib/format";
  import { store } from "../lib/store.svelte";
  import { ticker } from "../lib/ticker.svelte";
  import { pop } from "../lib/motion";
  import Icon from "./Icon.svelte";
  import Segmented from "./Segmented.svelte";
  import Switch from "./Switch.svelte";
  import { t } from "../lib/i18n.svelte";

  // Recording state as a small pill in the title bar; the menu holds the quick controls.
  let open = $state(false);
  let picking = $state(false);
  let apps = $state<AppWindow[]>([]);
  let monitors = $state<Monitor[]>([]);

  // Displays are read each time the menu opens, so a plugged-in screen shows up.
  $effect(() => {
    if (!open) return;
    api.listMonitors().then((m) => (monitors = m)).catch(() => (monitors = []));
  });
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

  async function update(patch: Partial<NonNullable<typeof store.settings>>) {
    if (!store.settings) return;
    await store.save({ ...store.settings, ...patch });
  }

  async function pickSource(source: Source) {
    if (source === "app") {
      apps = await api.listApps();
      picking = true;
      return;
    }
    picking = false;
    await update({ source });
  }

  function close() {
    open = false;
    picking = false;
  }
</script>

<svelte:window
  onkeydown={(e) => e.key === "Escape" && close()}
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
        {#if picking}
          <ul class="scroll app-list">
            {#each apps as a (a.exe)}
              <li>
                <button
                  type="button"
                  class="choice"
                  onclick={() => {
                    picking = false;
                    update({ source: "app", appExe: a.exe });
                  }}
                >
                  <span class="choice-text"><span>{a.name}</span><span class="meta">{a.title}</span></span>
                </button>
              </li>
            {:else}
              <li class="meta empty">{t("No open windows found.")}</li>
            {/each}
          </ul>
          <button type="button" class="btn ghost small" onclick={() => (picking = false)}>{t("Back")}</button>
        {:else}
          {#each [{ v: "game", l: t("Games (automatic)") }, { v: "screen", l: t("Whole screen") }, { v: "app", l: settings.source === "app" ? t("One app · {app}", { app: appLabel }) : t("One app…") }] as o (o.v)}
            <button type="button" class="choice" aria-pressed={settings.source === o.v} onclick={() => pickSource(o.v as Source)}>
              <span>{o.l}</span>
              <span class="mono mark" class:on={settings.source === o.v}>{settings.source === o.v ? "[×]" : "[ ]"}</span>
            </button>
            {#if o.v === "screen" && settings.source === "screen" && monitors.length > 1}
              <div class="displays">
                {#each monitors as m (m.index)}
                  <button
                    type="button"
                    class="choice sub"
                    aria-pressed={settings.monitor === m.index}
                    onclick={() => update({ monitor: m.index })}
                  >
                    <span class="choice-text">
                      <span>{m.primary ? t("Display {n} (main)", { n: m.index + 1 }) : t("Display {n}", { n: m.index + 1 })}</span>
                      <span class="meta">{m.width} × {m.height}</span>
                    </span>
                    <span class="mono mark" class:on={settings.monitor === m.index}>{settings.monitor === m.index ? "(•)" : "( )"}</span>
                  </button>
                {/each}
              </div>
            {/if}
          {/each}
        {/if}
      </div>
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
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .choice {
    width: 100%;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    min-height: 36px;
    padding: 6px 12px;
    border: 0;
    border-radius: 14px;
    background: transparent;
    color: var(--muted);
    font-weight: 600;
    text-align: left;
  }
  .choice:hover,
  .choice[aria-pressed="true"] {
    background: var(--surface);
    color: var(--ink);
  }
  .choice-text {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .choice-text .meta {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .mark {
    font-size: 12px;
    color: var(--ghost);
  }
  .mark.on {
    color: var(--accent);
  }
  .displays {
    display: flex;
    flex-direction: column;
    margin: 2px 0 4px 12px;
  }
  .choice.sub {
    min-height: 32px;
    font-size: 12.5px;
  }
  .app-list {
    max-height: 220px;
    margin-bottom: 6px;
  }
  .empty {
    padding: 8px 10px;
  }
</style>
