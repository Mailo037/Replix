<script lang="ts">
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import { onMount } from "svelte";
  import {
    api,
    type AppWindow,
    type EncoderOption,
    type InputDevice,
    type Monitor,
    type Settings,
    type StorageInfo,
  } from "../lib/api";
  import { lengthLabel, size } from "../lib/format";
  import { store } from "../lib/store.svelte";
  import { updates } from "../lib/updates.svelte";
  import Icon from "./Icon.svelte";
  import MicMeter from "./MicMeter.svelte";
  import Profiles from "./Profiles.svelte";
  import { tip } from "../lib/tooltip";
  import Segmented from "./Segmented.svelte";
  import Select from "./Select.svelte";
  import Slider from "./Slider.svelte";
  import Switch from "./Switch.svelte";

  let monitors = $state<Monitor[]>([]);
  let encoders = $state<EncoderOption[]>([]);
  let apps = $state<AppWindow[]>([]);
  let choosingApp = $state(false);
  let recording = $state<keyof Settings["hotkeys"] | null>(null);

  let mics = $state<InputDevice[]>([]);

  const s = $derived(store.settings!);
  // Its own derived so the mic test restarts only when the device really changes.
  const micDevice = $derived(store.settings?.micDevice ?? null);
  const defaultMic = $derived(mics.find((m) => m.default));
  const micOptions = $derived([
    { value: null, label: "Windows default", hint: defaultMic?.name },
    ...mics.map((m) => ({ value: m.id as string | null, label: m.name })),
    ...(s.micDevice && !mics.some((m) => m.id === s.micDevice) ? [{ value: s.micDevice, label: "Not connected" }] : []),
  ]);
  const accents = ["#FF9A3C", "#FF5FD2", "#F5D547", "#3DDC97", "#5AB4FF", "#A98BFF", "#FF6B6B"];

  let storage = $state<StorageInfo | null>(null);
  const usedShare = $derived.by(() => {
    if (!storage) return 0;
    const used = storage.clips + storage.exports;
    const whole = s.storageLimitGb ? s.storageLimitGb * 1024 ** 3 : used + storage.free;
    return whole ? Math.min(1, used / whole) : 0;
  });

  onMount(async () => {
    [monitors, encoders, mics, storage] = await Promise.all([
      api.listMonitors(),
      api.getEncoders(),
      api.listMics(),
      api.storageInfo(),
    ]);
  });

  const updateText = $derived.by(() => {
    const v = updates.version ? `Version ${updates.version}` : "This version";
    switch (updates.phase) {
      case "checking":
        return "Looking for a new version…";
      case "current":
        return `${v} · up to date`;
      case "available":
        return `Version ${updates.next?.version} is ready to install`;
      case "downloading":
        return updates.progress === null ? "Downloading…" : `Downloading ${Math.round(updates.progress * 100)}%`;
      case "failed":
        return `${v} · couldn't reach the update server`;
      default:
        return v;
    }
  });

  async function refreshMics() {
    mics = await api.listMics();
  }

  function update(patch: Partial<Settings>) {
    store.save({ ...s, ...patch });
  }

  async function chooseFolder() {
    const dir = await openDialog({ directory: true, defaultPath: s.clipsDir, title: "Folder for clips" });
    if (typeof dir === "string") update({ clipsDir: dir });
  }

  async function showApps() {
    apps = await api.listApps();
    choosingApp = true;
  }

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
  };

  function keyName(code: string): string | null {
    if (code.startsWith("Key")) return code.slice(3);
    if (code.startsWith("Digit")) return code.slice(5);
    if (/^F\d{1,2}$/.test(code)) return code;
    if (code.startsWith("Numpad")) return code;
    return KEY_NAMES[code] ?? null;
  }

  function onKey(e: KeyboardEvent) {
    if (!recording) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.key === "Escape") {
      recording = null;
      return;
    }
    if (e.key === "Backspace") {
      update({ hotkeys: { ...s.hotkeys, [recording]: "" } });
      recording = null;
      return;
    }
    const key = keyName(e.code);
    if (!key) return;
    const parts = [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && "Super"].filter(Boolean);
    // Single letters need a modifier, or typing in a game would trigger them.
    if (!parts.length && !/^F\d/.test(key)) return;
    update({ hotkeys: { ...s.hotkeys, [recording]: [...parts, key].join("+") } });
    recording = null;
  }

  const hotkeyRows: { key: keyof Settings["hotkeys"]; label: string; hint: string }[] = [
    { key: "clip", label: "Save clip", hint: "Keeps the last moments" },
    { key: "screenshot", label: "Screenshot", hint: "Saves what's on screen now" },
    { key: "session", label: "Start or stop session", hint: "Records until you press it again" },
  ];
</script>

<svelte:window onkeydown={onKey} />

<section class="settings">
  {#if s}
    <div class="scroll body">
      <div class="column">
      <h1>Settings</h1>
      <h2>Recording</h2>
      <div class="panel group">
        <div class="row stack">
          <div class="text"><span>Record</span><span class="meta">What Replix keeps in the buffer</span></div>
          <Segmented
            label="Record"
            value={s.source}
            options={[
              { value: "game", label: "Games" },
              { value: "screen", label: "Whole screen" },
              { value: "app", label: "One app" },
            ]}
            onchange={(v) => (v === "app" && !s.appExe ? showApps() : update({ source: v }))}
          />
          {#if s.source === "game"}
            <p class="meta note">Replix starts when a game is in front and stops when it closes. Nothing else is recorded.</p>
          {:else if s.source === "screen" && monitors.length > 1}
            <Segmented
              label="Display"
              value={s.monitor}
              options={monitors.map((m) => ({ value: m.index, label: `Display ${m.index + 1}${m.primary ? " (main)" : ""}` }))}
              onchange={(v) => update({ monitor: v })}
            />
          {:else if s.source === "app"}
            <div class="inline">
              <span class="mono">{s.appExe ?? "No app chosen"}</span>
              <button type="button" class="btn small" onclick={showApps}>Choose app</button>
            </div>
          {/if}
          {#if choosingApp}
            <ul class="apps scroll">
              {#each apps as a (a.exe)}
                <li>
                  <button
                    type="button"
                    onclick={() => {
                      choosingApp = false;
                      update({ source: "app", appExe: a.exe });
                    }}
                  >
                    <span>{a.name}</span><span class="meta">{a.title}</span>
                  </button>
                </li>
              {/each}
            </ul>
          {/if}
        </div>
        <div class="row stack">
          <div class="text"><span>Clip length</span><span class="meta">How far back a clip reaches</span></div>
          <Segmented
            label="Clip length"
            value={s.bufferSeconds}
            options={[15, 30, 60, 120, 300, 600].map((n) => ({ value: n, label: lengthLabel(n) }))}
            onchange={(v) => update({ bufferSeconds: v })}
          />
        </div>
        <div class="row stack">
          <div class="text"><span>Quality</span><span class="meta">Higher looks better and takes more space</span></div>
          <div class="two">
            <Segmented
              label="Frame rate"
              value={s.fps}
              options={[30, 60, 120].map((n) => ({ value: n, label: `${n} fps` }))}
              onchange={(v) => update({ fps: v })}
            />
            <Segmented
              label="Quality"
              value={s.quality}
              options={[
                { value: "low", label: "Low" },
                { value: "medium", label: "Medium" },
                { value: "high", label: "High" },
              ]}
              onchange={(v) => update({ quality: v })}
            />
          </div>
        </div>
        <div class="row">
          <div class="text">
            <span>Encoder</span>
            <span class="meta">{store.status.encoder ? `Using ${store.status.encoder}` : "Picked automatically"}</span>
          </div>
          <div class="pick">
            <Select
              label="Encoder"
              value={s.encoder}
              options={[{ value: "auto", label: "Automatic" }, ...encoders.map((enc) => ({ value: enc.id, label: enc.label }))]}
              onchange={(v) => update({ encoder: v })}
            />
          </div>
        </div>
        <div class="row">
          <div class="text"><span>Mouse cursor</span><span class="meta">Show the pointer in clips</span></div>
          <Switch checked={s.captureCursor} label="Show mouse cursor" onchange={(v) => update({ captureCursor: v })} />
        </div>
      </div>

      <h2>Microphone</h2>
      <div class="panel group">
        <div class="row">
          <div class="text"><span>Record microphone</span><span class="meta">Your voice on its own track</span></div>
          <Switch checked={s.recordMic} label="Record microphone" onchange={(v) => update({ recordMic: v })} />
        </div>
        {#if s.recordMic}
          <div class="row">
            <div class="text">
              <span>Device</span>
              <span class="meta">{s.micDevice ? "Stays on this mic" : `Follows Windows${defaultMic ? ` · ${defaultMic.name}` : ""}`}</span>
            </div>
            <div class="pick wide">
              <Select
                label="Microphone"
                value={s.micDevice}
                options={micOptions}
                onopen={refreshMics}
                onchange={(v) => update({ micDevice: v })}
              />
            </div>
          </div>
          <div class="row">
            <div class="text"><span>Volume</span><span class="meta">How loud your voice is in clips</span></div>
            <div class="volume">
              <Slider
                label="Microphone volume"
                min={0}
                max={200}
                step={5}
                value={Math.round(s.micVolume * 100)}
                valuetext="{Math.round(s.micVolume * 100)}%"
                oninput={(v) => update({ micVolume: v / 100 })}
              />
              <span class="mono percent">{Math.round(s.micVolume * 100)}%</span>
            </div>
          </div>
          <div class="row">
            <div class="text"><span>Noise suppression</span><span class="meta">Filters out fans, hum and keyboard clatter</span></div>
            <Switch
              checked={s.micNoiseSuppression}
              label="Noise suppression"
              onchange={(v) => update({ micNoiseSuppression: v })}
            />
          </div>
          <div class="row stack">
            <div class="text"><span>Test</span></div>
            <MicMeter device={micDevice} />
          </div>
        {/if}
      </div>

      <h2>Games & apps</h2>
      <p class="meta intro">Give a game or app its own settings. Anything you add here also counts as a game.</p>
      <Profiles />

      <h2>Hotkeys</h2>
      <div class="panel group">
        {#each hotkeyRows as h (h.key)}
          <div class="row">
            <div class="text">
              <span>{h.label}</span>
              {#if store.conflicts.includes(s.hotkeys[h.key])}
                <span class="meta taken">Used by another program. Pick a different key.</span>
              {:else}
                <span class="meta">{h.hint}</span>
              {/if}
            </div>
            <button
              type="button"
              class="key"
              class:listening={recording === h.key}
              class:taken={store.conflicts.includes(s.hotkeys[h.key])}
              onclick={() => (recording = h.key)}
            >
              {recording === h.key ? "Press keys…" : s.hotkeys[h.key] || "Not set"}
            </button>
          </div>
        {/each}
      </div>

      <h2>Storage</h2>
      <div class="panel group">
        {#if storage}
          <div class="row stack">
            <div class="text">
              <span>Space used</span>
              <span class="meta">
                {size(storage.clips)} in clips · {size(storage.exports)} in exports · {size(storage.free)} free on this drive
              </span>
            </div>
            <div class="usage" role="img" aria-label="{Math.round(usedShare * 100)}% of {s.storageLimitGb ? 'the limit' : 'the drive'} used">
              <span style:width="{Math.max(usedShare * 100, 0.8)}%"></span>
            </div>
            {#if s.storageLimitGb}
              <span class="meta">{Math.round(usedShare * 100)}% of the {s.storageLimitGb} GB limit</span>
            {/if}
          </div>
        {/if}
        <div class="row">
          <div class="text"><span>Clips folder</span><span class="meta path mono">{s.clipsDir}</span></div>
          <div class="inline">
            <button type="button" class="icon-btn" aria-label="Open folder" use:tip={"Open folder"} onclick={() => api.openClipsFolder()}><Icon name="folder" /></button>
            <button type="button" class="btn small" onclick={chooseFolder}>Change</button>
          </div>
        </div>
        <div class="row stack">
          <div class="text">
            <span>Space limit</span>
            <span class="meta">When full, the oldest clips go to the recycle bin. Favorites and drafts stay.</span>
          </div>
          <Segmented
            label="Space limit"
            value={s.storageLimitGb}
            options={[0, 25, 50, 100, 250].map((n) => ({ value: n, label: n ? `${n} GB` : "No limit" }))}
            onchange={(v) => update({ storageLimitGb: v })}
          />
        </div>
      </div>

      <h2>Appearance</h2>
      <div class="panel group">
        <div class="row">
          <div class="text"><span>Accent colour</span></div>
          <div class="swatches" role="group" aria-label="Accent colour">
            {#each accents as c (c)}
              <button
                type="button"
                class="swatch"
                class:on={s.accent.toLowerCase() === c.toLowerCase()}
                style:background={c}
                aria-label={c}
                aria-pressed={s.accent.toLowerCase() === c.toLowerCase()}
                onclick={() => update({ accent: c })}
              ></button>
            {/each}
          </div>
        </div>
      </div>

      <h2>General</h2>
      <div class="panel group">
        <div class="row">
          <div class="text">
            <span>Notifications</span>
            <span class="meta">Messages like “Clip saved” in the top right corner of your screen. Off shows none at all.</span>
          </div>
          <Switch checked={s.showToast} label="Notifications" onchange={(v) => update({ showToast: v })} />
        </div>
        <div class="row">
          <div class="text"><span>Start with Windows</span><span class="meta">Opens quietly in the tray</span></div>
          <Switch checked={s.launchAtStartup} label="Start with Windows" onchange={(v) => update({ launchAtStartup: v })} />
        </div>
        <div class="row">
          <div class="text"><span>Keep running when closed</span><span class="meta">Closing the window leaves Replix recording in the tray</span></div>
          <Switch checked={s.closeToTray} label="Keep running when closed" onchange={(v) => update({ closeToTray: v })} />
        </div>
        <div class="row">
          <div class="text"><span>Updates</span><span class="meta" role="status">{updateText}</span></div>
          {#if updates.phase === "available" || updates.phase === "downloading"}
            <button type="button" class="btn small primary" disabled={updates.phase === "downloading"} onclick={() => updates.install()}>
              {updates.phase === "downloading" ? "Updating…" : "Update"}
            </button>
          {:else}
            <button type="button" class="btn small" disabled={updates.phase === "checking"} onclick={() => updates.check()}>Check now</button>
          {/if}
        </div>
      </div>
      </div>
    </div>
  {/if}
</section>

<style>
  .settings {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  h1 {
    margin: 6px 6px 4px;
    font-size: 16px;
    font-weight: 600;
  }
  /* Scrolls edge to edge; the content sits in a centred column. */
  .body {
    flex: 1;
    min-height: 0;
    padding: 0 16px 40px;
  }
  .column {
    max-width: 680px;
    margin: 0 auto;
  }
  h2 {
    margin: 18px 6px 8px;
    font-size: 14px;
    font-weight: 600;
  }
  .group {
    display: flex;
    flex-direction: column;
  }
  .intro {
    margin: -2px 6px 10px;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 16px;
    min-height: 48px;
    padding: 8px 12px;
  }
  .row + .row {
    border-top: 1px solid var(--line);
  }
  .row.stack {
    flex-direction: column;
    align-items: stretch;
    gap: 10px;
  }
  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .path {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .note {
    margin: 0;
  }
  .two {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 8px;
  }
  .inline {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .inline .mono {
    flex: 1;
    color: var(--muted);
  }
  .usage {
    height: 8px;
    border-radius: 4px;
    background: var(--black);
    overflow: hidden;
  }
  .usage span {
    display: block;
    height: 100%;
    border-radius: 4px;
    background: var(--accent);
    transition: width 400ms var(--travel);
  }
  .pick {
    width: 200px;
    flex: none;
  }
  .pick.wide {
    width: 280px;
  }
  .volume {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 260px;
  }
  .percent {
    width: 4ch;
    text-align: right;
    color: var(--muted);
    font-size: 12.5px;
  }
  .key {
    min-width: 110px;
    height: 30px;
    padding: 0 12px;
    border: 0;
    border-radius: 8px;
    background: var(--surface);
    font-family: var(--mono);
    font-size: 12.5px;
  }
  .key.listening {
    background: var(--accent);
    color: var(--on-accent);
  }
  .key.taken {
    color: var(--red);
    text-decoration: line-through;
  }
  .meta.taken {
    color: var(--red);
  }
  .swatches {
    display: flex;
    gap: 8px;
  }
  .swatch {
    width: 22px;
    height: 22px;
    border: 0;
    border-radius: 50%;
    transition: transform 160ms var(--travel), box-shadow 160ms ease;
  }
  .swatch.on {
    box-shadow: 0 0 0 2px var(--overlay), 0 0 0 4px var(--ink);
  }
  .swatch:active {
    transform: scale(0.9);
  }
  .apps {
    list-style: none;
    margin: 0;
    padding: 4px;
    max-height: 240px;
    border-radius: var(--r-row);
    background: var(--canvas);
  }
  .apps button {
    width: 100%;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    padding: 8px 12px;
    border: 0;
    border-radius: 10px;
    background: transparent;
    text-align: left;
  }
  .apps button:hover {
    background: var(--surface);
  }
</style>
