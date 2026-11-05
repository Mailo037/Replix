<script lang="ts">
  import type { Source } from "../../lib/api";
  import { t } from "../../lib/i18n.svelte";

  // A monitor showing what gets recorded for each choice.
  let { source }: { source: Source } = $props();
</script>

<div class="art" aria-hidden="true">
  <div class="monitor" class:all={source === "screen"}>
    <div class="screen">
      {#if source === "game"}
        <div class="game">
          <span class="sun"></span>
          <span class="hills"></span>
          <span class="hud mono">HP ████░░ · 12/30</span>
          <span class="cross"></span>
        </div>
        <span class="corners"><i></i><i></i><i></i><i></i></span>
      {:else}
        <div class="desk">
          <span class="win a" class:pick={source === "app"}><i></i><b></b><b></b><b class="short"></b></span>
          <span class="win b"><i></i><b></b><b class="short"></b></span>
          <span class="win c"><i></i><b></b></span>
          <span class="bar"></span>
        </div>
      {/if}
    </div>
    <span class="stand"></span>
  </div>
  <span class="tag mono">
    {source === "game" ? t("[ only the game ]") : source === "screen" ? t("[ everything ]") : t("[ just this app ]")}
  </span>
</div>

<style>
  .art {
    position: absolute;
    inset: 0;
  }
  .monitor {
    position: absolute;
    left: 12cqmin;
    right: 12cqmin;
    top: 20cqmin;
  }
  .screen {
    position: relative;
    aspect-ratio: 16 / 10;
    border-radius: 3cqmin;
    border: 1.4cqmin solid #2e243c;
    background: #140f1b;
    overflow: hidden;
    transition: border-color 300ms ease;
  }
  .all .screen {
    border-color: var(--accent);
    box-shadow: 0 0 8cqmin color-mix(in srgb, var(--accent) 25%, transparent);
  }
  .stand {
    display: block;
    width: 22cqmin;
    height: 7cqmin;
    margin: 0 auto;
    background: linear-gradient(#2e243c, #221a2c);
    clip-path: polygon(30% 0, 70% 0, 100% 100%, 0 100%);
  }
  .game {
    position: absolute;
    inset: 0;
    background: linear-gradient(#231b2e, #18121f 64%, #2e243c 64.5%);
    animation: fade 300ms var(--travel);
  }
  .sun {
    position: absolute;
    top: 16%;
    right: 22%;
    width: 9cqmin;
    height: 9cqmin;
    border-radius: 50%;
    background: var(--accent);
  }
  .hills {
    position: absolute;
    left: -10%;
    right: -10%;
    bottom: 30%;
    height: 22%;
    background: #3a2f4f;
    clip-path: polygon(0 100%, 18% 30%, 34% 70%, 52% 10%, 70% 60%, 86% 25%, 100% 100%);
  }
  .hud {
    position: absolute;
    left: 10%;
    bottom: 10%;
    font-size: 2.6cqmin;
    color: var(--muted);
  }
  .cross {
    position: absolute;
    left: 50%;
    top: 50%;
    width: 5cqmin;
    height: 5cqmin;
    transform: translate(-50%, -50%);
    background:
      linear-gradient(var(--ink), var(--ink)) center / 0.5cqmin 100% no-repeat,
      linear-gradient(var(--ink), var(--ink)) center / 100% 0.5cqmin no-repeat;
    opacity: 0.8;
  }
  /* Viewfinder corners: the part that gets recorded. */
  .corners {
    position: absolute;
    inset: 5%;
    animation: breathe 2.4s ease-in-out infinite;
  }
  .corners i {
    position: absolute;
    width: 8cqmin;
    height: 6cqmin;
    border: 0 solid var(--accent);
  }
  .corners i:nth-child(1) {
    top: 0;
    left: 0;
    border-width: 0.8cqmin 0 0 0.8cqmin;
    border-top-left-radius: 2cqmin;
  }
  .corners i:nth-child(2) {
    top: 0;
    right: 0;
    border-width: 0.8cqmin 0.8cqmin 0 0;
    border-top-right-radius: 2cqmin;
  }
  .corners i:nth-child(3) {
    bottom: 0;
    left: 0;
    border-width: 0 0 0.8cqmin 0.8cqmin;
    border-bottom-left-radius: 2cqmin;
  }
  .corners i:nth-child(4) {
    bottom: 0;
    right: 0;
    border-width: 0 0.8cqmin 0.8cqmin 0;
    border-bottom-right-radius: 2cqmin;
  }
  .desk {
    position: absolute;
    inset: 0;
    background: radial-gradient(circle at 30% 30%, #221a2c, #140f1b);
    animation: fade 300ms var(--travel);
  }
  .win {
    position: absolute;
    display: flex;
    flex-direction: column;
    gap: 1.4cqmin;
    padding: 4.6cqmin 2.4cqmin 2cqmin;
    border-radius: 1.6cqmin;
    background: #2c2239;
    transition: opacity 300ms ease, box-shadow 300ms ease;
  }
  .win i {
    position: absolute;
    top: 1.4cqmin;
    left: 2cqmin;
    width: 8cqmin;
    height: 1.4cqmin;
    border-radius: 1cqmin;
    background: #3d3152;
  }
  .win b {
    height: 1.4cqmin;
    border-radius: 1cqmin;
    background: #382c4c;
  }
  .win b.short {
    width: 60%;
  }
  .win.a {
    left: 8%;
    top: 12%;
    width: 46%;
    height: 50%;
  }
  .win.b {
    left: 46%;
    top: 30%;
    width: 42%;
    height: 42%;
  }
  .win.c {
    left: 18%;
    top: 58%;
    width: 30%;
    height: 22%;
  }
  .win.pick {
    z-index: 2;
    box-shadow:
      0 0 0 0.8cqmin var(--accent),
      0 0 8cqmin color-mix(in srgb, var(--accent) 30%, transparent);
  }
  .desk:has(.pick) .win:not(.pick) {
    opacity: 0.35;
  }
  .bar {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    height: 7%;
    background: #221a2c;
  }
  .tag {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 10cqmin;
    text-align: center;
    font-size: 3.6cqmin;
    color: var(--accent);
  }
  @keyframes fade {
    from {
      opacity: 0;
    }
  }
  @keyframes breathe {
    50% {
      opacity: 0.45;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .corners {
      animation: none;
    }
  }
</style>
