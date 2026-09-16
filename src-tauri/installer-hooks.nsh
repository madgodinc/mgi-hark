; The app adds itself to the per-user Run key (autostart.rs). Take that entry
; away with the app, so Windows does not try to start a missing file.
!macro NSIS_HOOK_POSTUNINSTALL
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Hark"
!macroend
