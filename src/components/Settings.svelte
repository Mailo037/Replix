<script lang="ts">
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import { onMount } from "svelte";
  import {
    api,
    type AppWindow,
    type Codec,
    type EncoderOption,
    type InputDevice,
    type Monitor,
    type Settings,
    type StorageInfo,
  } from "../lib/api";
  import { lengthLabel, size } from "../lib/format";
  import { playsHevc } from "../lib/media";
  import { store } from "../lib/store.svelte";
  import { updates } from "../lib/updates.svelte";
  import Icon from "./Icon.svelte";
  import MicMeter from "./MicMeter.svelte";
  import Profiles from "./Profiles.svelte";
  import Privacy from "./Privacy.svelte";
  import { languages, t } from "../lib/i18n.svelte";
  import { tip } from "../lib/tooltip";
  import { isWindows } from "../lib/platform";
  import Segmented from "./Segmented.svelte";
  import Select from "./Select.svelte";
  import HotkeyDialog from "./HotkeyDialog.svelte";
  import { themes } from "../lib/themes";
  import Slider from "./Slider.svelte";
  import Switch from "./Switch.svelte";
  import TrackDefaults from "./TrackDefaults.svelte";

  let monitors = $state<Monitor[]>([]);
  let encoders = $state<EncoderOption[]>([]);
  let codecs = $state<Codec[]>(["h264"]);
  let apps = $state<AppWindow[]>([]);
  let choosingApp = $state(false);
  let recording = $state<keyof Settings["hotkeys"] | null>(null);

  let mics = $state<InputDevice[]>([]);

  const s = $derived(store.settings!);
  /** HEVC is offered where a graphics card records it and the editor can play it; once chosen it stays, so it can be switched back. */
  const offerHevc = $derived((codecs.includes("hevc") && playsHevc()) || s.codec === "hevc");
  // Its own derived so the mic test restarts only when the device really changes.
  const micDevice = $derived(store.settings?.micDevice ?? null);
  const defaultMic = $derived(mics.find((m) => m.default));
  const micOptions = $derived([
    { value: null, label: isWindows ? t("Windows default") : t("System default"), hint: defaultMic?.name },
    ...mics.map((m) => ({ value: m.id as string | null, label: m.name })),
    ...(s.micDevice && !mics.some((m) => m.id === s.micDevice) ? [{ value: s.micDevice, label: t("Not connected") }] : []),
  ]);
  const accents = ["#C5F26B", "#FF9EC0", "#FFC56B", "#7EF0C0", "#9FD8FF", "#C9B6FF", "#FF8A7A"];

  let storage = $state<StorageInfo | null>(null);
  const usedShare = $derived.by(() => {
    if (!storage) return 0;
    const used = storage.clips + storage.exports;
    const whole = s.storageLimitGb ? s.storageLimitGb * 1024 ** 3 : used + storage.free;
    return whole ? Math.min(1, used / whole) : 0;
  });

  onMount(async () => {
    [monitors, encoders, codecs, mics, storage] = await Promise.all([
      api.listMonitors(),
      api.getEncoders(),
      api.getCodecs(),
      api.listMics(),
      api.storageInfo(),
    ]);
  });

  const updateText = $derived.by(() => {
    const v = updates.version ? t("Version {version}", { version: updates.version }) : t("This version");
    switch (updates.phase) {
      case "checking":
        return t("Looking for a new version…");
      case "current":
        return t("{v} · up to date", { v });
      case "available":
        return t("Version {version} is ready to install", { version: updates.next?.version ?? "" });
      case "downloading":
        return updates.progress === null
          ? t("Downloading…")
          : t("Downloading {percent}%", { percent: Math.round(updates.progress * 100) });
      case "failed":
        return t("{v} · couldn't reach the update server", { v });
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
    const dir = await openDialog({ directory: true, defaultPath: s.clipsDir, title: t("Folder for clips") });
    if (typeof dir === "string") update({ clipsDir: dir });
  }

  async function showApps() {
    apps = await api.listApps();
    choosingApp = true;
  }

  const hotkeyRows: { key: keyof Settings["hotkeys"]; label: string; hint: string }[] = $derived([
    { key: "clip", label: t("Save clip"), hint: t("Keeps the last moments") },
    { key: "screenshot", label: t("Screenshot"), hint: t("Saves what's on screen now") },
    { key: "session", label: t("Start or stop session"), hint: t("Records until you press it again") },
    { key: "marker", label: t("Add marker"), hint: t("Marks a moment to find it again in the editor") },
  ]);
</script>


<section class="settings">
  {#if s}
    <div class="scroll body">
      <div class="column">
      <h1>{t("Settings")}</h1>
      <h2>{t("Recording")}</h2>
      <div class="panel group">
        <div class="row stack">
          <div class="text"><span>{t("Record")}</span><span class="meta">{t("What Replix keeps in the buffer")}</span></div>
          <Segmented
            label={t("Record")}
            value={s.source}
            options={[
              { value: "game", label: t("Games") },
              { value: "screen", label: t("Whole screen") },
              { value: "app", label: t("One app") },
            ]}
            onchange={(v) => (v === "app" && !s.appExe ? showApps() : update({ source: v }))}
          />
          {#if s.source === "game"}
            <p class="meta note">{t("Replix starts when a game is in front and stops when it closes. Nothing else is recorded.")}</p>
          {:else if s.source === "screen" && monitors.length > 1}
            <Segmented
              label={t("Display")}
              value={s.monitor}
              options={monitors.map((m) => ({
                value: m.index,
                label: m.primary ? t("Display {n} (main)", { n: m.index + 1 }) : t("Display {n}", { n: m.index + 1 }),
              }))}
              onchange={(v) => update({ monitor: v })}
            />
          {:else if s.source === "app"}
            <div class="inline">
              <span class="mono">{s.appExe ?? t("No app chosen")}</span>
              <button type="button" class="btn small" onclick={showApps}>{t("Choose app")}</button>
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
          <div class="text"><span>{t("Clip length")}</span><span class="meta">{t("How far back a clip reaches")}</span></div>
          <Segmented
            label={t("Clip length")}
            value={s.bufferSeconds}
            options={[15, 30, 60, 120, 300, 600].map((n) => ({ value: n, label: lengthLabel(n) }))}
            onchange={(v) => update({ bufferSeconds: v })}
          />
        </div>
        <div class="row stack">
          <div class="text"><span>{t("Quality")}</span><span class="meta">{t("Higher looks better and takes more space")}</span></div>
          <div class="two">
            <Segmented
              label={t("Frame rate")}
              value={s.fps}
              options={[30, 60, 120].map((n) => ({ value: n, label: t("{n} fps", { n }) }))}
              onchange={(v) => update({ fps: v })}
            />
            <Segmented
              label={t("Quality")}
              value={s.quality}
              options={[
                { value: "low", label: t("Low") },
                { value: "medium", label: t("Medium") },
                { value: "high", label: t("High") },
              ]}
              onchange={(v) => update({ quality: v })}
            />
          </div>
        </div>
        <div class="row">
          <div class="text">
            <span>{t("Encoder")}</span>
            <span class="meta">{store.status.encoder ? t("Using {encoder}", { encoder: store.status.encoder }) : t("Picked automatically")}</span>
          </div>
          <div class="pick">
            <Select
              label={t("Encoder")}
              value={s.encoder}
              options={[{ value: "auto", label: t("Automatic") }, ...encoders.map((enc) => ({ value: enc.id, label: enc.label }))]}
              onchange={(v) => update({ encoder: v })}
            />
          </div>
        </div>
        {#if offerHevc}
          <div class="row">
            <div class="text">
              <span>{t("Video format")}</span>
              <span class="meta">
                {s.codec === "hevc"
                  ? t("About half the space. Exports are still H.264, which plays everywhere.")
                  : t("H.264 plays everywhere. HEVC needs about half the space for the same picture.")}
              </span>
            </div>
            <div class="pick">
              <Segmented
                label={t("Video format")}
                value={s.codec}
                options={[
                  { value: "h264", label: "H.264" },
                  { value: "hevc", label: "HEVC" },
                ]}
                onchange={(v) => update({ codec: v })}
              />
            </div>
          </div>
        {/if}
        <div class="row">
          <div class="text"><span>{t("Mouse cursor")}</span><span class="meta">{t("Show the pointer in clips")}</span></div>
          <Switch checked={s.captureCursor} label={t("Show mouse cursor")} onchange={(v) => update({ captureCursor: v })} />
        </div>
      </div>

      <h2>{t("Microphone")}</h2>
      <div class="panel group">
        <div class="row">
          <div class="text"><span>{t("Record microphone")}</span><span class="meta">{t("Your voice on its own track")}</span></div>
          <Switch checked={s.recordMic} label={t("Record microphone")} onchange={(v) => update({ recordMic: v })} />
        </div>
        {#if s.recordMic}
          <div class="row">
            <div class="text">
              <span>{t("Device")}</span>
              <span class="meta">{s.micDevice ? t("Stays on this mic") : defaultMic ? (isWindows ? t("Follows Windows · {device}", { device: defaultMic.name }) : t("Follows the system · {device}", { device: defaultMic.name })) : isWindows ? t("Follows Windows") : t("Follows the system")}</span>
            </div>
            <div class="pick wide">
              <Select
                label={t("Microphone")}
                value={s.micDevice}
                options={micOptions}
                onopen={refreshMics}
                onchange={(v) => update({ micDevice: v })}
              />
            </div>
          </div>
          <div class="row">
            <div class="text"><span>{t("Volume")}</span><span class="meta">{t("How loud your voice is in clips")}</span></div>
            <div class="volume">
              <Slider
                label={t("Microphone volume")}
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
            <div class="text"><span>{t("Noise suppression")}</span><span class="meta">{t("Filters out fans, hum and keyboard clatter")}</span></div>
            <Switch
              checked={s.micNoiseSuppression}
              label={t("Noise suppression")}
              onchange={(v) => update({ micNoiseSuppression: v })}
            />
          </div>
          <div class="row stack">
            <div class="text"><span>{t("Test")}</span></div>
            <MicMeter device={micDevice} />
          </div>
        {/if}
      </div>

      <h2>{t("Track volume")}</h2>
      <p class="meta intro">{t("How loud each track starts when you open a clip in the editor. A saved draft keeps its own.")}</p>
      <TrackDefaults />

      <h2>{t("Games & apps")}</h2>
      <p class="meta intro">{t("Give a game or app its own settings. Anything you add here also counts as a game.")}</p>
      <Profiles />

      <h2>{t("Privacy")}</h2>
      <p class="meta intro">
        {t("These programs never end up in a recording. Replix doesn't record them as a game or app, gives their sound no track, and on the whole screen it turns only their windows black, while the rest of the screen is recorded as usual.")}
      </p>
      <Privacy />

      <h2>{t("Hotkeys")}</h2>
      <div class="panel group">
        {#each hotkeyRows as h (h.key)}
          <div class="row">
            <div class="text">
              <span>{h.label}</span>
              {#if store.conflicts.includes(s.hotkeys[h.key])}
                <span class="meta taken">{t("Used by another program. Pick a different key.")}</span>
              {:else}
                <span class="meta">{h.hint}</span>
              {/if}
            </div>
            <button
              type="button"
              class="key"
              class:listening={recording === h.key}
              class:taken={store.conflicts.includes(s.hotkeys[h.key])}
              aria-label={t("Change the hotkey for {name}", { name: h.label })}
              onclick={() => (recording = h.key)}
            >
              {s.hotkeys[h.key] || t("Not set")}
            </button>
          </div>
        {/each}
      </div>

      {#if recording}
        {@const row = hotkeyRows.find((r) => r.key === recording)!}
        <HotkeyDialog
          label={row.label}
          current={s.hotkeys[row.key]}
          others={hotkeyRows.filter((r) => r.key !== row.key).map((r) => ({ label: r.label, keys: s.hotkeys[r.key] }))}
          onclose={() => (recording = null)}
          onsave={(keys) => {
            update({ hotkeys: { ...s.hotkeys, [row.key]: keys } });
            recording = null;
          }}
        />
      {/if}

      <h2>{t("Storage")}</h2>
      <div class="panel group">
        {#if storage}
          <div class="row stack">
            <div class="text">
              <span>{t("Space used")}</span>
              <span class="meta">
                {t("{clips} in clips · {exports} in exports · {free} free on this drive", { clips: size(storage.clips), exports: size(storage.exports), free: size(storage.free) })}
              </span>
            </div>
            <div
              class="usage"
              role="img"
              aria-label={s.storageLimitGb
                ? t("{percent}% of the limit used", { percent: Math.round(usedShare * 100) })
                : t("{percent}% of the drive used", { percent: Math.round(usedShare * 100) })}
            >
              <span style:width="{Math.max(usedShare * 100, 0.8)}%"></span>
            </div>
            {#if s.storageLimitGb}
              <span class="meta">{t("{percent}% of the {limit} GB limit", { percent: Math.round(usedShare * 100), limit: s.storageLimitGb })}</span>
            {/if}
          </div>
        {/if}
        <div class="row">
          <div class="text"><span>{t("Clips folder")}</span><span class="meta path mono">{s.clipsDir}</span></div>
          <div class="inline">
            <button type="button" class="icon-btn" aria-label={t("Open folder")} use:tip={t("Open folder")} onclick={() => api.openClipsFolder()}><Icon name="folder" /></button>
            <button type="button" class="btn small" onclick={chooseFolder}>{t("Change")}</button>
          </div>
        </div>
        <div class="row stack">
          <div class="text">
            <span>{t("Space limit")}</span>
            <span class="meta">{isWindows ? t("When full, the oldest clips go to the recycle bin. Favorites, drafts and imported videos stay.") : t("When full, the oldest clips go to the trash. Favorites, drafts and imported videos stay.")}</span>
          </div>
          <Segmented
            label={t("Space limit")}
            value={s.storageLimitGb}
            options={[0, 25, 50, 100, 250].map((n) => ({ value: n, label: n ? t("{n} GB", { n }) : t("No limit") }))}
            onchange={(v) => update({ storageLimitGb: v })}
          />
        </div>
      </div>

      <h2>{t("Appearance")}</h2>
      <div class="panel group">
        <div class="row">
          <div class="text"><span>{t("Accent colour")}</span></div>
          <div class="swatches" role="group" aria-label={t("Accent colour")}>
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
        <div class="row">
          <div class="text"><span>{t("Background")}</span></div>
          <div class="swatches" role="group" aria-label={t("Background")}>
            {#each themes as theme (theme.id)}
              <button
                type="button"
                class="swatch theme"
                class:on={(s.theme || "plum") === theme.id}
                style:background="linear-gradient(135deg, {theme.colors.canvas} 50%, {theme.colors.raised} 50%)"
                aria-label={theme.name}
                aria-pressed={(s.theme || "plum") === theme.id}
                use:tip={theme.name}
                onclick={() => update({ theme: theme.id })}
              ></button>
            {/each}
          </div>
        </div>
      </div>

      <h2>{t("General")}</h2>
      <div class="panel group">
        <div class="row">
          <div class="text"><span>{t("Language")}</span></div>
          <div class="pick">
            <Select
              label={t("Language")}
              value={s.language ?? "auto"}
              options={[{ value: "auto", label: isWindows ? t("Same as Windows") : t("Same as the system") }, ...languages.map((l) => ({ value: l.code as string, label: l.name }))]}
              onchange={(v) => update({ language: v })}
            />
          </div>
        </div>
        <div class="row">
          <div class="text">
            <span>{t("Notifications")}</span>
            <span class="meta">{t("Messages like “Clip saved” in a corner of your screen. Off shows none at all.")}</span>
          </div>
          <Switch checked={s.showToast} label={t("Notifications")} onchange={(v) => update({ showToast: v })} />
        </div>
        {#if s.showToast}
          <div class="row stack">
            <div class="text"><span>{t("Notification corner")}</span><span class="meta">{t("Where on the screen they show up")}</span></div>
            <Segmented
              label={t("Notification corner")}
              value={s.toastPosition}
              options={[
                { value: "topLeft", label: t("Top left") },
                { value: "topRight", label: t("Top right") },
                { value: "bottomLeft", label: t("Bottom left") },
                { value: "bottomRight", label: t("Bottom right") },
              ]}
              onchange={(v) => store.save({ ...s, toastPosition: v }).then((saved) => void (saved && api.previewToast()))}
            />
          </div>
          <div class="row">
            <div class="text"><span>{t("Notification size")}</span></div>
            <Segmented
              label={t("Notification size")}
              value={s.toastSize ?? "medium"}
              options={[
                { value: "small", label: t("Small") },
                { value: "medium", label: t("Medium") },
                { value: "large", label: t("Large") },
              ]}
              onchange={(v) => store.save({ ...s, toastSize: v }).then((saved) => void (saved && api.previewToast()))}
            />
          </div>
        {/if}
        <div class="row">
          <div class="text"><span>{isWindows ? t("Start with Windows") : t("Start when you log in")}</span><span class="meta">{t("Opens quietly in the tray")}</span></div>
          <Switch checked={s.launchAtStartup} label={isWindows ? t("Start with Windows") : t("Start when you log in")} onchange={(v) => update({ launchAtStartup: v })} />
        </div>
        <div class="row">
          <div class="text"><span>{t("Keep running when closed")}</span><span class="meta">{t("Closing the window leaves Replix recording in the tray")}</span></div>
          <Switch checked={s.closeToTray} label={t("Keep running when closed")} onchange={(v) => update({ closeToTray: v })} />
        </div>
        <div class="row">
          <div class="text"><span>{t("Updates")}</span><span class="meta" role="status">{updateText}</span></div>
          {#if updates.phase === "available" || updates.phase === "downloading"}
            <button type="button" class="btn small primary" disabled={updates.phase === "downloading"} onclick={() => updates.install()}>
              {updates.phase === "downloading" ? t("Updating…") : t("Update")}
            </button>
          {:else}
            <button type="button" class="btn small" disabled={updates.phase === "checking"} onclick={() => updates.check()}>{t("Check now")}</button>
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
    margin: 4px 12px 8px;
    font-size: 30px;
    font-weight: 800;
    letter-spacing: -0.035em;
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
    margin: 22px 8px 10px;
    font-size: 19px;
    font-weight: 800;
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
    min-height: 52px;
    padding: 8px 14px;
    font-weight: 600;
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
    height: 32px;
    padding: 0 14px;
    border: 0;
    border-radius: 999px;
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
    width: 26px;
    height: 26px;
    border: 0;
    border-radius: 9px;
    transition: transform 160ms var(--travel), box-shadow 160ms ease;
  }
  .swatch.on {
    box-shadow: 0 0 0 2px var(--overlay), 0 0 0 4px var(--ink);
  }
  .swatch.theme {
    box-shadow: inset 0 0 0 1px rgb(255 255 255 / 0.18);
  }
  .swatch.theme.on {
    box-shadow: inset 0 0 0 1px rgb(255 255 255 / 0.18), 0 0 0 2px var(--overlay), 0 0 0 4px var(--ink);
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
    border-radius: 14px;
    background: transparent;
    font-weight: 600;
    text-align: left;
  }
  .apps button:hover {
    background: var(--surface);
  }
</style>
