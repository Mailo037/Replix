<script lang="ts">
  import { t } from "../lib/i18n.svelte";
  import { fit } from "../lib/place";
  import { store } from "../lib/store.svelte";
  import Icon from "./Icon.svelte";

  /**
   * Tags for one clip or several: the ones they wear, each with a cross to take it off, and a
   * field for a new one that suggests tags used on other clips.
   */
  let {
    tags,
    anchor,
    onadd,
    onremove,
    onclose,
  }: {
    tags: string[];
    /** What the panel opens next to. */
    anchor: DOMRect;
    onadd: (tag: string) => void;
    onremove: (tag: string) => void;
    onclose: () => void;
  } = $props();

  let panel = $state<HTMLDivElement>();
  let input = $state<HTMLInputElement>();
  let draft = $state("");

  const has = (tag: string) => tags.some((x) => x.toLowerCase() === tag.toLowerCase());
  const suggestions = $derived.by(() => {
    const q = draft.trim().replace(/^#+/, "").toLowerCase();
    return store.tags.filter((tag) => !has(tag.name) && tag.name.toLowerCase().includes(q)).slice(0, 6);
  });

  $effect(() => {
    const node = panel;
    if (!node) return;
    node.showPopover();
    fit(node, anchor, { align: "end" });
    input?.focus();
    const toggled = (e: Event) => (e as ToggleEvent).newState === "closed" && onclose();
    node.addEventListener("toggle", toggled);
    return () => node.removeEventListener("toggle", toggled);
  });

  function add(tag = draft) {
    // As Replix keeps it: no leading #, spaces collapsed, at most 32 characters.
    const tidy = tag.trim().replace(/^#+/, "").replace(/\s+/g, " ").slice(0, 32);
    draft = "";
    if (tidy && !has(tidy)) onadd(tidy);
    input?.focus();
  }
</script>

<div class="tags-panel" role="dialog" aria-label={t("Tags")} popover="auto" bind:this={panel}>
  {#if tags.length}
    <ul class="chips">
      {#each tags as tag (tag)}
        <li class="chip">
          <span>#{tag}</span>
          <button type="button" aria-label={t("Remove the tag {tag}", { tag })} onclick={() => onremove(tag)}>
            <Icon name="close" size={12} />
          </button>
        </li>
      {/each}
    </ul>
  {/if}
  <label class="field">
    <Icon name="tag" size={16} />
    <input
      bind:this={input}
      bind:value={draft}
      placeholder={t("Add a tag")}
      aria-label={t("Add a tag")}
      maxlength="33"
      spellcheck="false"
      onkeydown={(e) => {
        // The library's own keys (Delete, Ctrl+A) stay out of the field.
        e.stopPropagation();
        if (e.key === "Enter") {
          e.preventDefault();
          add();
        } else if (e.key === "Escape") panel?.hidePopover();
      }}
    />
  </label>
  {#if suggestions.length}
    <div class="suggest" role="group" aria-label={t("Tags on other clips")}>
      {#each suggestions as tag (tag.name)}
        <button type="button" onclick={() => add(tag.name)}>#{tag.name}</button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .tags-panel {
    position: fixed;
    inset: auto;
    margin: 0;
    width: 260px;
    padding: 8px;
    border: 0;
    border-radius: 20px;
    background: var(--raised);
    color: var(--ink);
    box-shadow:
      0 0 0 1px rgb(255 255 255 / 0.04),
      0 14px 36px rgb(0 0 0 / 0.5);
  }
  .tags-panel:popover-open {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .chip {
    display: flex;
    align-items: center;
    gap: 2px;
    height: 26px;
    padding: 0 2px 0 10px;
    border-radius: 999px;
    background: var(--surface);
    font-size: 12.5px;
    font-weight: 600;
  }
  .chip button {
    width: 22px;
    height: 22px;
    padding: 0;
    display: grid;
    place-items: center;
    border: 0;
    border-radius: 50%;
    background: transparent;
    color: var(--muted);
  }
  .chip button:hover {
    background: var(--hover);
    color: var(--ink);
  }
  .field {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 34px;
    padding: 0 12px;
    border-radius: 14px;
    background: var(--black);
    color: var(--muted);
  }
  .field:focus-within {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }
  .field input {
    flex: 1;
    min-width: 0;
    border: 0;
    background: transparent;
    color: var(--ink);
    font: inherit;
    font-weight: 600;
    outline: none;
  }
  .suggest {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .suggest button {
    height: 26px;
    padding: 0 10px;
    border: 0;
    border-radius: 999px;
    background: transparent;
    color: var(--muted);
    font-size: 12.5px;
    font-weight: 600;
    box-shadow: inset 0 0 0 1px var(--line);
  }
  .suggest button:hover {
    background: var(--hover);
    color: var(--ink);
  }
</style>
