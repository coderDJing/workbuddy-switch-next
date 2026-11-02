; Register only this install directory in the current user's PATH. Keep it on
; updater uninstalls; remove only an entry that this installer added.
!macro WB_RUN_PATH_ACTION ACTION
  ExecWait '"$INSTDIR\workbuddy-switch-next.exe" --installer-cli-path "${ACTION}"' $0
  ${If} $0 != 0
    DetailPrint "Could not update the user PATH for WorkBuddy Switch Next (exit $0)."
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro WB_RUN_PATH_ACTION "install"
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode != 1
    !insertmacro CheckIfAppIsRunning "${MAINBINARYNAME}.exe" "${PRODUCTNAME}"
    !insertmacro WB_RUN_PATH_ACTION "uninstall"
  ${EndIf}
!macroend
