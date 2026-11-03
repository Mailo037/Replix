<script lang="ts">
  // The Replix mark: a play symbol built from solid cells.
  let { size = 19, color = "var(--accent)" }: { size?: number; color?: string } = $props();
  const cells = [
    [0, 0], [0, 4], [0, 8], [0, 12], [0, 16],
    [4, 4], [4, 8], [4, 12],
    [8, 8],
  ];
</script>

<svg width={(size * 11) / 19} height={size} viewBox="0 0 11 19" aria-hidden="true" fill={color}>
  {#each cells as [x, y]}
    <rect {x} {y} width="3" height="3" rx="0.4" />
  {/each}
</svg>
