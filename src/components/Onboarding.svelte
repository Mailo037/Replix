<script lang="ts">
  import { onDestroy, onMount, tick } from "svelte";
  import { api, type AppWindow, type InputDevice, type Settings } from "../lib/api";
  import { appChoices } from "../lib/appIcons.svelte";
  import { lengthLabel } from "../lib/format";
  import { t } from "../lib/i18n.svelte";
  import { isWindows } from "../lib/platform";
  import { store } from "../lib/store.svelte";
  import Icon from "./Icon.svelte";
  import MicMeter from "./MicMeter.svelte";
  import Segmented from "./Segmented.svelte";
  import Select from "./Select.svelte";
  import Switch from "./Switch.svelte";

  // First start, in two steps: what Replix keeps, then the one key that saves it.
  // Everything else waits in Settings.
  const steps = ["setup", "key"] as const;
  let step = $state(0);
  let mics = $state<InputDevice[]>([]);
  let apps = $state<AppWindow[]>([]);

  const s = $derived(store.settings!);
  const current = $derived(steps[step]);
  const micDevice = $derived(store.settings?.micDevice ?? null);
  const micOptions = $derived([
    { value: null, label: isWindows ? t("Windows default") : t("System default"), hint: mics.find((m) => m.default)?.name },
    ...mics.map((m) => ({ value: m.id as string | null, label: m.name })),
  ]);
  const clipKeys = $derived(s.hotkeys.clip ? s.hotkeys.clip.split("+") : [t("Not set")]);

  onMount(async () => {
    mics = await api.listMics();
  });

  function update(patch: Partial<Settings>) {
    store.save({ ...s, ...patch });
  }

  /** "One app" with none picked yet asks which one right away, as in Settings; it is recorded once picked. */
  let appSelect = $state<Select<string | null>>();
  let choosingApp = $state(false);
  const pickingApp = $derived(s.source === "app" || choosingApp);
  async function chooseSource(source: Settings["source"]) {
    if (source === "app" && !s.appExe) {
      choosingApp = true;
      await tick();
      appSelect?.show();
      return;
    }
    choosingApp = false;
    update({ source });
  }
  async function loadApps() {
    apps = await api.listApps().catch(() => apps);
  }
  $effect(() => {
    if (pickingApp) loadApps();
  });

  // Trying the key on the page shows what it does; the real one only acts in a game.
  let tried = $state(false);
  let pressed = $state(false);
  let pressTimer: ReturnType<typeof setTimeout> | undefined;
  function tryKeys() {
    clearTimeout(pressTimer);
    pressed = true;
    pressTimer = setTimeout(() => {
      pressed = false;
      tried = true;
    }, 140);
  }
  onDestroy(() => clearTimeout(pressTimer));

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
</script>

<svelte:window onkeydown={onKey} />

<section class="onboarding" aria-label={t("Set up Replix")}>
  <div class="frame">
    <div class="top">
      <div class="progress" role="progressbar" aria-label={t("Setup")} aria-valuemin={1} aria-valuemax={steps.length} aria-valuenow={step + 1}>
        {#each steps as name, i (name)}
          <span class:done={i <= step}></span>
        {/each}
      </div>
      {#if step < steps.length - 1}
        <button type="button" class="skip" onclick={finish}>{t("Skip setup")}</button>
      {/if}
    </div>

    {#key current}
      <div class="copy scroll">
        <div class="inner">
          {#if current === "setup"}
            <h1>{t("Never miss the moment")}</h1>
            <p>{t("Replix quietly keeps the last few seconds of what you play. When something great happens, you keep it. Nothing to start, nothing to stop.")}</p>
            <div class="panel card">
              <div class="row stack">
                <span class="name">{t("Record")}</span>
                <Segmented
                  label={t("Record")}
                  value={choosingApp ? "app" : s.source}
                  options={[
                    { value: "game", label: t("Games") },
                    { value: "screen", label: t("Whole screen") },
                    { value: "app", label: t("One app") },
                  ]}
                  onchange={chooseSource}
                />
                {#if pickingApp}
                  <Select
                    bind:this={appSelect}
                    label={t("App to record")}
                    placeholder={t("Choose an open app…")}
                    value={s.source === "app" ? s.appExe : null}
                    options={appChoices(apps, s.appExe)}
                    onopen={loadApps}
                    onchange={(exe) => {
                      choosingApp = false;
                      update({ source: "app", appExe: exe });
                    }}
                  />
                {:else}
                  <span class="note">
                    {s.source === "game" ? t("Starts by itself when a game is in front. Nothing else is recorded.") : t("Everything on your main display.")}
                  </span>
                {/if}
              </div>
              <div class="row">
                <span class="name">{t("Clip length")}</span>
                <Segmented
                  label={t("Clip length")}
                  value={s.bufferSeconds}
                  options={[15, 30, 60, 120].map((n) => ({ value: n, label: lengthLabel(n) }))}
                  onchange={(v) => update({ bufferSeconds: v })}
                />
              </div>
              <div class="row">
                <span class="name">{t("Record my voice")}</span>
                <Switch checked={s.recordMic} label={t("Record my voice")} onchange={(v) => update({ recordMic: v })} />
              </div>
              {#if s.recordMic}
                <div class="row">
                  <span class="name">{t("Microphone")}</span>
                  <div class="pick">
                    <Select label={t("Microphone")} value={s.micDevice} options={micOptions} onchange={(v) => update({ micDevice: v })} />
                  </div>
                </div>
                <div class="meter">
                  <MicMeter device={micDevice} />
                </div>
              {/if}
            </div>
          {:else}
            <h1>{t("One key, and it's saved")}</h1>
            <p>{t("Press it in any game. Replix saves what just happened, and the game keeps running.")}</p>
            <button type="button" class="keys" class:pressed class:tried disabled={!s.hotkeys.clip} onclick={tryKeys}>
              {#each clipKeys as key, i (i)}
                {#if i > 0}<span class="plus mono">+</span>{/if}
                <span class="cap mono">{key}</span>
              {/each}
            </button>
            <div class="result" role="status">
              {#if tried}
                <span class="saved">
                  <span class="tick"><Icon name="check" size={14} /></span>
                  {t("That's it. In a game, this saves a {length} clip.", { length: lengthLabel(s.bufferSeconds) })}
                </span>
              {:else if s.hotkeys.clip}
                <span class="hint">{t("Click the keys to try it.")}</span>
              {/if}
            </div>
            {#if store.conflicts.length}
              <p class="warn">{t("{names} is taken by another program. Pick a different key in Settings.", { names: store.conflicts.join(", ") })}</p>
            {/if}
            <div class="panel card">
              <div class="row">
                <span class="name">{isWindows ? t("Start with Windows") : t("Start when you log in")}</span>
                <Switch checked={s.launchAtStartup} label={isWindows ? t("Start with Windows") : t("Start when you log in")} onchange={(v) => update({ launchAtStartup: v })} />
              </div>
            </div>
          {/if}
        </div>
      </div>
    {/key}

    <div class="nav">
      {#if step > 0}
        <button type="button" class="btn ghost" onclick={back}>{t("Back")}</button>
      {/if}
      <button type="button" class="btn primary" onclick={next}>
        {current === "key" ? t("Open my library") : t("Next")}
      </button>
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
    width: min(520px, 100%);
    height: min(680px, 100%);
    display: flex;
    flex-direction: column;
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
    animation: copy 360ms var(--travel);
  }
  /* Centred while it fits; scrolls from the top once it doesn't. */
  .inner {
    margin: auto 0;
    padding: 24px 0 8px;
    display: flex;
    flex-direction: column;
    gap: 14px;
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
    max-width: 46ch;
    color: var(--muted);
    font-size: 14px;
    line-height: 1.55;
    text-wrap: pretty;
  }
  .warn {
    color: var(--red);
  }
  .card {
    margin-top: 6px;
    padding: 4px 20px;
    display: flex;
    flex-direction: column;
  }
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: 10px 16px;
    min-height: 56px;
    padding: 8px 0;
  }
  .row + .row {
    border-top: 1px solid var(--line);
  }
  .row.stack {
    flex-direction: column;
    align-items: stretch;
    padding: 14px 0;
  }
  .name {
    font-size: 14px;
    font-weight: 600;
  }
  .note {
    font-size: 12.5px;
    color: var(--muted);
  }
  /* Grows for a long microphone name; the label beside it gives way. */
  .pick {
    flex: 0 1 auto;
    min-width: min(240px, 100%);
    max-width: 65%;
  }
  .meter {
    padding: 0 0 16px;
  }
  .keys {
    align-self: flex-start;
    margin-top: 10px;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--ink);
  }
  .keys:disabled {
    cursor: default;
  }
  .cap {
    display: grid;
    place-items: center;
    min-width: 64px;
    height: 64px;
    padding: 0 20px;
    border-radius: 16px;
    background: var(--surface);
    box-shadow: inset 0 -4px 0 var(--black);
    font-size: 20px;
    transition:
      transform 90ms ease,
      box-shadow 90ms ease,
      background-color 160ms ease;
  }
  .keys:not(:disabled):hover .cap {
    background: var(--raised);
  }
  .keys.tried .cap {
    background: var(--raised);
    box-shadow:
      inset 0 0 0 1.5px var(--accent),
      inset 0 -4px 0 var(--black);
  }
  .keys.pressed .cap {
    transform: translateY(3px);
    box-shadow:
      inset 0 0 0 1.5px var(--accent),
      inset 0 -1px 0 var(--black);
  }
  .plus {
    font-size: 18px;
    color: var(--ghost);
  }
  .result {
    min-height: 40px;
    display: flex;
    align-items: center;
  }
  .hint {
    font-size: 12.5px;
    color: var(--ghost);
  }
  .saved {
    display: inline-flex;
    align-items: center;
    gap: 10px;
    padding: 8px 16px 8px 8px;
    border-radius: 20px;
    background: var(--surface);
    font-size: 13.5px;
    font-weight: 600;
    animation: pop 300ms var(--travel);
  }
  .tick {
    width: 24px;
    height: 24px;
    flex: none;
    border-radius: 12px;
    display: grid;
    place-items: center;
    background: var(--accent);
    color: var(--on-accent);
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
  @keyframes copy {
    from {
      opacity: 0;
      transform: translateY(10px);
    }
  }
  @keyframes pop {
    from {
      opacity: 0;
      transform: translateY(6px) scale(0.96);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .copy,
    .saved {
      animation: none;
    }
    .cap {
      transition: none;
    }
  }
</style>
