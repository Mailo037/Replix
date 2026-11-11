<script lang="ts">
  import type { TrackDefault } from "../lib/api";
  import { appIcon } from "../lib/appIcons.svelte";
  import { t } from "../lib/i18n.svelte";
  import { store } from "../lib/store.svelte";
  import { KINDS, withDefault } from "../lib/trackDefaults";
  import Icon from "./Icon.svelte";
  import Slider from "./Slider.svelte";

  // How each track starts out in the editor when a clip has no draft: one row per kind of
  // track, and one per program given its own in the editor's track menu.
  const s = $derived(store.settings!);
  const list = $derived(s.trackDefaults ?? []);
  const kinds: readonly string[] = KINDS;
  const programs = $derived(list.filter((d) => !kinds.includes(d.key)));

  const kindRows = $derived([
    { key: "game", label: t("Game"), hint: t("The game's own sound"), icon: "game" },
    { key: "app", label: t("Apps"), hint: t("Programs with a track of their own"), icon: "app" },
    { key: "desktop", label: t("Desktop"), hint: t("Everything else you hear"), icon: "desktop" },
    { key: "mic", label: t("Microphone"), hint: t("Your voice"), icon: "mic" },
  ]);

  const current = (key: string): TrackDefault => list.find((d) => d.key === key) ?? { key, name: "", enabled: true, volume: 1 };

  function change(key: string, patch: Partial<TrackDefault>) {
    store.save({ ...s, trackDefaults: withDefault(list, { ...current(key), ...patch }) });
  }

  function remove(key: string) {
    store.save({ ...s, trackDefaults: list.filter((d) => d.key !== key) });
  }
</script>

{#snippet controls(d: TrackDefault, name: string)}
  <button
    type="button"
    class="mute"
    class:off={!d.enabled}
    aria-pressed={!d.enabled}
    aria-label={d.enabled ? t("Mute {name}", { name }) : t("Unmute {name}", { name })}
    onclick={() => change(d.key, { enabled: !d.enabled })}
  >
    <Icon name={d.enabled ? "speaker" : "mute"} size={16} />
  </button>
  <div class="volume">
    <Slider
      label={t("{name} volume", { name })}
      min={0}
      max={100}
      step={5}
      value={Math.round(d.volume * 100)}
      valuetext="{Math.round(d.volume * 100)}%"
      disabled={!d.enabled}
      oninput={(v) => change(d.key, { volume: v / 100 })}
    />
    <span class="mono percent">{d.enabled ? `${Math.round(d.volume * 100)}%` : t("Off")}</span>
  </div>
{/snippet}

<div class="panel group">
  {#each kindRows as k (k.key)}
    <div class="track">
      <span class="initial" aria-hidden="true"><Icon name={k.icon} size={16} /></span>
      <span class="text"><span>{k.label}</span><span class="meta">{k.hint}</span></span>
      {@render controls(current(k.key), k.label)}
      <span class="forget-space"></span>
    </div>
  {/each}
  {#each programs as d (d.key)}
    {@const icon = appIcon(d.path ?? null)}
    <div class="track">
      {#if icon}
        <span class="initial icon" aria-hidden="true"><img src={icon} alt="" /></span>
      {:else}
        <span class="initial" aria-hidden="true">{(d.name || d.key).slice(0, 1).toUpperCase()}</span>
      {/if}
      <span class="text"><span>{d.name || d.key}</span><span class="meta mono">{d.key}</span></span>
      {@render controls(d, d.name || d.key)}
      <button
        type="button"
        class="forget"
        aria-label={t("Forget the default for {name}", { name: d.name || d.key })}
        onclick={() => remove(d.key)}
      >
        <Icon name="close" size={14} />
      </button>
    </div>
  {/each}
  <p class="meta hint">{t("Right-click a track in the editor to give one program its own default.")}</p>
</div>

<style>
  .group {
    display: flex;
    flex-direction: column;
  }
  .track {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 52px;
    padding: 8px 14px;
  }
  .track + .track,
  .track + .hint {
    border-top: 1px solid var(--line);
  }
  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    font-weight: 600;
  }
  .text .meta {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .initial {
    width: 28px;
    height: 28px;
    flex: none;
    border-radius: 10px;
    background: var(--surface);
    color: var(--accent);
    font-family: var(--mono);
    font-size: 12.5px;
    display: grid;
    place-items: center;
  }
  .icon img {
    width: 18px;
    height: 18px;
  }
  .mute,
  .forget {
    width: 30px;
    height: 30px;
    flex: none;
    padding: 0;
    display: grid;
    place-items: center;
    border: 0;
    border-radius: 999px;
    background: transparent;
    color: var(--muted);
  }
  .mute:hover,
  .forget:hover {
    background: var(--surface);
    color: var(--ink);
  }
  .forget-space {
    width: 30px;
    flex: none;
  }
  .mute.off {
    color: var(--red);
  }
  .volume {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 220px;
    flex: none;
  }
  .percent {
    width: 4ch;
    text-align: right;
    color: var(--muted);
    font-size: 12.5px;
  }
  .hint {
    margin: 0;
    padding: 10px 14px;
  }
</style>
