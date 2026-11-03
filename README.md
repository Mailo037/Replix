# Replix

Keeps the last moments of your games so you never miss a clip. Press <kbd>Alt</kbd> + <kbd>C</kbd> and the last 30 seconds are saved, with every sound (game, Discord, Spotify, your mic) on its own track.

- Replay buffer from 15 seconds to 10 minutes, for games (detected automatically), the whole screen or one app
- Own settings per game or app
- Screenshots and long sessions
- Editor: trim, cut parts out, mute or turn down single tracks, 9:16 for TikTok and Shorts
- Export in full quality, small enough for Discord, or as a GIF
- Copy clips straight into Discord, drag them out, or get a link
- Microphone choice, live level and noise suppression
- Updates itself from GitHub releases

Windows 10 and 11. Built with Tauri (Rust) and Svelte; FFmpeg does the encoding.

## Development

Needs Node 22 and Rust. FFmpeg goes next to the app as a sidecar:

```powershell
./scripts/fetch-ffmpeg.ps1
npm install
npm run tauri dev
```

`rust-toolchain.toml` uses the GNU toolchain so it builds without Visual Studio. CI and releases build with MSVC.

## Releasing

```powershell
npm run set-version 0.2.0
git commit -am "Release 0.2.0"
git tag v0.2.0
git push --follow-tags
```

The release workflow builds the installer, publishes it on GitHub and uploads `latest.json`, which installed copies check for updates. It needs the repository secrets `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.

## FFmpeg

Releases bundle the FFmpeg "essentials" build by Gyan Doshi, which is licensed under the GPL v3. Its source is available at [ffmpeg.org](https://ffmpeg.org/download.html).
