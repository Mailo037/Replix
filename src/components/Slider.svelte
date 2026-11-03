<script lang="ts">
  // A range input with the app's own track and knob; keyboard and screen readers work as usual.
  let {
    value,
    min = 0,
    max = 100,
    step = 1,
    label,
    valuetext,
    disabled = false,
    oninput,
  }: {
    value: number;
    min?: number;
    max?: number;
    step?: number;
    label: string;
    valuetext?: string;
    disabled?: boolean;
    oninput: (v: number) => void;
  } = $props();

  const fill = $derived(max > min ? ((value - min) / (max - min)) * 100 : 0);
</script>

<input
  type="range"
  class="slider"
  {min}
  {max}
  {step}
  {value}
  {disabled}
  aria-label={label}
  aria-valuetext={valuetext}
  style:--fill="{fill}%"
  oninput={(e) => oninput(Number(e.currentTarget.value))}
/>

<style>
  .slider {
    appearance: none;
    display: block;
    width: 100%;
    height: 22px;
    margin: 0;
    background: transparent;
    cursor: pointer;
  }
  .slider:focus-visible {
    outline: none;
  }
  .slider::-webkit-slider-runnable-track {
    height: 4px;
    border-radius: 2px;
    background: linear-gradient(to right, var(--accent) var(--fill), var(--black) var(--fill));
  }
  .slider::-webkit-slider-thumb {
    appearance: none;
    width: 14px;
    height: 14px;
    margin-top: -5px;
    border-radius: 50%;
    background: var(--ink);
    box-shadow: 0 1px 4px rgb(0 0 0 / 0.5);
    transition: transform 140ms var(--travel);
  }
  .slider:hover::-webkit-slider-thumb,
  .slider:active::-webkit-slider-thumb {
    transform: scale(1.15);
  }
  .slider:focus-visible::-webkit-slider-thumb {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  .slider:disabled {
    cursor: default;
  }
  .slider:disabled::-webkit-slider-runnable-track {
    background: linear-gradient(to right, var(--hover) var(--fill), var(--black) var(--fill));
  }
  .slider:disabled::-webkit-slider-thumb {
    background: var(--ghost);
    transform: none;
  }
</style>
