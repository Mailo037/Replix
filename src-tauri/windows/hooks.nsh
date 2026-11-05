; Replix up to 0.1.5 kept FFmpeg in the install folder and did not end it on exit; one still
; running there would keep the update from writing its files. Only processes running from
; this install folder are stopped, never another FFmpeg.
!macro NSIS_HOOK_PREINSTALL
  nsExec::ExecToLog `powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command "Get-Process ffmpeg,ffprobe -ErrorAction SilentlyContinue | Where-Object { $$_.Path -like '$INSTDIR\*' } | ForEach-Object { $$_.Kill(); $$_.WaitForExit(5000) | Out-Null }"`
  Pop $0
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  nsExec::ExecToLog `powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command "Get-Process ffmpeg,ffprobe -ErrorAction SilentlyContinue | Where-Object { $$_.Path -like '$INSTDIR\*' } | ForEach-Object { $$_.Kill(); $$_.WaitForExit(5000) | Out-Null }"`
  Pop $0
!macroend
