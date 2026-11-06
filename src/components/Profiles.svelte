<script lang="ts">
  import { api, type AppWindow, type Profile, type Quality } from "../lib/api";
  import { appIcon } from "../lib/appIcons.svelte";
  import { endsWithExe } from "../lib/platform";
  import { lengthLabel } from "../lib/format";
  import { t } from "../lib/i18n.svelte";
  import { store } from "../lib/store.svelte";
  import Icon from "./Icon.svelte";
  import Segmented from "./Segmented.svelte";
  import Select from "./Select.svelte";
  import Switch from "./Switch.svelte";

  // Games and apps with their own clip length, quality and so on.
  const s = $derived(store.settings!);
  let open = $state<string | null>(null);
  let apps = $state<AppWindow[]>([]);

  /** In the choices, "same as the general setting". */
  const SAME = "same";
  const same = $derived({ value: SAME, label: t("Default") } as const);

  const key = (exe: string) => exe.toLowerCase();

  function save(profiles: Profile[]) {
    store.save({ ...s, profiles });
  }

  function change(exe: string, patch: Partial<Profile>) {
    save(s.profiles.map((p) => (key(p.exe) === key(exe) ? { ...p, ...patch } : p)));
  }

  function remove(exe: string) {
    save(s.profiles.filter((p) => key(p.exe) !== key(exe)));
    open = null;
  }

  async function loadApps() {
    apps = await api.listApps().catch(() => apps);
  }
  loadApps();

  function add(app: AppWindow) {
    if (!s.profiles.some((p) => key(p.exe) === key(app.exe))) {
      save([
        ...s.profiles,
        { exe: app.exe, name: app.name, path: app.path || null, ignore: false, bufferSeconds: null, fps: null, quality: null, recordMic: null },
      ]);
    }
    open = key(app.exe);
  }

  const qualityLabel: Record<Quality, string> = $derived({ low: t("Low"), medium: t("Medium"), high: t("High") });

  function summary(p: Profile): string {
    if (p.ignore) return t("Never recorded");
    const parts = [
      p.bufferSeconds !== null && lengthLabel(p.bufferSeconds),
      p.fps !== null && t("{n} fps", { n: p.fps }),
      p.quality !== null && t("{quality} quality", { quality: qualityLabel[p.quality] }),
      p.recordMic !== null && (p.recordMic ? t("Mic on") : t("Mic off")),
    ].filter(Boolean);
    return parts.length ? parts.join(" · ") : t("Recorded with the settings above");
  }

  /** Where a program lives, for its icon: as added, else from a running window or a recorded track. */
  function pathOf(p: Profile): string | null {
    if (p.path) return p.path;
    const running = apps.find((a) => key(a.exe) === key(p.exe));
    if (running?.path) return running.path;
    for (const item of store.items) {
      for (const track of item.tracks) if (track.exe && endsWithExe(track.exe, p.exe)) return track.exe;
    }
    return null;
  }

  const free = $derived(apps.filter((a) => !s.profiles.some((p) => key(p.exe) === key(a.exe))));
</script>

<div class="panel group">
  {#each s.profiles as p (key(p.exe))}
    {@const expanded = open === key(p.exe)}
    {@const icon = appIcon(pathOf(p))}
    <div class="profile">
      <button type="button" class="head" aria-expanded={expanded} onclick={() => (open = expanded ? null : key(p.exe))}>
        {#if icon}
          <span class="initial icon" class:off={p.ignore} aria-hidden="true"><img src={icon} alt="" /></span>
        {:else}
          <span class="initial" class:off={p.ignore} aria-hidden="true">{p.name.slice(0, 1).toUpperCase()}</span>
        {/if}
        <span class="text"><span>{p.name}</span><span class="meta">{summary(p)}</span></span>
        <Icon name={expanded ? "chevronUp" : "chevronDown"} size={16} />
      </button>
      {#if expanded}
        <div class="body">
          <div class="line">
            <span class="text"><span>{t("Record {name}", { name: p.name })}</span><span class="meta">{t("Off: Replix leaves it alone, even when it looks like a game")}</span></span>
            <Switch checked={!p.ignore} label={t("Record {name}", { name: p.name })} onchange={(v) => change(p.exe, { ignore: !v })} />
          </div>
          {#if !p.ignore}
            <div class="field">
              <span class="label">{t("Clip length")}</span>
              <Segmented
                label={t("Clip length")}
                value={p.bufferSeconds ?? SAME}
                options={[same, ...[15, 30, 60, 120, 300].map((n) => ({ value: n, label: lengthLabel(n) }))]}
                onchange={(v) => change(p.exe, { bufferSeconds: v === SAME ? null : Number(v) })}
              />
            </div>
            <div class="two">
              <div class="field">
                <span class="label">{t("Frame rate")}</span>
                <Segmented
                  label={t("Frame rate")}
                  value={p.fps ?? SAME}
                  options={[same, ...[30, 60, 120].map((n) => ({ value: n, label: `${n}` }))]}
                  onchange={(v) => change(p.exe, { fps: v === SAME ? null : Number(v) })}
                />
              </div>
              <div class="field">
                <span class="label">{t("Quality")}</span>
                <Segmented
                  label={t("Quality")}
                  value={p.quality ?? SAME}
                  options={[same, ...(["low", "medium", "high"] as const).map((q) => ({ value: q, label: qualityLabel[q] }))]}
                  onchange={(v) => change(p.exe, { quality: v === SAME ? null : (v as Quality) })}
                />
              </div>
            </div>
            <div class="field">
              <span class="label">{t("Microphone")}</span>
              <Segmented
                label={t("Microphone")}
                value={p.recordMic === null ? SAME : p.recordMic ? "on" : "off"}
                options={[same, { value: "on", label: t("On") }, { value: "off", label: t("Off") }]}
                onchange={(v) => change(p.exe, { recordMic: v === SAME ? null : v === "on" })}
              />
            </div>
          {/if}
          <div class="foot">
            <span class="meta mono">{p.exe}</span>
            <button type="button" class="btn ghost small" onclick={() => remove(p.exe)}>{t("Remove")}</button>
          </div>
        </div>
      {/if}
    </div>
  {/each}

  <div class="add-row">
    <span class="plus" aria-hidden="true"><Icon name="plus" size={14} /></span>
    <div class="add-select">
      <Select
        label={t("Add a game or app")}
        placeholder={t("Add a game or app…")}
        value={null}
        options={free.length
          ? free.map((a) => ({ value: a.exe, label: a.name, hint: a.title !== a.name ? a.title : undefined }))
          : [{ value: "", label: t("No other open windows"), hint: t("Start the game first") }]}
        onopen={loadApps}
        onchange={(exe) => {
          const app = free.find((a) => a.exe === exe);
          if (app) add(app);
        }}
      />
    </div>
    <span class="meta">{t("Open the game or app first.")}</span>
  </div>
</div>

<style>
  .group {
    display: flex;
    flex-direction: column;
  }
  .profile + .profile,
  .profile + .add-row {
    border-top: 1px solid var(--line);
  }
  .head {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 48px;
    padding: 8px 12px;
    border: 0;
    border-radius: 20px;
    background: transparent;
    color: var(--muted);
    text-align: left;
  }
  .head:hover,
  .head .text > span:first-child {
    color: var(--ink);
  }
  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .initial,
  .plus {
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
  .initial.off {
    color: var(--ghost);
  }
  .icon img {
    width: 18px;
    height: 18px;
  }
  /* A program Replix leaves alone looks switched off. */
  .icon.off img {
    filter: grayscale(1);
    opacity: 0.5;
  }
  .body {
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding: 4px 12px 12px 52px;
    animation: open 180ms var(--travel);
  }
  @keyframes open {
    from {
      opacity: 0;
      transform: translateY(-4px);
    }
  }
  .line {
    display: flex;
    align-items: center;
    gap: 16px;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: 0;
  }
  .label {
    font-size: 12px;
    color: var(--muted);
  }
  .two {
    display: grid;
    grid-template-columns: 1fr 1.4fr;
    gap: 8px;
  }
  .foot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }
  .foot .mono {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .add-row {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 48px;
    padding: 8px 12px;
  }
  .add-select {
    width: 260px;
    flex: none;
  }
  .add-row .meta {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  @media (prefers-reduced-motion: reduce) {
    .body {
      animation: none;
    }
  }
</style>
