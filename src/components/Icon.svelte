<script lang="ts" module>
  // 20×20 stroke icons, drawn in currentColor.
  const PATHS: Record<string, string> = {
    clips: "M3 5.5A2.5 2.5 0 0 1 5.5 3h9A2.5 2.5 0 0 1 17 5.5v9a2.5 2.5 0 0 1-2.5 2.5h-9A2.5 2.5 0 0 1 3 14.5zM8.6 7.6v4.8l4-2.4z",
    screenshots: "M3 7V4h3M14 4h3v3M17 13v3h-3M6 16H3v-3M12.5 10a2.5 2.5 0 1 1-5 0 2.5 2.5 0 0 1 5 0z",
    sessions: "M16.5 10a6.5 6.5 0 1 1-13 0 6.5 6.5 0 0 1 13 0zM12.5 10a2.5 2.5 0 1 1-5 0 2.5 2.5 0 0 1 5 0z",
    drafts: "M4 16l1-4 8-8 3 3-8 8zM11.5 5.5l3 3",
    star: "M10 3l2.1 4.3 4.7.7-3.4 3.3.8 4.7-4.2-2.2-4.2 2.2.8-4.7L3.2 8l4.7-.7z",
    settings: "M3 6h14M3 14h14M9.7 6a2.2 2.2 0 1 1-4.4 0 2.2 2.2 0 0 1 4.4 0zM14.7 14a2.2 2.2 0 1 1-4.4 0 2.2 2.2 0 0 1 4.4 0z",
    search: "M14.5 9a5.5 5.5 0 1 1-11 0 5.5 5.5 0 0 1 11 0zM13 13l4 4",
    back: "M12 5l-5 5 5 5",
    chevronUp: "M6 12l4-4 4 4",
    chevronDown: "M6 8l4 4 4-4",
    more: "M5 10h.01M10 10h.01M15 10h.01",
    folder: "M3 6.5A1.5 1.5 0 0 1 4.5 5H8l1.5 2h6A1.5 1.5 0 0 1 17 8.5v6a1.5 1.5 0 0 1-1.5 1.5h-11A1.5 1.5 0 0 1 3 14.5z",
    trash: "M4 6h12M8 6V4h4v2M6 6l.7 10h6.6L14 6",
    camera: "M3 7h3l1.5-2h5L14 7h3v9H3zM12.8 11.5a2.8 2.8 0 1 1-5.6 0 2.8 2.8 0 0 1 5.6 0z",
    record: "M16.5 10a6.5 6.5 0 1 1-13 0 6.5 6.5 0 0 1 13 0z",
    close: "M5 5l10 10M15 5L5 15",
    check: "M4 10.5l4 4 8-9",
    game: "M6 7h8a4 4 0 0 1 3.9 4.9l-.6 2.5a2 2 0 0 1-3.4.9L12.5 14h-5l-1.4 1.3a2 2 0 0 1-3.4-.9l-.6-2.5A4 4 0 0 1 6 7zM7 9.5v2M6 10.5h2M13 10h.01",
    mic: "M7.5 5.5a2.5 2.5 0 0 1 5 0v4a2.5 2.5 0 0 1-5 0zM4.5 10a5.5 5.5 0 0 0 11 0M10 15.5V17",
    app: "M3 5.5A1.5 1.5 0 0 1 4.5 4h11A1.5 1.5 0 0 1 17 5.5v9a1.5 1.5 0 0 1-1.5 1.5h-11A1.5 1.5 0 0 1 3 14.5zM3 7.5h14",
    desktop: "M3 4.5h14v9H3zM7 16.5h6M10 13.5v3",
    speaker: "M4 8h3l4-3v10l-4-3H4zM14 7.5a3.5 3.5 0 0 1 0 5",
    export: "M10 13V3M6.5 6.5L10 3l3.5 3.5M4 11v4.5h12V11",
    image: "M3 5h14v10H3zM3 13l4-4 3 3 2-2 5 5",
    play: "M6.5 4.5l9 5.5-9 5.5z",
    pause: "M6.5 4.5h2.5v11H6.5zM11 4.5h2.5v11H11z",
    edit: "M4 16l1-4 8-8 3 3-8 8z",
    plus: "M10 4v12M4 10h12",
    scissors: "M8 6a2 2 0 1 1-4 0 2 2 0 0 1 4 0zM8 14a2 2 0 1 1-4 0 2 2 0 0 1 4 0zM7.6 7.2L16 14.5M7.6 12.8L16 5.5",
    restore: "M4.5 9.5a5.5 5.5 0 1 1 1.6 4.2M4.5 5.5v4h4",
    copy: "M7 7V4.5A1.5 1.5 0 0 1 8.5 3h7A1.5 1.5 0 0 1 17 4.5v7a1.5 1.5 0 0 1-1.5 1.5H13M4.5 7h7A1.5 1.5 0 0 1 13 8.5v7a1.5 1.5 0 0 1-1.5 1.5h-7A1.5 1.5 0 0 1 3 15.5v-7A1.5 1.5 0 0 1 4.5 7z",
    link: "M8.5 11.5l3-3M9 6l1.2-1.2a3 3 0 0 1 4.2 4.2L13.2 10M11 14l-1.2 1.2a3 3 0 0 1-4.2-4.2L6.8 10",
  };
  const FILLED = new Set(["play", "pause", "record"]);
</script>

<script lang="ts">
  let { name, size = 18, filled = false }: { name: string; size?: number; filled?: boolean } = $props();
  const solid = $derived(filled || FILLED.has(name));
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 20 20"
  aria-hidden="true"
  fill={solid ? "currentColor" : "none"}
  stroke={solid ? "none" : "currentColor"}
  stroke-width="1.7"
  stroke-linecap="round"
  stroke-linejoin="round"
>
  <path d={PATHS[name] ?? ""} />
</svg>
