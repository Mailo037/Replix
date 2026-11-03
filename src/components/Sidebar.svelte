<script lang="ts">
  import { store, type Filter } from "../lib/store.svelte";
  import Icon from "./Icon.svelte";

  const sections: { filter: Filter; label: string; icon: string; count: () => number }[] = [
    { filter: "all", label: "All clips", icon: "clips", count: () => store.items.length },
    { filter: "screenshots", label: "Screenshots", icon: "screenshots", count: () => store.counts.screenshots },
    { filter: "sessions", label: "Sessions", icon: "sessions", count: () => store.counts.sessions },
    { filter: "drafts", label: "Drafts", icon: "drafts", count: () => store.counts.drafts },
    { filter: "favorites", label: "Favorites", icon: "star", count: () => store.counts.favorites },
  ];

  const isActive = (filter: Filter, game: string | null = null) =>
    store.view.name === "library" && store.view.filter === filter && store.view.game === game;
</script>

<nav class="sidebar" aria-label="Library">
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
      <div class="heading">Games</div>
      <ul class="scroll games">
        {#each store.games as g (g.name)}
          <li>
            <button
              type="button"
              class="row"
              class:current={isActive("all", g.name)}
              onclick={() => store.go({ name: "library", filter: "all", game: g.name })}
            >
              <span class="initial" aria-hidden="true">{g.name.slice(0, 1).toUpperCase()}</span>
              <span class="label">{g.name}</span>
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
    width: 196px;
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
  .group {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .games-panel {
    min-height: 0;
  }
  .heading {
    padding: 8px 12px 4px;
    font-size: 12px;
    color: var(--ghost);
  }
  .games {
    min-height: 0;
  }
  .row {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 10px;
    height: 34px;
    padding: 0 10px;
    border: 0;
    border-radius: 9px;
    background: transparent;
    color: var(--muted);
    text-align: left;
    transition: background-color 160ms ease, color 160ms ease;
  }
  .row:hover {
    color: var(--ink);
  }
  .row.current {
    background: var(--raised);
    color: var(--ink);
    font-weight: 500;
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
    width: 20px;
    height: 20px;
    flex: none;
    border-radius: 6px;
    background: var(--surface);
    color: var(--accent);
    font-family: var(--mono);
    font-size: 11px;
    display: grid;
    place-items: center;
  }
</style>
