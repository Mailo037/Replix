# Developing Replix

Needs Node 22 and Rust, on Windows, Linux or macOS.

```sh
npm install
npm run tauri dev
```

Linux also needs Tauri's libraries and the tools Replix calls, e.g. on Debian and Ubuntu:

```sh
sudo apt install libwebkit2gtk-4.1-dev libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev ffmpeg pulseaudio-utils xclip
```

macOS needs the Xcode command line tools and `brew install ffmpeg`.

What differs between the systems lives in `src-tauri/src/platform/` (windows, processes, monitors, how FFmpeg grabs the picture), `src-tauri/src/audio/` (WASAPI, PulseAudio, AVFoundation), `src-tauri/src/pipe/` and the `CANDIDATES` in `encoder.rs`. [AGENTS.md](../AGENTS.md) has the rules for keeping all three working. `npm run check:all` runs clippy for all three from one machine (the other systems are type-checked only; CI builds and tests on each).

On Windows the installer does not carry FFmpeg. On first start Replix downloads the build pinned in `src-tauri/src/install.rs` (version, size and SHA-256), into `%LOCALAPPDATA%\app.replix\ffmpeg\<version>`. To move to a new FFmpeg, change those constants and `-Version` in `scripts/fetch-ffmpeg.ps1`; installed copies fetch it after their next update and remove the old one. The media tests use whatever `ffmpeg` is on the PATH.

CI and releases build with the default toolchain of each system (MSVC on Windows). Without Visual Studio, Windows builds with the GNU toolchain: `rustup override set stable-x86_64-pc-windows-gnu` in the repository folder.

On Linux and macOS Replix uses the FFmpeg on the system (`install.rs` checks it is 6 or newer and has `x11grab` or `avfoundation`).

## Tests

```sh
npm test                                 # the page's logic (Vitest, src/**/*.test.ts)
npm run test:e2e                         # the page in a browser (Playwright, tests/e2e)
cd src-tauri && cargo test               # recording, saving, the library, FFmpeg
```

The browser tests run the page without Replix's Rust side: `tests/e2e/tauri.ts` answers
its commands from fixtures, records every call so a test can check what was sent, and can
send events. The first time, `npx playwright install chromium` fetches the browser. The
media in `tests/fixtures` is VP9 and Opus, which Playwright's Chromium plays.

## Releasing

```powershell
npm run set-version 0.2.0
git commit -am "Release 0.2.0"
git tag v0.2.0
git push --follow-tags
```

The release workflow builds the Windows installer, the Linux AppImage, .deb and .rpm, and the macOS .dmg for Apple Silicon and Intel, publishes them on GitHub and uploads `latest.json`, which installed copies check for updates (Windows, the AppImage and the macOS app update themselves; a .deb or .rpm is updated by installing the new package). It needs the repository secrets `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.
