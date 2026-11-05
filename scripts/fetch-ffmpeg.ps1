# Puts ffmpeg.exe and ffprobe.exe in src-tauri/binaries/<name>-<target triple>.exe, for
# CI's media tests. The app itself downloads FFmpeg on first start (src-tauri/src/install.rs);
# keep -Version here the same as VERSION there.
#
#   ./scripts/fetch-ffmpeg.ps1                # triple of the active Rust toolchain
#   ./scripts/fetch-ffmpeg.ps1 -Triple x86_64-pc-windows-msvc
#
# The "essentials" build has everything Replix uses (gfxcapture, ddagrab, NVENC, AMF,
# Quick Sync, x264) at half the size of the full build.
param(
    [string]$Version = "9.0.2",
    [string]$Triple = ""
)
$ErrorActionPreference = "Stop"

if (-not $Triple) {
    $Triple = ((rustc -vV | Select-String "^host:").ToString() -split " ")[1]
}
$root = Split-Path $PSScriptRoot -Parent
$out = Join-Path $root "src-tauri/binaries"
New-Item -ItemType Directory -Force $out | Out-Null

$work = Join-Path ([IO.Path]::GetTempPath()) "replix-ffmpeg-$Version"
New-Item -ItemType Directory -Force $work | Out-Null
$base = "https://github.com/GyanD/codexffmpeg/releases/download/$Version/ffmpeg-$Version-essentials_build"

if (Get-Command 7z -ErrorAction SilentlyContinue) {
    $archive = Join-Path $work "ffmpeg.7z"
    Invoke-WebRequest "$base.7z" -OutFile $archive
    7z e -y "-o$work" $archive "*/bin/ffmpeg.exe" "*/bin/ffprobe.exe" -r | Out-Null
} else {
    $archive = Join-Path $work "ffmpeg.zip"
    Invoke-WebRequest "$base.zip" -OutFile $archive
    Expand-Archive $archive -DestinationPath $work -Force
    Get-ChildItem $work -Recurse -Include ffmpeg.exe, ffprobe.exe | Move-Item -Destination $work -Force
}

foreach ($name in "ffmpeg", "ffprobe") {
    Copy-Item (Join-Path $work "$name.exe") (Join-Path $out "$name-$Triple.exe") -Force
}
Remove-Item $work -Recurse -Force
Write-Host "FFmpeg $Version ready for $Triple in $out"
