# Developing Replix

Needs Node 22 and Rust.

```powershell
npm install
npm run tauri dev
```

The installer does not carry FFmpeg. On first start Replix downloads the build pinned in `src-tauri/src/install.rs` (version, size and SHA-256), into `%LOCALAPPDATA%\app.replix\ffmpeg\<version>`. To move to a new FFmpeg, change those constants and `-Version` in `scripts/fetch-ffmpeg.ps1`; installed copies fetch it after their next update and remove the old one. The media tests use whatever `ffmpeg` is on the PATH.

`rust-toolchain.toml` uses the GNU toolchain so it builds without Visual Studio. CI and releases build with MSVC.

## Releasing

```powershell
npm run set-version 0.2.0
git commit -am "Release 0.2.0"
git tag v0.2.0
git push --follow-tags
```

The release workflow builds the installer, publishes it on GitHub and uploads `latest.json`, which installed copies check for updates. It needs the repository secrets `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.
