<script lang="ts">
  let { checked, label, onchange }: { checked: boolean; label: string; onchange: (v: boolean) => void } = $props();
</script>

<button
  type="button"
  role="switch"
  aria-checked={checked}
  aria-label={label}
  class="switch"
  class:on={checked}
  onclick={() => onchange(!checked)}
>
  <span></span>
</button>

<style>
  .switch {
    position: relative;
    flex: none;
    width: 44px;
    height: 26px;
    padding: 0;
    border: 0;
    border-radius: 13px;
    background: var(--hover);
    transition: background-color 220ms ease;
  }
  .switch span {
    position: absolute;
    top: 3px;
    left: 3px;
    width: 20px;
    height: 20px;
    border-radius: 50%;
    background: var(--muted);
    transition: left 220ms var(--travel), background-color 220ms ease;
  }
  .switch.on {
    background: var(--accent);
  }
  .switch.on span {
    left: 21px;
    background: #fff;
  }
</style>
