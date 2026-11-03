<script lang="ts" module>
  // Peaks per file, so reopening a clip doesn't decode its audio again.
  const cache = new Map<string, Promise<Float32Array | null>>();
  const BUCKETS = 1200;

  async function peaks(src: string): Promise<Float32Array | null> {
    try {
      const buffer = await (await fetch(src)).arrayBuffer();
      const audio = await new OfflineAudioContext(1, 1, 44100).decodeAudioData(buffer);
      const out = new Float32Array(BUCKETS);
      const per = Math.max(1, Math.floor(audio.length / BUCKETS));
      for (let c = 0; c < audio.numberOfChannels; c++) {
        const data = audio.getChannelData(c);
        for (let b = 0; b < BUCKETS; b++) {
          let max = 0;
          // Every fourth sample is plenty for a picture this size.
          for (let i = b * per, end = Math.min(data.length, i + per); i < end; i += 4) max = Math.max(max, Math.abs(data[i]));
          out[b] = Math.max(out[b], max);
        }
      }
      const loudest = out.reduce((m, v) => Math.max(m, v), 0) || 1;
      return out.map((v) => Math.sqrt(v / loudest));
    } catch {
      return null;
    }
  }
</script>

<script lang="ts">
  // The track's loudness over time, drawn as bars across its lane.
  let { src, muted = false, enabled = true }: { src: string; muted?: boolean; enabled?: boolean } = $props();

  let canvas = $state<HTMLCanvasElement>();
  let data = $state<Float32Array | null>(null);
  let width = $state(0);
  let height = $state(0);

  $effect(() => {
    if (!enabled) return;
    let p = cache.get(src);
    if (!p) cache.set(src, (p = peaks(src)));
    p.then((d) => (data = d));
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
