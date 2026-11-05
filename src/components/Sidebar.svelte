<script lang="ts">
  import { store, type Filter } from "../lib/store.svelte";
  import { gameColor } from "../lib/format";
  import { t } from "../lib/i18n.svelte";
  import Icon from "./Icon.svelte";

  const sections = $derived<{ filter: Filter; label: string; icon: string; count: () => number }[]>([
    { filter: "all", label: t("All clips"), icon: "clips", count: () => store.items.length },
    { filter: "screenshots", label: t("Screenshots"), icon: "screenshots", count: () => store.counts.screenshots },
    { filter: "sessions", label: t("Sessions"), icon: "sessions", count: () => store.counts.sessions },
    { filter: "drafts", label: t("Drafts"), icon: "drafts", count: () => store.counts.drafts },
    { filter: "favorites", label: t("Favorites"), icon: "star", count: () => store.counts.favorites },
  ]);

  const gameLabel = (name: string) => (name === "Desktop" ? t("Desktop") : name);

  const isActive = (filter: Filter, game: string | null = null) =>
    store.view.name === "library" && store.view.filter === filter && store.view.game === game;
</script>

<nav class="sidebar" aria-label={t("Library")}>
  <ul class="panel group">
    {#each sections as s (s.filter)}
      <li>
        <button
          type="button"
          class="row"
          class:current={isActive(s.filter)}
          aria-current={isActive(s.filter) ? "page" : undefined}
          onclick={() => store.go({ name: "library", filter: s.filter, game: null })}
        >
          <Icon name={s.icon} />
          <span class="label">{s.label}</span>
          <span class="count mono">{s.count()}</span>
        </button>
      </li>
    {/each}
  </ul>

  {#if store.games.length}
    <div class="panel group games-panel">
      <div class="heading">{t("Games")}</div>
      <ul class="scroll games">
        {#each store.games as g (g.name)}
          <li>
            <button
              type="button"
              class="row"
              class:current={isActive("all", g.name)}
              onclick={() => store.go({ name: "library", filter: "all", game: g.name })}
            >
              <span class="initial" aria-hidden="true" style:background={gameColor(g.name)}>{gameLabel(g.name).slice(0, 1).toUpperCase()}</span>
              <span class="label">{gameLabel(g.name)}</span>
              <span class="count mono">{g.count}</span>
            </button>
          </li>
        {/each}
      </ul>
    </div>
  {/if}
</nav>

<style>
  .sidebar {
    width: 220px;
    flex: none;
    display: flex;
    flex-direction: column;
    gap: 12px;
    min-height: 0;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  /* The panel's corners follow the rows inside it: row radius plus the 6px padding. */
  .group {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 6px;
    border-radius: 22px;
  }
  .games-panel {
    min-height: 0;
  }
  .heading {
    padding: 8px 12px 4px;
    font-size: 12.5px;
    font-weight: 700;
    color: var(--ghost);
  }
  .games {
    min-height: 0;
  }
  .row {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 11px;
    height: 38px;
    padding: 0 10px 0 12px;
    border: 0;
    border-radius: 16px;
    background: transparent;
    color: var(--muted);
    font-size: 14px;
    font-weight: 600;
    text-align: left;
    transition: background-color 160ms ease, color 160ms ease;
  }
  .row:hover {
    background: var(--surface);
    color: var(--ink);
  }
  .row.current {
    background: var(--accent);
    color: var(--on-accent);
    font-weight: 700;
  }
  .row.current .count {
    min-width: 22px;
    padding: 1px 6px;
    border-radius: 999px;
    background: var(--on-accent);
    color: var(--accent);
    text-align: center;
  }
  .label {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .count {
    font-size: 12px;
    font-weight: 400;
    color: var(--ghost);
  }
  .initial {
    width: 24px;
    height: 24px;
    margin-left: -4px;
    flex: none;
    border-radius: 8px;
    color: var(--on-sticker);
    font-size: 12.5px;
    font-weight: 800;
    display: grid;
    place-items: center;
  }
</style>
