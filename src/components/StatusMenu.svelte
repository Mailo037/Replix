<script lang="ts">
  import { api, type AppWindow, type Source } from "../lib/api";
  import { lengthLabel } from "../lib/format";
  import { store } from "../lib/store.svelte";
  import Icon from "./Icon.svelte";
  import Segmented from "./Segmented.svelte";
  import Switch from "./Switch.svelte";

  // Recording state as a small pill in the title bar; the menu holds the quick controls.
  let open = $state(false);
  let picking = $state(false);
  let apps = $state<AppWindow[]>([]);
  let now = $state(Date.now());
  let root = $state<HTMLElement>();

  $effect(() => {
    if (!store.status.sessionSince) return;
    const timer = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(timer);
  });

  const settings = $derived(store.settings);
  const appLabel = $derived(settings?.appExe?.replace(/\.exe$/i, "") ?? "the app");

  const headline = $derived.by(() => {
    const s = store.status;
    if (s.sessionSince) {
      const secs = Math.max(0, Math.floor((now - s.sessionSince) / 1000));
      return `Session ${Math.floor(secs / 60)}:${String(secs % 60).padStart(2, "0")}`;
    }
    switch (s.state) {
      case "starting":
        return "Getting ready";
      case "off":
        return "Paused";
      case "waiting":
        return settings?.source === "app" ? "Waiting for the app" : "Waiting for a game";
      case "recording":
        return s.target ? `Recording ${s.target}` : "Recording";
      case "error":
        return "Can't record";
    }
  });

  const detail = $derived.by(() => {
    const s = store.status;
    if (s.sessionSince) return `Recording ${s.target ?? ""}. Press the session key to stop.`;
    switch (s.state) {
      case "starting":
        return "Testing your graphics card";
      case "off":
        return "Turn recording on to keep clips";
      case "waiting":
        return settings?.source === "app" ? `Open ${appLabel} to start` : "Start a game to begin";
      case "recording":
        return `Keeping the last ${lengthLabel(s.clipSeconds ?? settings?.bufferSeconds ?? 30)} of ${s.target ?? "the screen"}`;
      case "error":
        return s.error ?? "Something went wrong";
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
    aria-label="Recording: {headline}"
    onclick={() => (open ? close() : (open = true))}
  >
    <span
      class="dot"
      class:live={store.status.state === "recording"}
      class:session={!!store.status.sessionSince}
      class:error={store.status.state === "error"}
    ></span>
    <span>{headline}</span>
    <Icon name="chevronDown" size={14} />
  </button>

  {#if open && settings}
    <div class="popover" role="dialog" aria-label="Recording">
      <div class="pop-row">
        <span class="pop-title">
          <span>Recording</span>
          <span class="meta">{detail}</span>
        </span>
        <Switch checked={settings.enabled} label="Recording" onchange={(v) => update({ enabled: v })} />
      </div>
      <div>
        <div class="pop-label">Clip length</div>
        <Segmented
          label="Clip length"
          track="var(--overlay)"
          value={settings.bufferSeconds}
          options={[15, 30, 60, 120].map((n) => ({ value: n, label: lengthLabel(n) }))}
          onchange={(v) => update({ bufferSeconds: v })}
        />
      </div>
      <div>
        <div class="pop-label">Record</div>
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
              <li class="meta empty">No open windows found.</li>
            {/each}
          </ul>
          <button type="button" class="btn ghost small" onclick={() => (picking = false)}>Back</button>
        {:else}
          {#each [{ v: "game", l: "Games (automatic)" }, { v: "screen", l: "Whole screen" }, { v: "app", l: settings.source === "app" ? `One app · ${appLabel}` : "One app…" }] as o (o.v)}
            <button type="button" class="choice" aria-pressed={settings.source === o.v} onclick={() => pickSource(o.v as Source)}>
              <span>{o.l}</span>
              <span class="mono mark" class:on={settings.source === o.v}>{settings.source === o.v ? "[×]" : "[ ]"}</span>
            </button>
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
    gap: 8px;
    height: 26px;
    padding: 0 8px 0 10px;
    border: 0;
    border-radius: 13px;
    background: transparent;
    color: var(--muted);
    font-size: 12px;
    font-weight: 500;
    white-space: nowrap;
    transition: background-color 140ms ease, color 140ms ease;
  }
  .pill:hover,
  .pill.open {
    background: var(--surface);
    color: var(--ink);
  }
  .dot {
    width: 7px;
    height: 7px;
    flex: none;
    border-radius: 50%;
    background: var(--ghost);
  }
  .dot.live {
    background: var(--accent);
    animation: breathe 2.4s ease-in-out infinite;
  }
  .dot.session {
    background: #ff5a4e;
    animation: breathe 1.2s ease-in-out infinite;
  }
  .dot.error {
    background: var(--red);
  }
  @keyframes breathe {
    50% {
      opacity: 0.35;
    }
  }
  .popover {
    position: absolute;
    top: calc(100% + 6px);
    right: 0;
    z-index: 30;
    width: 272px;
    padding: 12px;
    border-radius: 18px;
    background: var(--raised);
    box-shadow: 0 16px 40px rgb(0 0 0 / 0.45);
    display: flex;
    flex-direction: column;
    gap: 14px;
    animation: drop 180ms var(--travel);
  }
  @keyframes drop {
    from {
      opacity: 0;
      transform: translateY(-6px);
    }
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
    font-weight: 500;
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
    padding: 6px 10px;
    border: 0;
    border-radius: 10px;
    background: transparent;
    color: var(--muted);
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
  .app-list {
    max-height: 220px;
    margin-bottom: 6px;
  }
  .empty {
    padding: 8px 10px;
  }
  @media (prefers-reduced-motion: reduce) {
    .dot.live,
    .dot.session {
      animation: none;
    }
  }
</style>
