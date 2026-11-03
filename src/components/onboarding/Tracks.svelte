<script lang="ts">
  import Icon from "../Icon.svelte";

  // Four sources, four tracks. One of them gets muted and goes quiet while the rest play on.
  const rows = [
    { name: "Game", icon: "game", muted: false },
    { name: "Discord", icon: "speaker", muted: true },
    { name: "Spotify", icon: "app", muted: false },
    { name: "Your mic", icon: "mic", muted: false },
  ];
  const BARS = 22;
</script>

<div class="art" aria-hidden="true">
  {#each rows as row, r (row.name)}
    <div class="row" class:mutes={row.muted}>
      <span class="tile"><Icon name={row.icon} size={18} /></span>
      <span class="name">{row.name}</span>
      <span class="wave">
        {#each { length: BARS } as _, i (i)}
          <i style:animation-delay="{-((i * 0.37 + r * 0.53) % 1.2)}s"></i>
        {/each}
      </span>
      <span class="toggle"><span></span></span>
    </div>
  {/each}
</div>

<style>
  .art {
    position: absolute;
    inset: 12cqmin 8cqmin;
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 3cqmin;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 3cqmin;
    height: 11.5cqmin;
    padding: 0 3cqmin;
    border-radius: 3cqmin;
    background: #1c1c1c;
  }
  .tile {
    width: 7cqmin;
    height: 7cqmin;
    flex: none;
    border-radius: 2cqmin;
    background: var(--raised);
    color: var(--accent);
    display: grid;
    place-items: center;
  }
  .name {
    width: 17cqmin;
    font-size: 3.4cqmin;
    font-weight: 500;
  }
  .wave {
    flex: 1;
    height: 6cqmin;
    display: flex;
    align-items: center;
    gap: 0.8cqmin;
  }
  .wave i {
    flex: 1;
    height: 100%;
    border-radius: 1cqmin;
    background: var(--accent);
    transform-origin: center;
    animation: level 1.2s ease-in-out infinite alternate;
  }
  .toggle {
    position: relative;
    width: 9cqmin;
    height: 5.4cqmin;
    flex: none;
    border-radius: 3cqmin;
    background: var(--accent);
  }
  .toggle span {
    position: absolute;
    top: 0.7cqmin;
    left: 4.3cqmin;
    width: 4cqmin;
    height: 4cqmin;
    border-radius: 50%;
    background: #fff;
  }
  /* Discord goes quiet halfway through the loop, then comes back. */
  .mutes .toggle {
    animation: switch-bg 6s var(--travel) infinite;
  }
  .mutes .toggle span {
    animation: switch-knob 6s var(--travel) infinite;
  }
  .mutes .wave i {
    animation:
      level 1.2s ease-in-out infinite alternate,
      hush 6s var(--travel) infinite;
  }
  .mutes .name,
  .mutes .tile {
    animation: dim 6s var(--travel) infinite;
  }
  @keyframes level {
    from {
      transform: scaleY(0.15);
    }
    to {
      transform: scaleY(0.95);
    }
  }
  @keyframes hush {
    0%,
    30%,
    92%,
    100% {
      background: var(--accent);
      opacity: 1;
    }
    38%,
    84% {
      background: var(--line);
      opacity: 0.6;
      height: 12%;
    }
  }
  @keyframes switch-bg {
    0%,
    30%,
    92%,
    100% {
      background: var(--accent);
    }
    36%,
    86% {
      background: var(--hover);
    }
  }
  @keyframes switch-knob {
    0%,
    30%,
    92%,
    100% {
      left: 4.3cqmin;
    }
    36%,
    86% {
      left: 0.7cqmin;
    }
  }
  @keyframes dim {
    0%,
    30%,
    92%,
    100% {
      opacity: 1;
    }
    36%,
    86% {
      opacity: 0.4;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .wave i,
    .mutes .wave i {
      animation: none;
      transform: scaleY(0.5);
    }
    .mutes .toggle,
    .mutes .toggle span,
    .mutes .name,
    .mutes .tile {
      animation: none;
    }
  }
</style>
