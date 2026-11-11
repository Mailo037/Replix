<script lang="ts" module>
  // Replix's own icons: 20×20, one 1.7 stroke in currentColor, and round everywhere. Every
  // corner is rounded off, so boxes, triangles and stars sit next to circles without looking
  // harder than them. The rules for adding one are in AGENTS.md under "Icons".

  type Point = [number, number];

  const n = (v: number) => +v.toFixed(2);

  /** A rectangle whose corners are rounded with radius r. */
  function box(x: number, y: number, w: number, h: number, r: number): string {
    const [sw, sh] = [n(w - 2 * r), n(h - 2 * r)];
    const a = `a${r} ${r} 0 0 1`;
    return `M${n(x + r)} ${y}h${sw}${a} ${r} ${r}v${sh}${a}-${r} ${r}h-${sw}${a}-${r}-${r}v-${sh}${a} ${r}-${r}z`;
  }

  /** A circle around (cx, cy). */
  function ring(cx: number, cy: number, r: number): string {
    return `M${n(cx - r)} ${cy}a${r} ${r} 0 1 0 ${n(2 * r)} 0a${r} ${r} 0 1 0-${n(2 * r)} 0z`;
  }

  /**
   * A shape through `points` with every corner rounded off by up to r, so no tip is sharp.
   * With `open` it is a line: its two ends stay where they are (the stroke rounds them).
   */
  function soft(points: Point[], r: number, open = false): string {
    const at = (i: number) => points[(i + points.length) % points.length];
    let d = "";
    points.forEach((p, i) => {
      const move = i === 0 ? "M" : "L";
      if (open && (i === 0 || i === points.length - 1)) {
        d += `${move}${n(p[0])} ${n(p[1])}`;
        return;
      }
      const toward = (q: Point): Point => {
        const [dx, dy] = [q[0] - p[0], q[1] - p[1]];
        const length = Math.hypot(dx, dy);
        const k = Math.min(r, length / 2) / length;
        return [p[0] + dx * k, p[1] + dy * k];
      };
      const [a, b] = [toward(at(i - 1)), toward(at(i + 1))];
      d += `${move}${n(a[0])} ${n(a[1])}Q${n(p[0])} ${n(p[1])} ${n(b[0])} ${n(b[1])}`;
    });
    return open ? d : `${d}z`;
  }

  /** The five points and five dents of a star around (cx, cy). */
  function starPoints(cx: number, cy: number, outer: number, inner: number): Point[] {
    return Array.from({ length: 10 }, (_, i) => {
      const angle = -Math.PI / 2 + (i * Math.PI) / 5;
      const r = i % 2 ? inner : outer;
      return [cx + r * Math.cos(angle), cy + r * Math.sin(angle)];
    });
  }

  // Shared pieces, so icons that show the same thing draw it the same way.
  const FRAME = // four rounded corners of a frame, as on a viewfinder
    "M3 7.5v-2a2.5 2.5 0 0 1 2.5-2.5h2M12.5 3h2a2.5 2.5 0 0 1 2.5 2.5v2M17 12.5v2a2.5 2.5 0 0 1-2.5 2.5h-2M7.5 17h-2A2.5 2.5 0 0 1 3 14.5v-2";
  const PENCIL = soft([[4, 16], [5, 12], [13, 4], [16, 7], [8, 15]], 1.2);
  const SPEAKER = soft([[3.5, 7.5], [7, 7.5], [11, 4.5], [11, 15.5], [7, 12.5], [3.5, 12.5]], 1.2);
  const TRAY = "M3.5 12v2a2.5 2.5 0 0 0 2.5 2.5h8a2.5 2.5 0 0 0 2.5-2.5v-2";

  const PATHS: Record<string, string> = {
    clips: box(3, 3.5, 14, 13, 3) + soft([[8.5, 7.5], [12.5, 10], [8.5, 12.5]], 0.9),
    screenshots: FRAME + ring(10, 10, 2.5),
    sessions: ring(10, 10, 6.5) + ring(10, 10, 2.5),
    drafts: PENCIL + "M11.5 5.5l3 3",
    star: soft(starPoints(10, 10.4, 7.2, 3.5), 1.2),
    settings: "M3 6h14M3 14h14" + ring(7.5, 6, 2.2) + ring(12.5, 14, 2.2),
    search: ring(9, 9, 5.5) + "M13 13l4 4",
    back: "M12 5l-5 5 5 5",
    expand: FRAME,
    shrink: "M7.5 3v2.5a2 2 0 0 1-2 2H3M12.5 3v2.5a2 2 0 0 0 2 2H17M12.5 17v-2.5a2 2 0 0 1 2-2H17M7.5 17v-2.5a2 2 0 0 0-2-2H3",
    chevronUp: "M6 12l4-4 4 4",
    chevronDown: "M6 8l4 4 4-4",
    more: "M5 10h.01M10 10h.01M15 10h.01",
    folder: "M3 6.5a2 2 0 0 1 2-2h2.6a1.5 1.5 0 0 1 1.2.6l1 1.4h5.2a2 2 0 0 1 2 2v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z",
    trash: "M3.5 6h13M8 6V4.5a1 1 0 0 1 1-1h2a1 1 0 0 1 1 1V6M5 6l.8 9.2a2 2 0 0 0 2 1.8h4.4a2 2 0 0 0 2-1.8L15 6",
    camera:
      "M3 8.5a2 2 0 0 1 2-2h1.5l1.2-1.6a1 1 0 0 1 .8-.4h3a1 1 0 0 1 .8.4l1.2 1.6H15a2 2 0 0 1 2 2V14a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" +
      ring(10, 11.2, 2.6),
    record: ring(10, 10, 5.6),
    close: "M5 5l10 10M15 5L5 15",
    check: "M4 10.5l4 4 8-9",
    game: "M6 7h8a4 4 0 0 1 3.9 4.9l-.6 2.5a2 2 0 0 1-3.4.9L12.5 14h-5l-1.4 1.3a2 2 0 0 1-3.4-.9l-.6-2.5A4 4 0 0 1 6 7zM7 9.5v2M6 10.5h2M13 10h.01",
    mic: "M7.5 5.5a2.5 2.5 0 0 1 5 0v4a2.5 2.5 0 0 1-5 0zM4.5 10a5.5 5.5 0 0 0 11 0M10 15.5V17",
    app: box(3, 4, 14, 12, 2.5) + "M3 7.5h14",
    desktop: box(2.5, 3.5, 15, 10, 2.5) + "M7 16.5h6M10 13.5v3",
    speaker: SPEAKER + "M14 7.5a3.5 3.5 0 0 1 0 5",
    mute: SPEAKER + "M14 8l3.5 4M17.5 8L14 12",
    export: "M10 12.5V3M6.5 6.5L10 3l3.5 3.5" + TRAY,
    import: "M10 3v9.5M6.5 9L10 12.5 13.5 9" + TRAY,
    image:
      box(3, 4, 14, 12, 2.5) +
      "M3.2 13.2l3.6-3.6a1.2 1.2 0 0 1 1.7 0l6.4 6.4M11.5 12.5l1.3-1.3a1.2 1.2 0 0 1 1.7 0l2.4 2.4" +
      ring(13, 7.8, 1.3),
    play: soft([[6.5, 5], [15, 10], [6.5, 15]], 1.5),
    pause: box(6, 5, 1.9, 10, 0.95) + box(12.1, 5, 1.9, 10, 0.95),
    flag: "M5 17.5V3" + soft([[5, 4], [14.5, 4], [12.3, 7.75], [14.5, 11.5], [5, 11.5]], 1.2, true),
    prevMarker: "M14 5l-5 5 5 5M6 5v10",
    nextMarker: "M6 5l5 5-5 5M14 5v10",
    heart: "M10 16.5S3.5 12.6 3.5 8A3.3 3.3 0 0 1 10 6.3 3.3 3.3 0 0 1 16.5 8c0 4.6-6.5 8.5-6.5 8.5z",
    trophy:
      "M6.5 5a1.5 1.5 0 0 1 1.5-1.5h4A1.5 1.5 0 0 1 13.5 5v2.5a3.5 3.5 0 0 1-7 0z" +
      "M6.5 5H5a1 1 0 0 0-1 1 2.5 2.5 0 0 0 2.6 2.5M13.5 5H15a1 1 0 0 1 1 1 2.5 2.5 0 0 1-2.6 2.5M10 11v4.5M7 16.5h6",
    skull:
      "M10 3a6 6 0 0 0-6 6c0 1.9.9 3.2 2 4v1.5A1.5 1.5 0 0 0 7.5 16h5a1.5 1.5 0 0 0 1.5-1.5V13c1.1-.8 2-2.1 2-4a6 6 0 0 0-6-6z" +
      "M7.5 9.5h.01M12.5 9.5h.01M10 16v-1.8",
    target: ring(10, 10, 5.5) + "M10 2.5v4M10 13.5v4M2.5 10h4M13.5 10h4",
    bolt: soft([[11, 2.5], [4.5, 11], [10, 11], [9, 17.5], [15.5, 9], [10, 9]], 1),
    smile: ring(10, 10, 7) + "M7 11.5a3.5 3.5 0 0 0 6 0M7.5 8h.01M12.5 8h.01",
    alert: soft([[10, 3.5], [17, 16], [3, 16]], 2) + "M10 8.5v3M10 13.8h.01",
    shield:
      "M9.2 3.3a2 2 0 0 1 1.6 0l4.3 1.8a1.5 1.5 0 0 1 .9 1.4v3c0 3.6-2.6 6.4-6 7.5-3.4-1.1-6-3.9-6-7.5v-3a1.5 1.5 0 0 1 .9-1.4z",
    globe: ring(10, 10, 7) + "M3 10h14M10 3c2 2 2.8 4.4 2.8 7s-.8 5-2.8 7c-2-2-2.8-4.4-2.8-7S8 5 10 3z",
    edit: PENCIL,
    plus: "M10 4v12M4 10h12",
    scissors: ring(6, 6, 2) + ring(6, 14, 2) + "M7.6 7.2L16 14.5M7.6 12.8L16 5.5",
    undo: "M7.5 5L4 8.5 7.5 12M4 8.5h8a4 4 0 0 1 0 8h-2",
    redo: "M12.5 5L16 8.5 12.5 12M16 8.5H8a4 4 0 0 0 0 8h2",
    restore: "M4.5 9.5a5.5 5.5 0 1 1 1.6 4.2M4.5 5.5v4h4",
    copy: box(3, 7, 10, 10, 2) + "M7 7V5a2 2 0 0 1 2-2h6a2 2 0 0 1 2 2v6a2 2 0 0 1-2 2h-2",
    link: "M8.5 11.5l3-3M9 6l1.2-1.2a3 3 0 0 1 4.2 4.2L13.2 10M11 14l-1.2 1.2a3 3 0 0 1-4.2-4.2L6.8 10",
  };
  // Drawn solid. They keep the stroke too, so a filled icon has the same round outline and
  // size as its outlined form (a star turning solid does not shrink).
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
  stroke="currentColor"
  stroke-width="1.7"
  stroke-linecap="round"
  stroke-linejoin="round"
>
  <path d={PATHS[name] ?? ""} />
</svg>
