<script lang="ts">
  import { onMount } from "svelte";
  import { api, type AppWindow, type InputDevice, type Settings, type Source } from "../lib/api";
  import { lengthLabel } from "../lib/format";
  import { store } from "../lib/store.svelte";
  import Icon from "./Icon.svelte";
  import MicMeter from "./MicMeter.svelte";
  import Buffer from "./onboarding/Buffer.svelte";
  import Hotkeys from "./onboarding/Hotkeys.svelte";
  import Keys from "./onboarding/Keys.svelte";
  import Mic from "./onboarding/Mic.svelte";
  import Ready from "./onboarding/Ready.svelte";
  import Sources from "./onboarding/Sources.svelte";
  import Tracks from "./onboarding/Tracks.svelte";
  import Segmented from "./Segmented.svelte";
  import Select from "./Select.svelte";
  import Switch from "./Switch.svelte";

  // First start: one idea per step, each with its own picture.
  const steps = ["welcome", "clip", "source", "tracks", "mic", "keys", "ready"] as const;
  let step = $state(0);
  let mics = $state<InputDevice[]>([]);
  let apps = $state<AppWindow[]>([]);

  const s = $derived(store.settings!);
  const current = $derived(steps[step]);
  const keys = (combo: string) => (combo ? combo.split("+") : ["Not set"]);
  const micDevice = $derived(store.settings?.micDevice ?? null);
  const micOptions = $derived([
    { value: null, label: "Windows default", hint: mics.find((m) => m.default)?.name },
    ...mics.map((m) => ({ value: m.id as string | null, label: m.name })),
  ]);
  const appOptions = $derived([
    ...apps.map((a) => ({ value: a.exe as string | null, label: a.name, hint: a.title })),
    ...(s.appExe && !apps.some((a) => a.exe === s.appExe) ? [{ value: s.appExe, label: s.appExe }] : []),
  ]);

  onMount(async () => {
    mics = await api.listMics();
  });

  function update(patch: Partial<Settings>) {
    store.save({ ...s, ...patch });
  }

  async function pickSource(source: Source) {
    update({ source });
    if (source === "app") apps = await api.listApps();
  }

  function next() {
    if (step < steps.length - 1) step += 1;
    else finish();
  }

  function back() {
    if (step > 0) step -= 1;
  }

  function finish() {
    update({ onboarded: true });
    store.go({ name: "library", filter: "all", game: null });
  }

  function onKey(e: KeyboardEvent) {
    if ((e.target as HTMLElement).closest("input, textarea, [role='listbox'], [aria-haspopup]")) return;
    if (e.key === "ArrowRight" || e.key === "Enter") {
      if (e.key === "Enter" && (e.target as HTMLElement).closest("button")) return;
      e.preventDefault();
      next();
    } else if (e.key === "ArrowLeft") {
      e.preventDefault();
      back();
    }
  }

  const sources: { value: Source; title: string; text: string; icon: string }[] = [
    { value: "game", title: "Games", text: "Starts by itself when a game is in front. Nothing else is recorded.", icon: "game" },
    { value: "screen", title: "Whole screen", text: "Everything on your main display.", icon: "desktop" },
    { value: "app", title: "One app", text: "Pick a program and Replix records only that.", icon: "app" },
  ];
</script>

<svelte:window onkeydown={onKey} />

<section class="onboarding" aria-label="Set up Replix">
  <div class="frame">
    <div class="stage">
      {#key current}
        <div class="scene">
          {#if current === "welcome"}
            <Buffer seconds={s.bufferSeconds} />
          {:else if current === "clip"}
            <Keys keys={keys(s.hotkeys.clip)} seconds={s.bufferSeconds} />
          {:else if current === "source"}
            <Sources source={s.source} />
          {:else if current === "tracks"}
            <Tracks />
          {:else if current === "mic"}
            <Mic denoise={s.micNoiseSuppression} on={s.recordMic} />
          {:else if current === "keys"}
            <Hotkeys
              rows={[
                { keys: keys(s.hotkeys.clip), icon: "clips", taken: store.conflicts.includes(s.hotkeys.clip) },
                { keys: keys(s.hotkeys.screenshot), icon: "camera", taken: store.conflicts.includes(s.hotkeys.screenshot) },
                { keys: keys(s.hotkeys.session), icon: "record", taken: store.conflicts.includes(s.hotkeys.session) },
              ]}
            />
          {:else}
            <Ready />
          {/if}
        </div>
      {/key}
    </div>

    <div class="content">
      <div class="top">
        <div class="progress" role="progressbar" aria-label="Setup" aria-valuemin={1} aria-valuemax={steps.length} aria-valuenow={step + 1}>
          {#each steps as name, i (name)}
            <span class:done={i <= step}></span>
          {/each}
        </div>
        {#if step < steps.length - 1}
          <button type="button" class="skip" onclick={finish}>Skip setup</button>
        {/if}
      </div>

      {#key current}
        <div class="copy">
          {#if current === "welcome"}
            <h1>Never miss the moment</h1>
            <p>
              Replix quietly keeps the last few seconds of what you play. When something great happens, you keep it. Nothing
              to start, nothing to stop.
            </p>
          {:else if current === "clip"}
            <h1>One key, and it's saved</h1>
            <p>
              Press <kbd>{keys(s.hotkeys.clip).join(" + ")}</kbd> in any game. Replix saves what just happened, and the game keeps
              running.
            </p>
            <div class="field">
              <span class="label">How far back?</span>
              <Segmented
                label="Clip length"
                value={s.bufferSeconds}
                options={[15, 30, 60, 120].map((n) => ({ value: n, label: lengthLabel(n) }))}
                onchange={(v) => update({ bufferSeconds: v })}
              />
            </div>
          {:else if current === "source"}
            <h1>What should Replix watch?</h1>
            <p>Most people leave this on games. You can change it any time.</p>
            <div class="choices" role="radiogroup" aria-label="What to record">
              {#each sources as option (option.value)}
                <button
                  type="button"
                  class="choice"
                  role="radio"
                  aria-checked={s.source === option.value}
                  onclick={() => pickSource(option.value)}
                >
                  <span class="choice-icon"><Icon name={option.icon} /></span>
                  <span class="choice-text"><b>{option.title}</b><span>{option.text}</span></span>
                  <span class="mono mark">{s.source === option.value ? "[×]" : "[ ]"}</span>
                </button>
              {/each}
            </div>
            {#if s.source === "app"}
              <Select
                label="App to record"
                placeholder="Choose an open app…"
                value={s.appExe}
                options={appOptions}
                onopen={async () => (apps = await api.listApps())}
                onchange={(v) => update({ appExe: v })}
              />
            {/if}
          {:else if current === "tracks"}
            <h1>Every sound gets its own track</h1>
            <p>
              The game, Discord, Spotify and your mic are kept apart. Later you can mute your friends or the music and keep the
              rest. Any app that plays sound gets a track by itself.
            </p>
          {:else if current === "mic"}
            <h1>Is your mic on?</h1>
            <p>Say something. If the bar moves, you're good.</p>
            <div class="rows">
              <div class="line">
                <span>Record my voice</span>
                <Switch checked={s.recordMic} label="Record my voice" onchange={(v) => update({ recordMic: v })} />
              </div>
              {#if s.recordMic}
                <div class="line">
                  <span>Microphone</span>
                  <div class="pick">
                    <Select label="Microphone" value={s.micDevice} options={micOptions} onchange={(v) => update({ micDevice: v })} />
                  </div>
                </div>
                <div class="line">
                  <span>Noise suppression</span>
                  <Switch
                    checked={s.micNoiseSuppression}
                    label="Noise suppression"
                    onchange={(v) => update({ micNoiseSuppression: v })}
                  />
                </div>
                <MicMeter device={micDevice} />
              {/if}
            </div>
          {:else if current === "keys"}
            <h1>Three keys to remember</h1>
            <ul class="keylist">
              <li><span>Save a clip</span><kbd>{keys(s.hotkeys.clip).join(" + ")}</kbd></li>
              <li><span>Screenshot</span><kbd>{keys(s.hotkeys.screenshot).join(" + ")}</kbd></li>
              <li><span>Start or stop a session</span><kbd>{keys(s.hotkeys.session).join(" + ")}</kbd></li>
            </ul>
            {#if store.conflicts.length}
              <p class="warn">{store.conflicts.join(", ")} is taken by another program. Pick a different key in Settings.</p>
            {:else}
              <p>Change them any time in Settings.</p>
            {/if}
          {:else}
            <h1>You're all set</h1>
            <p>Start a game whenever you like. Replix waits in the tray and stays out of your way.</p>
            <div class="rows">
              <div class="line">
                <span>Start with Windows</span>
                <Switch checked={s.launchAtStartup} label="Start with Windows" onchange={(v) => update({ launchAtStartup: v })} />
              </div>
              <div class="line">
                <span>Notifications in the corner</span>
                <Switch checked={s.showToast} label="Notifications in the corner" onchange={(v) => update({ showToast: v })} />
              </div>
            </div>
          {/if}
        </div>
      {/key}

      <div class="nav">
        {#if step > 0}
          <button type="button" class="btn ghost" onclick={back}>Back</button>
        {/if}
        <button type="button" class="btn primary" onclick={next}>
          {current === "welcome" ? "Let's set it up" : current === "ready" ? "Open my library" : "Next"}
        </button>
      </div>
    </div>
  </div>
</section>

<style>
  .onboarding {
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: grid;
    place-items: center;
    padding: 8px 24px 24px;
  }
  .frame {
    container-type: size;
    width: min(1060px, 100%);
    height: min(600px, 100%);
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    gap: clamp(28px, 5vw, 64px);
    align-items: stretch;
  }
  .stage {
    position: relative;
    align-self: center;
    width: min(100%, 92cqh, 460px);
    aspect-ratio: 1;
    justify-self: end;
    container-type: size;
  }
  .scene {
    position: absolute;
    inset: 0;
    animation: scene 420ms var(--travel);
  }
  .content {
    min-width: 0;
    display: flex;
    flex-direction: column;
    padding: 12px 0;
    max-width: 460px;
  }
  .top {
    display: flex;
    align-items: center;
    gap: 16px;
    min-height: 36px;
  }
  .progress {
    flex: 1;
    display: flex;
    gap: 6px;
  }
  .progress span {
    flex: 1;
    height: 4px;
    border-radius: 2px;
    background: var(--surface);
    transition: background-color 300ms ease;
  }
  .progress span.done {
    background: var(--accent);
  }
  .skip {
    border: 0;
    background: transparent;
    color: var(--ghost);
    font-size: 12.5px;
    padding: 6px 4px;
  }
  .skip:hover {
    color: var(--ink);
  }
  .copy {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding-top: clamp(20px, 8cqh, 64px);
    animation: copy 360ms var(--travel);
  }
  h1 {
    margin: 0;
    font-size: clamp(24px, 2.8vw, 30px);
    line-height: 1.12;
    font-weight: 800;
    letter-spacing: -0.02em;
    text-wrap: balance;
  }
  p {
    margin: 0;
    max-width: 44ch;
    color: var(--muted);
    font-size: 14px;
    line-height: 1.55;
    text-wrap: pretty;
  }
  .warn {
    color: var(--red);
  }
  kbd {
    font-family: var(--mono);
    font-size: 12.5px;
    padding: 2px 7px;
    border-radius: 10px;
    background: var(--surface);
    color: var(--ink);
    white-space: nowrap;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-top: 6px;
  }
  .label {
    font-size: 12.5px;
    color: var(--muted);
  }
  .choices {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-top: 4px;
  }
  .choice {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 9px 12px;
    border: 0;
    border-radius: var(--r-row);
    background: var(--overlay);
    text-align: left;
    transition: background-color 160ms ease, box-shadow 160ms ease;
  }
  .choice:hover {
    background: var(--surface);
  }
  .choice[aria-checked="true"] {
    background: var(--surface);
    box-shadow: inset 0 0 0 1.5px var(--accent);
  }
  .choice-icon {
    width: 32px;
    height: 32px;
    flex: none;
    border-radius: 12px;
    background: var(--raised);
    color: var(--accent);
    display: grid;
    place-items: center;
  }
  .choice-text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .choice-text span {
    font-size: 12.5px;
    color: var(--muted);
  }
  .mark {
    font-size: 12px;
    color: var(--ghost);
  }
  [aria-checked="true"] .mark {
    color: var(--accent);
  }
  .pick {
    width: 260px;
    min-width: 0;
  }
  .rows {
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin-top: 4px;
  }
  .line {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    min-height: 40px;
  }
  .keylist {
    list-style: none;
    margin: 4px 0 0;
    padding: 0;
    display: flex;
    flex-direction: column;
  }
  .keylist li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    min-height: 48px;
    border-bottom: 1px solid var(--line);
  }
  .nav {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding-top: 16px;
  }
  .nav .btn.primary {
    min-width: 120px;
  }
  @keyframes scene {
    from {
      opacity: 0;
      transform: scale(0.97);
    }
  }
  @keyframes copy {
    from {
      opacity: 0;
      transform: translateY(10px);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .scene,
    .copy {
      animation: none;
    }
  }
</style>
