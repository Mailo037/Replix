<script lang="ts" generics="T extends string | number | null">
  import { fit } from "../lib/place";
  import Icon from "./Icon.svelte";

  // A dropdown drawn by the app. The list sits in the top layer, so scrolling panels don't clip it.
  let {
    options,
    value,
    label,
    onchange,
    onopen,
    placeholder = "Choose…",
  }: {
    options: { value: T; label: string; hint?: string }[];
    value: T;
    label: string;
    onchange: (v: T) => void;
    /** Runs as the list opens, to refresh it. */
    onopen?: () => void;
    placeholder?: string;
  } = $props();

  const id = $props.id();
  let trigger = $state<HTMLButtonElement>();
  let list = $state<HTMLDivElement>();
  let open = $state(false);
  let active = $state(-1);
  let typed = "";
  let typedAt = 0;

  const chosen = $derived(options.find((o) => o.value === value));

  // Sized before it shows; placed once it can be measured, so it never runs out of the window.
  function onBeforeToggle(e: ToggleEvent) {
    if (e.newState !== "open" || !trigger || !list) return;
    onopen?.();
    active = Math.max(0, options.findIndex((o) => o.value === value));
    const r = trigger.getBoundingClientRect();
    list.style.visibility = "hidden";
    list.style.minWidth = `${Math.round(r.width)}px`;
    list.style.maxHeight = `${Math.round(Math.min(300, Math.max(innerHeight - r.bottom, r.top) - 20))}px`;
  }

  function onToggle(e: ToggleEvent) {
    open = e.newState === "open";
    if (open && trigger && list) {
      list.classList.toggle("up", fit(list, trigger.getBoundingClientRect()));
      list.style.visibility = "";
      list.focus();
      list?.querySelector<HTMLElement>(`[data-index="${active}"]`)?.scrollIntoView({ block: "nearest" });
    } else if (!document.activeElement || document.activeElement === document.body || list?.contains(document.activeElement)) {
      trigger?.focus();
    }
  }

  // The list stays where it opened, so it closes when anything underneath scrolls or resizes.
  $effect(() => {
    if (!open) return;
    const close = (e: Event) => e.target !== list && list?.hidePopover();
    addEventListener("scroll", close, true);
    addEventListener("resize", close);
    return () => {
      removeEventListener("scroll", close, true);
      removeEventListener("resize", close);
    };
  });

  $effect(() => {
    if (!list) return;
    const node = list;
    node.addEventListener("beforetoggle", onBeforeToggle as EventListener);
    node.addEventListener("toggle", onToggle as EventListener);
    return () => {
      node.removeEventListener("beforetoggle", onBeforeToggle as EventListener);
      node.removeEventListener("toggle", onToggle as EventListener);
    };
  });

  function pick(i: number) {
    const option = options[i];
    if (!option) return;
    if (option.value !== value) onchange(option.value);
    list?.hidePopover();
    trigger?.focus();
  }

  function move(to: number) {
    if (!options.length) return;
    active = (to + options.length) % options.length;
    list?.querySelector<HTMLElement>(`[data-index="${active}"]`)?.scrollIntoView({ block: "nearest" });
  }

  function onTriggerKey(e: KeyboardEvent) {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      // A click keeps the button as the list's invoker, so clicking it again closes the list.
      trigger?.click();
    }
  }

  function onListKey(e: KeyboardEvent) {
    if (e.key === "ArrowDown") move(active + 1);
    else if (e.key === "ArrowUp") move(active - 1);
    else if (e.key === "Home") move(0);
    else if (e.key === "End") move(options.length - 1);
    else if (e.key === "Enter" || e.key === " ") pick(active);
    else if (e.key === "Tab") {
      list?.hidePopover();
      return;
    } else if (e.key.length === 1 && !e.ctrlKey && !e.altKey) {
      // Typing jumps to the first entry that starts with what was typed.
      typed = Date.now() - typedAt < 700 ? typed + e.key.toLowerCase() : e.key.toLowerCase();
      typedAt = Date.now();
      const hit = options.findIndex((o) => o.label.toLowerCase().startsWith(typed));
      if (hit >= 0) move(hit);
    } else return;
    e.preventDefault();
  }
</script>

<button
  bind:this={trigger}
  type="button"
  class="select"
  class:open
  popovertarget="{id}-list"
  aria-haspopup="listbox"
  aria-expanded={open}
  aria-label="{label}: {chosen?.label ?? placeholder}"
  onkeydown={onTriggerKey}
>
  <span class="current" class:placeholder={!chosen}>{chosen?.label ?? placeholder}</span>
  <span class="chevron"><Icon name="chevronDown" size={16} /></span>
</button>

<div
  bind:this={list}
  id="{id}-list"
  popover="auto"
  class="options scroll"
  role="listbox"
  aria-label={label}
  tabindex="-1"
  aria-activedescendant={active >= 0 ? `${id}-${active}` : undefined}
  onkeydown={onListKey}
>
  {#each options as option, i (i)}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div
      id="{id}-{i}"
      data-index={i}
      role="option"
      tabindex="-1"
      aria-selected={option.value === value}
      class="option"
      class:active={i === active}
      onpointermove={() => (active = i)}
      onclick={() => pick(i)}
    >
      <span class="text">
        <span class="name">{option.label}</span>
        {#if option.hint}<span class="hint">{option.hint}</span>{/if}
      </span>
      {#if option.value === value}<span class="tick"><Icon name="check" size={16} /></span>{/if}
    </div>
  {/each}
</div>

<style>
  .select {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    min-width: 0;
    height: 30px;
    padding: 0 8px 0 10px;
    border: 0;
    border-radius: 8px;
    background: var(--surface);
    color: var(--ink);
    text-align: left;
    transition: background-color 160ms ease, box-shadow 160ms ease;
  }
  .select:hover {
    background: var(--raised);
  }
  .select.open {
    background: var(--raised);
    box-shadow: inset 0 0 0 1.5px var(--accent);
  }
  .current {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .placeholder {
    color: var(--ghost);
  }
  .chevron {
    display: flex;
    color: var(--ghost);
    transition: transform 200ms var(--travel);
  }
  .open .chevron {
    transform: rotate(180deg);
    color: var(--accent);
  }
  .options {
    position: fixed;
    inset: auto;
    margin: 0;
    max-width: min(420px, calc(100vw - 16px));
    max-height: calc(100vh - 16px);
    padding: 4px;
    border: 0;
    border-radius: 12px;
    background: var(--raised);
    color: var(--ink);
    box-shadow:
      0 0 0 1px rgb(255 255 255 / 0.04),
      0 14px 36px rgb(0 0 0 / 0.5);
    outline: none;
    overflow-y: auto;
  }
  .options:popover-open {
    display: flex;
    flex-direction: column;
    gap: 2px;
    animation: drop 160ms var(--travel);
  }
  .options:global(.up):popover-open {
    animation-name: lift;
  }
  @keyframes drop {
    from {
      opacity: 0;
      transform: translateY(-4px);
    }
  }
  @keyframes lift {
    from {
      opacity: 0;
      transform: translateY(4px);
    }
  }
  .option {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 32px;
    padding: 5px 8px 5px 10px;
    border-radius: 8px;
    cursor: pointer;
  }
  .option.active {
    background: var(--hover);
  }
  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .name,
  .hint {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .hint {
    font-size: 12px;
    color: var(--ghost);
  }
  .tick {
    display: flex;
    color: var(--accent);
  }
  @media (prefers-reduced-motion: reduce) {
    .options:popover-open {
      animation: none;
    }
  }
</style>
