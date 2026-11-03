<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";

  // A microphone whose rings follow your voice. Noise flecks vanish with suppression on.
  let { denoise = true, on = true }: { denoise?: boolean; on?: boolean } = $props();
  let level = $state(0);

  onMount(() => {
    const unlisten = listen<number>("mic-level", (e) => {
      const db = 20 * Math.log10(Math.max(e.payload, 1e-5));
      level = Math.max(Math.min(1, Math.max(0, (db + 60) / 60)), level - 0.08);
    });
    return () => {
      unlisten.then((f) => f());
    };
  });

  const noise = ["~", "·", "~", "≈", "·", "~", "·", "≈"];
</script>

<div class="art" class:off={!on} aria-hidden="true">
  {#each [3, 2, 1] as ring (ring)}
    <span class="ring" style:--k={1 + level * ring * 0.22} style:opacity={on ? 0.15 + level * 0.5 / ring : 0.08}></span>
  {/each}
  {#each noise as n, i (i)}
    <span class="noise mono" class:gone={denoise} style:--a="{i * 45}deg" style:animation-delay="{-i * 0.4}s">{n}</span>
  {/each}
  <svg class="mic" viewBox="0 0 40 60">
    <rect x="11" y="3" width="18" height="32" rx="9" />
    <path d="M5 26a15 15 0 0 0 30 0M20 41v10M12 53h16" fill="none" />
    <path class="grille" d="M15 13h10M15 19h10M15 25h10" fill="none" />
  </svg>
  <span class="state mono">{on ? (denoise ? "[ clean voice ]" : "[ raw ]") : "[ mic off ]"}</span>
</div>

<style>
  .art {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
  }
  .ring {
    position: absolute;
    width: 46cqmin;
    height: 46cqmin;
    border-radius: 50%;
    border: 0.6cqmin solid var(--accent);
    transform: scale(var(--k));
    transition:
      transform 90ms linear,
      opacity 90ms linear;
  }
  .ring:nth-child(2) {
    width: 36cqmin;
    height: 36cqmin;
  }
  .ring:nth-child(3) {
    width: 26cqmin;
    height: 26cqmin;
  }
  .mic {
    position: relative;
    width: 18cqmin;
    fill: var(--raised);
    stroke: var(--accent);
    stroke-width: 2.2;
    stroke-linecap: round;
  }
  .mic .grille {
    stroke: var(--ghost);
    stroke-width: 1.6;
  }
  .off .mic {
    stroke: var(--ghost);
  }
  .noise {
    position: absolute;
    font-size: 5cqmin;
    color: var(--ghost);
    transform: rotate(var(--a)) translateY(-34cqmin) rotate(calc(-1 * var(--a)));
    animation: flicker 2.4s ease-in-out infinite;
    transition: opacity 500ms ease;
  }
  .noise.gone {
    opacity: 0 !important;
    animation: none;
  }
  .state {
    position: absolute;
    bottom: 10cqmin;
    font-size: 3.6cqmin;
    color: var(--accent);
  }
  .off .state {
    color: var(--ghost);
  }
  @keyframes flicker {
    0%,
    100% {
      opacity: 0.2;
    }
    50% {
      opacity: 0.7;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .noise {
      animation: none;
      opacity: 0.4;
    }
    .ring {
      transition: none;
    }
  }
</style>
