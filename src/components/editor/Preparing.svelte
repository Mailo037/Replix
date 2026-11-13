<script lang="ts">
  import { onMount } from "svelte";
  import type { Preparing } from "../../lib/api";
  import { duration } from "../../lib/format";
  import { t } from "../../lib/i18n.svelte";

  /**
   * Shown while a clip is made ready for the editor: what is being done, how far it is and
   * about how long is left. A clip prepared before opens before this shows at all.
   */
  let { step }: { step: Preparing | null } = $props();

  /** Shown only once opening takes a moment, so a quick open does not flash it. */
  let shown = $state(false);
  let now = $state(performance.now());
  const started = performance.now();
  /** The first word of the copy: when it was heard and how far it was then. */
  let copyFrom: { at: number; done: number } | null = null;

  onMount(() => {
    const show = setTimeout(() => (shown = true), 350);
    const tick = setInterval(() => (now = performance.now()), 500);
    return () => {
      clearTimeout(show);
      clearInterval(tick);
    };
  });

  $effect(() => {
    if (step?.step === "splitting" && !copyFrom) copyFrom = { at: performance.now(), done: step.done };
  });

  /** How much of the copy is done, 0 to 1; null while there is nothing to measure. */
  const share = $derived(step?.step === "splitting" && step.total > 0 ? Math.min(1, step.done / step.total) : null);

  /** Seconds left at the speed the copy has had so far; null until that speed is clear. */
  const left = $derived.by(() => {
    if (step?.step !== "splitting" || !copyFrom) return null;
    const seconds = (now - copyFrom.at) / 1000;
    const rate = (step.done - copyFrom.done) / seconds;
    if (seconds < 1.5 || !(rate > 0)) return null;
    return Math.max(1, (step.total - step.done) / rate);
  });

  const label = $derived.by(() => {
    switch (step?.step) {
      case "waiting":
        return t("Waiting for other work to finish");
      case "reading":
        return t("Reading the clip");
      case "splitting":
        return t("Copying the audio tracks · {done} of {total}", { done: duration(step.done), total: duration(step.total) });
      default:
        return t("Preparing tracks…");
    }
  });
</script>

{#if shown}
  <div class="preparing" role="status" aria-live="polite">
    <h2>{t("Getting the clip ready")}</h2>
    <p class="step">{label}</p>
    <div
      class="bar"
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={share === null ? undefined : Math.round(share * 100)}
    >
      {#if share === null}
        <span class="busy pending-loop"></span>
      {:else}
        <span class="fill" style:width="{share * 100}%"></span>
      {/if}
    </div>
    <p class="numbers mono meta">
      {t("{time} so far", { time: duration((now - started) / 1000) })}{#if left !== null}{` · ${t("about {time} left", { time: duration(left) })}`}{/if}
    </p>
    {#if step?.step === "splitting"}
      <p class="hint meta">{t("Long sessions take a moment the first time. After that they open right away.")}</p>
    {/if}
  </div>
{/if}

<style>
  .preparing {
    margin: auto;
    width: min(380px, 100%);
    padding: 22px 24px;
    border-radius: 24px;
    background: var(--overlay);
    display: flex;
    flex-direction: column;
    gap: 8px;
    animation: rise 240ms var(--travel) both;
  }
  h2 {
    margin: 0;
    font-size: 15px;
  }
  .step {
    margin: 0;
    font-size: 13px;
    font-weight: 600;
    color: var(--muted);
  }
  .bar {
    position: relative;
    height: 6px;
    margin: 6px 0 2px;
    border-radius: 3px;
    background: var(--black);
    overflow: hidden;
  }
  .fill {
    position: absolute;
    inset: 0 auto 0 0;
    border-radius: 3px;
    background: var(--accent);
    transition: width 480ms linear;
  }
  /* No share to show yet: a short piece slides along, so it is clear something happens. */
  .busy {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 30%;
    border-radius: 3px;
    background: var(--accent);
    animation: slide 1.4s ease-in-out infinite;
  }
  .numbers {
    margin: 0;
    font-size: 12px;
  }
  .hint {
    margin: 4px 0 0;
    font-size: 12px;
  }
  @keyframes slide {
    from {
      left: -30%;
    }
    to {
      left: 100%;
    }
  }
  @keyframes rise {
    from {
      opacity: 0;
      transform: translateY(4px);
    }
  }
</style>
