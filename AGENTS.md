# Working on Replix

Replix is a Tauri 2 app: a Svelte page (`src/`) and a Rust backend (`src-tauri/`). It records
with FFmpeg as a helper process. [PRODUCT.md](PRODUCT.md) says what it is for and how it should
feel; [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) how to build, test and release it.

## Every change works on Windows, Linux and macOS

Replix ships for all three systems. Anything added or changed must meet these criteria; a
change that works on one system only is not done.

### 1. It builds and is clean everywhere

- `cargo clippy --all-targets -- -D warnings` passes for Windows, Linux and macOS. From one
  machine: `npm run check:all` (add `-- -j 2` on a machine with little memory). CI runs clippy
  and the tests on all three (`.github/workflows/ci.yml`); keep it green.
- The tests pass on every system. A test that only makes sense on one system sits behind the
  same `#[cfg(...)]` as the code it tests; shared logic is tested without one.
- The page has tests of its own: `npm test` (logic) and `npm run test:e2e` (the page in a
  browser, with Tauri stood in for). A change to the page keeps both green; a new command
  the page sends gets an answer in `tests/e2e/tauri.ts`.

### 2. System-specific code stays in its place

What differs between the systems lives only here, one file per system with the same functions:

| Where | What |
| --- | --- |
| `src-tauri/src/platform/` (`windows.rs`, `linux.rs`, `macos.rs`, `unix.rs`) | windows, processes, monitors, how FFmpeg grabs the picture (`capture`, `Capture`), helper processes (`background`, `prepare_tied`), disk space, language |
| `src-tauri/src/audio/` (`windows.rs`, `linux.rs`, `macos.rs`) | capturing sound: `stream`, `input_devices`, `audio_sessions` |
| `src-tauri/src/pipe/` | the pipes that feed audio into FFmpeg |
| `CANDIDATES` in `src-tauri/src/encoder.rs` | the video encoders to try on each system |
| `clipboard` in `src-tauri/src/share.rs` | putting files and text on the clipboard |
| `src-tauri/src/install.rs` | getting FFmpeg (downloaded on Windows, the system's elsewhere) |
| `src/lib/platform.ts` | the page's few differences: words and paths |

Everything else calls these and contains no `#[cfg(windows)]`-style switches of its own,
and no `windows::`, `wasapi`, `webview2_com`, `libc`, `x11rb`, `core_graphics` or
`core_foundation` imports.

### 3. A new system function exists on every system

- Adding a function to `platform`, `audio` or `pipe` means adding it to every system's file,
  with the same signature. CI fails on the system that lacks it.
- Where a system cannot do something, its version still exists and does the sensible
  nearest thing (returns `None`, an empty list, does nothing), with a doc comment saying why.
  It never panics and never leaves a feature broken without telling the person: a capability
  that is missing shows up in the status (`platform::capture_unavailable`) or as a clear
  error the page can show.
- Dependencies for one system go under `[target.'cfg(...)'.dependencies]` in `Cargo.toml`.
  Gate with `#[cfg(windows)]`, `#[cfg(target_os = "linux")]`, `#[cfg(target_os = "macos")]`
  or `#[cfg(unix)]` for what Linux and macOS share.

### 4. Processes, paths and FFmpeg are system-neutral

- Start helpers with `platform::background(&mut cmd, low_priority)` and `.spawn_tied()`
  (`ffmpeg::SpawnTied`) when they must end with Replix. Never call `creation_flags` or other
  Windows process options outside `platform/windows.rs`.
- Build paths with `Path`/`PathBuf`; never write `\`, `/` or `.exe` into a path by hand. Use
  `std::env::consts::EXE_SUFFIX` and `platform::exe_name`. Executable names are compared
  ignoring case. On the page, compare paths with `normalPath`, `isInside` and `endsWithExe`
  from `src/lib/platform.ts`.
- Picture sources come from `platform::capture`; never use `gfxcapture`, `ddagrab`,
  `hwdownload`, `d3d11`, `x11grab` or `avfoundation` outside the system's own file and
  `CANDIDATES`. Linux and macOS use the system's FFmpeg, so options and filters must exist in
  FFmpeg 6 (the oldest Replix accepts there; `install.rs` checks it).
- Shell tools Replix calls on Linux (`pactl`, `parec`, `wl-copy`, `xclip`) are listed as
  package dependencies in `src-tauri/tauri.linux.conf.json` and in the docs.

### 5. The page speaks to every system

- No text names one system for everyone. Where the word differs ("Windows default",
  "recycle bin" against "Trash"), use `isWindows` / `isMac` / `isLinux` from
  `src/lib/platform.ts` and give each its own text: `isWindows ? t("…") : t("…")`.
- Every new text is translated into all 12 languages; `node scripts/i18n.mjs --check`
  reports every language complete.
- Keyboard shortcuts on the page accept Ctrl or Cmd (`e.ctrlKey || e.metaKey`).

### 6. Bundling and docs keep up

- Settings for one system's bundle go in `src-tauri/tauri.linux.conf.json` or
  `src-tauri/tauri.macos.conf.json`; macOS permissions (microphone and the like) in
  `src-tauri/Info.plist`.
- When a feature works differently or not at all on a system, update the
  "Linux and macOS" table in [README.md](README.md).

## Style

- Comments and docs say what something is for and why, in plain words, as the existing
  ones do.
- English is the source language of every text; translations live in `src/locales/`.

## Icons

Replix draws its own icons; there is no icon library. All of them live in
`src/components/Icon.svelte` and are used as `<Icon name="…" size={16} />`. They should look
like one set: soft and round, never boxy or pointed.

- **Grid.** A 20×20 view box. The drawing stays inside 3–17 (2.5–17.5 for wide shapes), centred
  by eye, not by numbers: a triangle or star sits slightly lower than its box.
- **Stroke.** One line, 1.7 wide, in `currentColor`, with round ends and round joins. No second
  weight, no hard-coded colour; the button or text around it decides the colour.
- **Round everywhere.** No sharp corner anywhere. Rectangles use `box(x, y, w, h, r)`, circles
  `ring(cx, cy, r)`, and anything with points (triangles, stars, the bolt, the pencil) goes through `soft(points, r)`, which rounds each corner. Corner radius: 2.5–3 on
  shapes about 12–14 wide, 2 on shapes about 10 wide, 1–1.5 on small parts.
- **Lines** (chevrons, arrows, crosses, the slash of mute) stay open strokes; the round ends
  and joins already make them soft. Don't close them into outlines.
- **Same thing, same drawing.** Parts that recur are shared constants (`FRAME`, `PENCIL`,
  `SPEAKER`, `TRAY`), so related icons match. Reuse them before drawing a new variant.
- **Filled.** An icon can be drawn solid (`filled`, or listed in `FILLED`). It keeps its stroke,
  so the solid and outlined forms have the same size and outline. Leave room for the stroke
  when drawing a solid icon (the two bars of `pause` are thin for that reason).
- **Few details.** An icon is read at 12–20 px. Two or three parts at most; a dot is
  `M x y h.01` with the round stroke.
- **Size.** 16 in buttons and menus, 18 (the default) in the sidebar and lists, 12–14 inside
  badges and small chips. Don't scale the stroke.
- **Window buttons** (`TitleBar.svelte`) are 10×10 with a 1 px stroke to match the system's own,
  but follow the same rule: round ends, rounded box.
- **Program icons** in the editor come from the system (`appIcons.svelte.ts`) and are shown
  as they are.
- **Naming.** Name an icon for what it shows or the action (`trash`, `copy`, `link`), not the
  place it is used. Before adding one, check whether an existing icon already says it.
