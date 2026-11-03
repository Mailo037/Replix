# Developing Replix

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
