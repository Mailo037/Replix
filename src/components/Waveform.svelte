<script lang="ts" module>
  import { api } from "../lib/api";
  import { currentRound, editorRound } from "../lib/editorRound.svelte";
  import { store } from "../lib/store.svelte";

  // Peaks per track, so reopening a clip doesn't ask for them again; the newest few dozen
  // are kept. Replix streams the track through FFmpeg for them, so even an hour-long
  // session costs little memory, and keeps them on disk, so asking again later is cheap.
  const KEEP = 48;
  const done = new Map<string, Float32Array>();
  /** Requests on their way, by round: one from an ended round is not joined. */
  const asking = new Map<string, Promise<Float32Array | null>>();

  function peaks(id: string, track: number, duration: number): Promise<Float32Array | null> {
    const key = `${id}:${track}`;
    const kept = done.get(key);
    if (kept) {
      // Most recently used goes last; the first one is the next to go.
      done.delete(key);
      done.set(key, kept);
      return Promise.resolve(kept);
    }
    const round = currentRound();
    const ask = `${round}:${key}`;
    let p = asking.get(ask);
    if (!p) {
      p = api
        .waveform(id, track, duration, round)
        .then(
          (v) => {
            const peaks = Float32Array.from(v);
            done.set(key, peaks);
            if (done.size > KEEP) done.delete(done.keys().next().value!);
            return peaks;
          },
          // Stopped or failed: nothing is kept, so the next time asks again.
          () => null,
        )
        .finally(() => asking.delete(ask));
      asking.set(ask, p);
    }
    return p;
  }
</script>

<script lang="ts">
  // The track's loudness over time, drawn as bars across its lane.
  let { id, track, duration, muted = false }: { id: string; track: number; duration: number; muted?: boolean } = $props();

  let canvas = $state<HTMLCanvasElement>();
  let data = $state<Float32Array | null>(null);
  let width = $state(0);
  let height = $state(0);

  $effect(() => {
    // Hidden, the editor stops its waveforms; they are asked for again once it is back.
    // A new round asks again for what the last one did not finish.
    void editorRound.n;
    if (!store.visible || data) return;
    let current = true;
    peaks(id, track, duration).then((d) => current && (data = d));
    return () => (current = false);
  });

  $effect(() => {
    if (!canvas || !width || !height) return;
    const ratio = devicePixelRatio || 1;
    canvas.width = Math.round(width * ratio);
    canvas.height = Math.round(height * ratio);
    const ctx = canvas.getContext("2d")!;
    ctx.scale(ratio, ratio);
    ctx.clearRect(0, 0, width, height);
    const color = getComputedStyle(canvas).color;
    ctx.fillStyle = color;
    const bar = 2;
    const gap = 1;
    const count = Math.floor(width / (bar + gap));
    const mid = height / 2;
    for (let i = 0; i < count; i++) {
      const v = data ? data[Math.floor((i / count) * data.length)] : 0.06;
      const h = Math.max(1.5, v * (height - 6));
      ctx.fillRect(i * (bar + gap), mid - h / 2, bar, h);
    }
    void muted;
  });
</script>

<div class="wave" class:muted bind:clientWidth={width} bind:clientHeight={height}>
  <canvas bind:this={canvas}></canvas>
</div>

<style>
  .wave {
    position: absolute;
    inset: 0;
    color: color-mix(in srgb, var(--accent) 70%, transparent);
  }
  .wave.muted {
    color: rgb(255 255 255 / 0.14);
  }
  canvas {
    display: block;
    width: 100%;
    height: 100%;
  }
</style>
