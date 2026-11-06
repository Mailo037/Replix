<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import { api } from "../lib/api";
  import { store } from "../lib/store.svelte";
  import { t } from "../lib/i18n.svelte";
  import { isMac, isWindows } from "../lib/platform";

  // Live microphone level while it is on screen, drawn as a row of characters.
  let { device }: { device: string | null } = $props();

  const CELLS = 40;
  /** Above this cell the signal is close to clipping. */
  const HOT = Math.floor(CELLS * 0.88);

  let level = $state(0);
  let peak = $state(0);
  let clippedAt = $state(0);
  let soundAt = $state(0);
  let signalAt = $state(0);
  let startedAt = $state(performance.now());
  let now = $state(performance.now());

  $effect(() => {
    // Nobody watches the meter while Replix is hidden; the microphone rests too, and the
    // test starts again on return. Beside another program it goes on: the test may be of
    // that program's voice chat.
    if (!store.visible) return;
    api.startMicTest(device);
    startedAt = performance.now();
    soundAt = 0;
    signalAt = 0;
    const timer = setInterval(() => (now = performance.now()), 500);
    return () => {
      api.stopMicTest();
      clearInterval(timer);
    };
  });

  onMount(() => {
    const unlisten = listen<number>("mic-level", (e) => {
      const at = performance.now();
      const db = 20 * Math.log10(Math.max(e.payload, 1e-5));
      // -60 dB to 0 dB across the bar.
      const v = Math.min(1, Math.max(0, (db + 60) / 60));
      level = Math.max(v, level - 0.07);
      peak = Math.max(v, peak - 0.015);
      signalAt = at;
      if (db > -50) soundAt = at;
      if (e.payload >= 0.98) clippedAt = at;
      now = at;
    });
    return () => {
      unlisten.then((f) => f());
    };
  });

  const filled = $derived(Math.round(level * CELLS));
  const peakCell = $derived(Math.min(CELLS - 1, Math.round(peak * CELLS) - 1));

  const message = $derived.by(() => {
    // The stream itself stopped or never started: unplugged, in use, or blocked by the system.
    if (now - Math.max(signalAt, startedAt) > 2500)
      return {
        text: isWindows
          ? t("No signal from this mic. Check it's plugged in and allowed in the Windows privacy settings.")
          : isMac
            ? t("No signal from this mic. Check it's plugged in and allowed under System Settings › Privacy & Security › Microphone.")
            : t("No signal from this mic. Check it's plugged in and not muted in the sound settings."),
        warn: true,
      };
    if (now - clippedAt < 1200) return { text: t("Too loud. Turn the volume down a little."), warn: true };
    if (soundAt && now - soundAt < 2500) return { text: t("Hearing you."), warn: false };
    if (now - startedAt > 8000) return { text: t("Nothing heard yet. If you're talking, check the mic isn't muted."), warn: false };
    return { text: t("Say something and watch the bar."), warn: false };
  });
</script>

<div class="meter">
  <div
    class="cells mono"
    role="meter"
    aria-label={t("Microphone level")}
    aria-valuemin={0}
    aria-valuemax={100}
    aria-valuenow={Math.round(level * 100)}
  >
    {#each { length: CELLS } as _, i (i)}<span class:on={i < filled} class:hot={i >= HOT} class:peak={i === peakCell && i >= filled}
        >{i < filled || i === peakCell ? "|" : "·"}</span
      >{/each}
  </div>
  <span class="meta" class:warn={message.warn} role="status">{message.text}</span>
</div>

<style>
  .meter {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .cells {
    display: flex;
    justify-content: space-between;
    height: 36px;
    align-items: center;
    padding: 0 12px;
    border-radius: 10px;
    background: var(--black);
    font-size: 14px;
    line-height: 1;
    color: var(--line);
    overflow: hidden;
  }
  .cells span {
    width: 0.6em;
    text-align: center;
    transition: color 60ms linear;
  }
  .on {
    color: var(--accent);
  }
  .on.hot {
    color: var(--red);
  }
  .peak {
    color: var(--muted);
  }
  .warn {
    color: var(--red);
  }
</style>
