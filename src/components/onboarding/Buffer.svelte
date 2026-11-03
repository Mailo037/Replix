<script lang="ts">
  // Frames slide past. The newest stay inside the bracket; older ones crumble into characters.
  let { seconds = 30 }: { seconds?: number } = $props();

  const scenes = [30, 62, 44, 70, 36, 58, 50, 66];
  const reel = [...scenes, ...scenes];
  const dust = [
    ":  .   ·  :   .",
    " .  :  ·    .  ",
    "·   .   :  ·   ",
    "  :   ·  .   : ",
    " .  ·   :  .   ",
  ].join("\n");
</script>

<div class="art" aria-hidden="true">
  <div class="track">
    <div class="reel">
      {#each reel as x, i (i)}
        <span class="frame" style:--x="{x}%"></span>
      {/each}
    </div>
  </div>
  <pre class="dust mono">{dust}</pre>
  <div class="keep">
    <span class="label mono">[ last {seconds < 60 ? `${seconds} s` : `${seconds / 60} min`} ]</span>
  </div>
  <div class="now mono">now</div>
</div>

<style>
  .art {
    position: absolute;
    inset: 0;
  }
  .track {
    position: absolute;
    left: 0;
    right: 0;
    top: 40cqmin;
    height: 20cqmin;
    overflow: hidden;
    mask-image: linear-gradient(to right, transparent 4%, #000 42%, #000 90%, transparent);
  }
  .reel {
    display: flex;
    gap: 2.4cqmin;
    width: max-content;
    animation: slide 9s linear infinite;
  }
  .frame {
    width: 24cqmin;
    height: 20cqmin;
    flex: none;
    border-radius: 2.4cqmin;
    background:
      radial-gradient(circle at var(--x) 34%, var(--accent) 0 7%, transparent 7.5%),
      linear-gradient(170deg, transparent 58%, #3a2f4f 58.5% 70%, transparent 70.5%),
      linear-gradient(#2a2038, #1c1526 62%, #31263f 62.5%);
  }
  .keep {
    position: absolute;
    top: 36cqmin;
    right: 6cqmin;
    width: 46cqmin;
    height: 28cqmin;
    border: 0.7cqmin solid var(--accent);
    border-radius: 4cqmin;
    box-shadow: 0 0 6cqmin color-mix(in srgb, var(--accent) 25%, transparent);
  }
  .label {
    position: absolute;
    top: -7cqmin;
    left: 50%;
    transform: translateX(-50%);
    white-space: nowrap;
    font-size: 3.6cqmin;
    color: var(--accent);
  }
  .now {
    position: absolute;
    top: 68cqmin;
    right: 6cqmin;
    width: 46cqmin;
    text-align: right;
    font-size: 3.2cqmin;
    color: var(--ghost);
  }
  .now::before {
    content: "";
    display: inline-block;
    width: 30cqmin;
    height: 0.3cqmin;
    margin-right: 2cqmin;
    vertical-align: middle;
    background: linear-gradient(to right, transparent, var(--line));
  }
  .dust {
    position: absolute;
    top: 38cqmin;
    left: 3cqmin;
    margin: 0;
    font-size: 3.4cqmin;
    line-height: 1.25;
    color: var(--ghost);
    opacity: 0.55;
    animation: drift 3s ease-in-out infinite alternate;
  }
  @keyframes slide {
    to {
      transform: translateX(-50%);
    }
  }
  @keyframes drift {
    to {
      transform: translate(-1.5cqmin, 1cqmin);
      opacity: 0.25;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .reel,
    .dust {
      animation: none;
    }
  }
</style>
