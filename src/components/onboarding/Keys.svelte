<script lang="ts">
  // The hotkey gets pressed, and the moment pops out as a clip.
  let { keys = ["Alt", "C"], seconds = 30 }: { keys?: string[]; seconds?: number } = $props();
  const length = $derived(seconds < 60 ? `0:${String(seconds).padStart(2, "0")}` : `${seconds / 60}:00`);
</script>

<div class="art" aria-hidden="true">
  <div class="toast">
    <span class="dot"></span>
    <span><b>Clip saved</b><br /><small>Last {seconds < 60 ? `${seconds} s` : `${seconds / 60} min`}</small></span>
  </div>
  <div class="clip">
    <span class="thumb"><span class="badge mono">[ NEW ]</span><span class="len mono">{length}</span></span>
  </div>
  <div class="keys">
    {#each keys as key, i (i)}
      {#if i > 0}<span class="plus">+</span>{/if}
      <span class="cap mono" class:wide={key.length > 1}><span>{key}</span></span>
    {/each}
  </div>
</div>

<style>
  .art {
    position: absolute;
    inset: 0;
  }
  .keys {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 14cqmin;
    display: flex;
    justify-content: center;
    align-items: center;
    gap: 3cqmin;
  }
  .plus {
    font-size: 6cqmin;
    color: var(--ghost);
  }
  .cap {
    display: grid;
    place-items: center;
    width: 21cqmin;
    height: 21cqmin;
    border-radius: 4.5cqmin;
    background: linear-gradient(#3a3a3a, #262626);
    box-shadow:
      0 2cqmin 0 #161616,
      0 3cqmin 5cqmin rgb(0 0 0 / 0.5);
    font-size: 7cqmin;
    color: var(--ink);
    animation: press 3.2s var(--travel) infinite;
  }
  .cap.wide {
    width: 30cqmin;
    font-size: 6cqmin;
  }
  .cap:last-child {
    animation-delay: 0.08s;
  }
  .cap span {
    transform: translateY(-0.6cqmin);
  }
  .clip {
    position: absolute;
    left: 50%;
    top: 30cqmin;
    width: 46cqmin;
    transform: translateX(-50%);
    animation: pop 3.2s var(--travel) infinite;
  }
  .thumb {
    position: relative;
    display: block;
    aspect-ratio: 16 / 9;
    border-radius: 3cqmin;
    outline: 0.6cqmin solid var(--accent);
    outline-offset: 0.8cqmin;
    background:
      radial-gradient(circle at 70% 35%, var(--accent) 0 7%, transparent 7.5%),
      linear-gradient(165deg, transparent 55%, #3a3a3a 55.5% 68%, transparent 68.5%),
      linear-gradient(#2a2a2a, #1b1b1b 60%, #333 60.5%);
  }
  .badge,
  .len {
    position: absolute;
    padding: 0.4cqmin 1.4cqmin;
    border-radius: 1.2cqmin;
    background: rgb(0 0 0 / 0.65);
    font-size: 2.8cqmin;
  }
  .badge {
    top: 1.6cqmin;
    left: 1.6cqmin;
    color: var(--accent);
  }
  .len {
    right: 1.6cqmin;
    bottom: 1.6cqmin;
    color: #fff;
  }
  .toast {
    position: absolute;
    top: 7cqmin;
    right: 7cqmin;
    display: flex;
    align-items: center;
    gap: 2.4cqmin;
    padding: 2cqmin 3.4cqmin 2cqmin 2.4cqmin;
    border-radius: 4cqmin;
    background: var(--raised);
    font-size: 3.2cqmin;
    line-height: 1.25;
    animation: toast 3.2s var(--travel) infinite;
  }
  .toast small {
    color: var(--muted);
  }
  .dot {
    width: 6cqmin;
    height: 6cqmin;
    border-radius: 1.6cqmin;
    background: var(--black);
    box-shadow: inset 0 0 0 1.6cqmin var(--black), inset 0 0 0 6cqmin var(--accent);
  }
  @keyframes press {
    0%,
    8% {
      transform: translateY(0);
      box-shadow:
        0 2cqmin 0 #161616,
        0 3cqmin 5cqmin rgb(0 0 0 / 0.5);
    }
    12%,
    20% {
      transform: translateY(1.6cqmin);
      box-shadow:
        0 0.4cqmin 0 #161616,
        0 1cqmin 2cqmin rgb(0 0 0 / 0.5);
      color: var(--accent);
    }
    28%,
    100% {
      transform: translateY(0);
    }
  }
  @keyframes pop {
    0%,
    16% {
      opacity: 0;
      transform: translate(-50%, 10cqmin) scale(0.85);
    }
    30%,
    84% {
      opacity: 1;
      transform: translate(-50%, 0) scale(1);
    }
    100% {
      opacity: 0;
      transform: translate(-50%, -3cqmin) scale(1);
    }
  }
  @keyframes toast {
    0%,
    24% {
      opacity: 0;
      transform: translateY(-3cqmin);
    }
    36%,
    80% {
      opacity: 1;
      transform: translateY(0);
    }
    100% {
      opacity: 0;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .cap,
    .clip,
    .toast {
      animation: none;
    }
  }
</style>
