<script lang="ts">
  import Icon from "../Icon.svelte";

  // The three keys light up one after another.
  let { rows }: { rows: { keys: string[]; icon: string; taken: boolean }[] } = $props();
</script>

<div class="art" aria-hidden="true">
  {#each rows as row, i (i)}
    <div class="row" class:taken={row.taken} style:animation-delay="{i * 1.2}s">
      <span class="tile"><Icon name={row.icon} size={20} filled={row.icon === "record"} /></span>
      <span class="keys">
        {#each row.keys as key, k (k)}
          {#if k > 0}<span class="plus">+</span>{/if}
          <span class="cap mono">{key}</span>
        {/each}
      </span>
    </div>
  {/each}
</div>

<style>
  .art {
    position: absolute;
    inset: 14cqmin 12cqmin;
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 5cqmin;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 4cqmin;
    padding: 2.4cqmin;
    border-radius: 3.6cqmin;
    background: #1d1627;
    animation: light 3.6s var(--travel) infinite;
  }
  .tile {
    width: 10cqmin;
    height: 10cqmin;
    flex: none;
    border-radius: 2.6cqmin;
    background: var(--raised);
    color: var(--accent);
    display: grid;
    place-items: center;
  }
  .keys {
    display: flex;
    align-items: center;
    gap: 2cqmin;
  }
  .plus {
    font-size: 4cqmin;
    color: var(--ghost);
  }
  .cap {
    min-width: 9cqmin;
    height: 9cqmin;
    padding: 0 2.2cqmin;
    display: grid;
    place-items: center;
    border-radius: 2.6cqmin;
    background: linear-gradient(#3a2f4f, #2a2038);
    box-shadow: 0 0.8cqmin 0 #140f1b;
    font-size: 3.8cqmin;
  }
  .taken .cap {
    color: var(--red);
    text-decoration: line-through;
  }
  @keyframes light {
    0%,
    100% {
      box-shadow: none;
    }
    12%,
    30% {
      box-shadow:
        0 0 0 0.6cqmin var(--accent),
        0 0 8cqmin color-mix(in srgb, var(--accent) 22%, transparent);
    }
    42% {
      box-shadow: none;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .row {
      animation: none;
    }
  }
</style>
