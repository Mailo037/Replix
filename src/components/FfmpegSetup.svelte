<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import { api, type FfmpegSetup } from "../lib/api";

  // FFmpeg is not in the installer: on first start Replix downloads it, and records once it is there.
  let setup = $state<FfmpegSetup>({ state: "ready" });

  onMount(() => {
    const unlisten = listen<FfmpegSetup>("ffmpeg-setup", (e) => (setup = e.payload));
    // Asked after listening, so a change in between is not missed.
    unlisten.then(() => api.ffmpegSetup().then((s) => (setup = s)));
    return () => {
      unlisten.then((f) => f());
    };
  });

  const mb = (bytes: number) => Math.round(bytes / 1024 / 1024);
  const share = $derived(setup.state === "downloading" && setup.total > 0 ? setup.done / setup.total : 0);
</script>

{#if setup.state !== "ready"}
  <div class="setup" role="status" aria-live="polite">
    <span class="text">
      {#if setup.state === "failed"}
        <strong>FFmpeg is missing</strong>
        <span class="error" title={setup.error}>{setup.error}</span>
      {:else}
        <strong>Getting FFmpeg ready</strong>
        <span>
          {#if setup.state === "downloading"}
            Downloading, {mb(setup.done)} of {mb(setup.total)} MB. Recording starts when it is done.
          {:else if setup.state === "unpacking"}
            Unpacking…
          {:else}
            Checking…
          {/if}
        </span>
      {/if}
    </span>
    {#if setup.state === "failed"}
      <button type="button" class="btn small primary" onclick={() => api.retryFfmpeg()}>Try again</button>
    {:else}
      <span class="bar" aria-hidden="true"><span class="fill" style:width="{Math.round(share * 100)}%"></span></span>
    {/if}
  </div>
{/if}

<style>
  .setup {
    position: absolute;
    left: 50%;
    bottom: 20px;
    z-index: 55;
    transform: translateX(-50%);
    display: flex;
    align-items: center;
    gap: 16px;
    width: min(520px, calc(100% - 48px));
    padding: 12px 14px 12px 18px;
    border-radius: 22px;
    background: var(--raised);
    animation: up 240ms var(--travel);
  }
  @keyframes up {
    from {
      opacity: 0;
      transform: translate(-50%, 8px);
    }
  }
  .text {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
    font-size: 13px;
  }
  .text span {
    color: var(--muted);
  }
  .text .error {
    color: var(--red);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .bar {
    flex: none;
    width: 96px;
    height: 6px;
    border-radius: 999px;
    background: var(--surface);
    overflow: hidden;
  }
  .fill {
    display: block;
    height: 100%;
    border-radius: inherit;
    background: var(--accent);
    transition: width 200ms linear;
  }
</style>
