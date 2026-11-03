<script lang="ts" generics="T extends string | number">
  let {
    options,
    value,
    label,
    onchange,
    track = "var(--canvas)",
  }: {
    options: { value: T; label: string }[];
    value: T;
    label: string;
    onchange: (v: T) => void;
    track?: string;
  } = $props();
</script>

<div class="segmented" role="group" aria-label={label} style:background={track}>
  {#each options as option (option.value)}
    <button
      type="button"
      aria-pressed={option.value === value}
      class:active={option.value === value}
      onclick={() => onchange(option.value)}
    >
      {option.label}
    </button>
  {/each}
</div>

<style>
  .segmented {
    display: grid;
    grid-auto-flow: column;
    grid-auto-columns: minmax(0, 1fr);
    gap: 2px;
    padding: 3px;
    border-radius: 10px;
  }
  button {
    height: 26px;
    padding: 0 10px;
    border: 0;
    border-radius: 7px;
    background: transparent;
    color: var(--muted);
    font-size: 12px;
    font-weight: 500;
    white-space: nowrap;
    transition: background-color 220ms ease, color 220ms ease;
  }
  button:hover {
    color: var(--ink);
  }
  button.active {
    background: var(--hover);
    color: var(--ink);
  }
</style>
