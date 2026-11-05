<script lang="ts">
  import { t } from "../../lib/i18n.svelte";
  // The Replix mark assembles itself cell by cell, then glows.
  const cells = [
    [0, 0], [0, 4], [0, 8], [0, 12], [0, 16],
    [4, 4], [4, 8], [4, 12],
    [8, 8],
  ];
</script>

<div class="art" aria-hidden="true">
  <span class="glow"></span>
  <svg viewBox="-1 -1 13 21" class="mark">
    {#each cells as [x, y], i (i)}
      <rect {x} {y} width="3" height="3" rx="0.4" style:animation-delay="{i * 90}ms" />
    {/each}
  </svg>
  <span class="caption mono">{t("ready")}<span class="cursor">_</span></span>
</div>

<style>
  .art {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
  }
  .glow {
    position: absolute;
    width: 70cqmin;
    height: 70cqmin;
    border-radius: 50%;
    background: radial-gradient(circle, color-mix(in srgb, var(--accent) 30%, transparent), transparent 65%);
    animation: glow 3.2s ease-in-out infinite alternate;
  }
  .mark {
    position: relative;
    width: 24cqmin;
    fill: var(--accent);
    overflow: visible;
  }
  rect {
    transform-box: fill-box;
    transform-origin: center;
    animation: cell 700ms var(--travel) both;
  }
  .caption {
    position: absolute;
    bottom: 16cqmin;
    font-size: 4.4cqmin;
    color: var(--muted);
  }
  .cursor {
    color: var(--accent);
    animation: blink 1s steps(1) infinite;
  }
  @keyframes cell {
    from {
      opacity: 0;
      transform: scale(0.2) translateY(-2px);
    }
  }
  @keyframes glow {
    to {
      transform: scale(1.12);
      opacity: 0.7;
    }
  }
  @keyframes blink {
    50% {
      opacity: 0;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    rect,
    .glow,
    .cursor {
      animation: none;
    }
  }
</style>
