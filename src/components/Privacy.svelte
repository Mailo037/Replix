<script lang="ts">
  import { api, type AppWindow, type Excluded } from "../lib/api";
  import { appIcon } from "../lib/appIcons.svelte";
  import { t } from "../lib/i18n.svelte";
  import { store } from "../lib/store.svelte";
  import Icon from "./Icon.svelte";
  import Select from "./Select.svelte";

  // Programs that never end up in a recording.
  const s = $derived(store.settings!);
  let apps = $state<AppWindow[]>([]);

  const key = (exe: string) => exe.toLowerCase();

  function save(excluded: Excluded[]) {
    store.save({ ...s, excluded });
  }

  async function loadApps() {
    apps = await api.listApps().catch(() => apps);
  }
  loadApps();

  function add(app: AppWindow) {
    if (!s.excluded.some((e) => key(e.exe) === key(app.exe))) {
      save([...s.excluded, { exe: app.exe, name: app.name, path: app.path || null }]);
    }
  }

  function remove(exe: string) {
    save(s.excluded.filter((e) => key(e.exe) !== key(exe)));
  }

  const free = $derived(apps.filter((a) => !s.excluded.some((e) => key(e.exe) === key(a.exe))));
</script>

<div class="panel group">
  {#each s.excluded as e (key(e.exe))}
    {@const icon = appIcon(e.path ?? apps.find((a) => key(a.exe) === key(e.exe))?.path ?? null)}
    <div class="program">
      {#if icon}
        <span class="initial icon" aria-hidden="true"><img src={icon} alt="" /></span>
      {:else}
        <span class="initial" aria-hidden="true">{e.name.slice(0, 1).toUpperCase()}</span>
      {/if}
      <span class="text"><span>{e.name}</span><span class="meta mono">{e.exe}</span></span>
      <button type="button" class="btn ghost small" aria-label={t("Stop keeping {name} out", { name: e.name })} onclick={() => remove(e.exe)}>
        {t("Remove")}
      </button>
    </div>
  {/each}

  <div class="add-row">
    <span class="plus" aria-hidden="true"><Icon name="shield" size={14} /></span>
    <div class="add-select">
      <Select
        label={t("Keep a program out")}
        placeholder={t("Keep a program out…")}
        value={null}
        options={free.length
          ? free.map((a) => ({ value: a.exe, label: a.name, hint: a.title !== a.name ? a.title : undefined }))
          : [{ value: "", label: t("No other open windows"), hint: t("Open the program first") }]}
        onopen={loadApps}
        onchange={(exe) => {
          const app = free.find((a) => a.exe === exe);
          if (app) add(app);
        }}
      />
    </div>
    <span class="meta">{t("Open the program first.")}</span>
  </div>
</div>

<style>
  .group {
    display: flex;
    flex-direction: column;
  }
  .program,
  .add-row {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 48px;
    padding: 8px 12px;
  }
  .program + .program,
  .program + .add-row {
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
  .icon img {
    width: 18px;
    height: 18px;
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
</style>
