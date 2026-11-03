<script lang="ts">
  import { api, type AppWindow, type Profile, type Quality } from "../lib/api";
  import { lengthLabel } from "../lib/format";
  import { store } from "../lib/store.svelte";
  import Icon from "./Icon.svelte";
  import Segmented from "./Segmented.svelte";
  import Switch from "./Switch.svelte";

  // Games and apps with their own clip length, quality and so on.
  const s = $derived(store.settings!);
  let open = $state<string | null>(null);
  let adding = $state(false);
  let apps = $state<AppWindow[]>([]);

  /** In the choices, "same as the general setting". */
  const SAME = "same";
  const same = { value: SAME, label: "Default" } as const;

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

  async function startAdding() {
    apps = await api.listApps();
    adding = true;
  }

  function add(app: AppWindow) {
    adding = false;
    if (!s.profiles.some((p) => key(p.exe) === key(app.exe))) {
      save([
        ...s.profiles,
        { exe: app.exe, name: app.name, ignore: false, bufferSeconds: null, fps: null, quality: null, recordMic: null },
      ]);
    }
    open = key(app.exe);
  }

  const qualityLabel: Record<Quality, string> = { low: "Low", medium: "Medium", high: "High" };

  function summary(p: Profile): string {
    if (p.ignore) return "Never recorded";
    const parts = [
      p.bufferSeconds !== null && lengthLabel(p.bufferSeconds),
      p.fps !== null && `${p.fps} fps`,
      p.quality !== null && `${qualityLabel[p.quality]} quality`,
      p.recordMic !== null && (p.recordMic ? "Mic on" : "Mic off"),
    ].filter(Boolean);
    return parts.length ? parts.join(" · ") : "Recorded with the settings above";
  }

  const free = $derived(apps.filter((a) => !s.profiles.some((p) => key(p.exe) === key(a.exe))));
</script>

<div class="panel group">
  {#each s.profiles as p (key(p.exe))}
    {@const expanded = open === key(p.exe)}
    <div class="profile">
      <button type="button" class="head" aria-expanded={expanded} onclick={() => (open = expanded ? null : key(p.exe))}>
        <span class="initial" class:off={p.ignore} aria-hidden="true">{p.name.slice(0, 1).toUpperCase()}</span>
        <span class="text"><span>{p.name}</span><span class="meta">{summary(p)}</span></span>
        <Icon name={expanded ? "chevronUp" : "chevronDown"} size={16} />
      </button>
      {#if expanded}
        <div class="body">
          <div class="line">
            <span class="text"><span>Record {p.name}</span><span class="meta">Off: Replix leaves it alone, even when it looks like a game</span></span>
            <Switch checked={!p.ignore} label="Record {p.name}" onchange={(v) => change(p.exe, { ignore: !v })} />
          </div>
          {#if !p.ignore}
            <div class="field">
              <span class="label">Clip length</span>
              <Segmented
                label="Clip length"
                value={p.bufferSeconds ?? SAME}
                options={[same, ...[15, 30, 60, 120, 300].map((n) => ({ value: n, label: lengthLabel(n) }))]}
                onchange={(v) => change(p.exe, { bufferSeconds: v === SAME ? null : Number(v) })}
              />
            </div>
            <div class="two">
              <div class="field">
                <span class="label">Frame rate</span>
                <Segmented
                  label="Frame rate"
                  value={p.fps ?? SAME}
                  options={[same, ...[30, 60, 120].map((n) => ({ value: n, label: `${n}` }))]}
                  onchange={(v) => change(p.exe, { fps: v === SAME ? null : Number(v) })}
                />
              </div>
              <div class="field">
                <span class="label">Quality</span>
                <Segmented
                  label="Quality"
                  value={p.quality ?? SAME}
                  options={[same, ...(["low", "medium", "high"] as const).map((q) => ({ value: q, label: qualityLabel[q] }))]}
                  onchange={(v) => change(p.exe, { quality: v === SAME ? null : (v as Quality) })}
                />
              </div>
            </div>
            <div class="field">
              <span class="label">Microphone</span>
              <Segmented
                label="Microphone"
                value={p.recordMic === null ? SAME : p.recordMic ? "on" : "off"}
                options={[same, { value: "on", label: "On" }, { value: "off", label: "Off" }]}
                onchange={(v) => change(p.exe, { recordMic: v === SAME ? null : v === "on" })}
              />
            </div>
          {/if}
          <div class="foot">
            <span class="meta mono">{p.exe}</span>
            <button type="button" class="btn ghost small" onclick={() => remove(p.exe)}>Remove</button>
          </div>
        </div>
      {/if}
    </div>
  {/each}

  {#if adding}
    <div class="picker">
      <div class="picker-head">
        <span class="meta">Open the game or app first, then pick it here.</span>
        <button type="button" class="btn ghost small" onclick={() => (adding = false)}>Cancel</button>
      </div>
      <ul class="scroll apps">
        {#each free as a (a.exe)}
          <li>
            <button type="button" onclick={() => add(a)}>
              <span>{a.name}</span><span class="meta">{a.title}</span>
            </button>
          </li>
        {:else}
          <li class="meta empty">No other open windows. Start the game, then try again.</li>
        {/each}
      </ul>
    </div>
  {:else}
    <button type="button" class="add" onclick={startAdding}>
      <span class="plus" aria-hidden="true"><Icon name="plus" size={14} /></span>
      <span>Add a game or app</span>
    </button>
  {/if}
</div>

<style>
  .group {
    display: flex;
    flex-direction: column;
  }
  .profile + .profile,
  .profile + .add,
  .profile + .picker {
    border-top: 1px solid var(--line);
  }
  .head,
  .add {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 48px;
    padding: 8px 12px;
    border: 0;
    border-radius: 12px;
    background: transparent;
    color: var(--muted);
    text-align: left;
  }
  .head:hover,
  .add:hover {
    color: var(--ink);
  }
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
    border-radius: 8px;
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
  .picker {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px;
  }
  .picker-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
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
  .empty {
    padding: 8px 12px;
  }
  @media (prefers-reduced-motion: reduce) {
    .body {
      animation: none;
    }
  }
</style>
