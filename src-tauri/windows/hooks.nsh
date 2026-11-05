; Replix ties its FFmpeg helpers to its own lifetime, but an FFmpeg left over from an
; older version (or a crash) would keep ffmpeg.exe locked and the update could not write it.
; Only processes running from this install folder are stopped, never another FFmpeg.
!macro NSIS_HOOK_PREINSTALL
  nsExec::ExecToLog `powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command "Get-Process ffmpeg,ffprobe -ErrorAction SilentlyContinue | Where-Object { $$_.Path -like '$INSTDIR\*' } | ForEach-Object { $$_.Kill(); $$_.WaitForExit(5000) | Out-Null }"`
  Pop $0
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  nsExec::ExecToLog `powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command "Get-Process ffmpeg,ffprobe -ErrorAction SilentlyContinue | Where-Object { $$_.Path -like '$INSTDIR\*' } | ForEach-Object { $$_.Kill(); $$_.WaitForExit(5000) | Out-Null }"`
  Pop $0
!macroend
