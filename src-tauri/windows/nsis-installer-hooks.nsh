!macro NSIS_HOOK_PREUNINSTALL
  ; Ensure packaged backend processes do not keep install files locked during uninstall.
  StrCpy $0 "$SYSDIR\WindowsPowerShell\v1.0\powershell.exe"
  StrCpy $1 "$$installRoot = ''$INSTDIR''.ToLower()"
  StrCpy $2 "Get-CimInstance Win32_Process -Filter \"Name=''python.exe'' OR Name=''pythonw.exe''\""
  StrCpy $3 "$2 | Where-Object { $$_.ExecutablePath -and $$_.ExecutablePath.ToLower().StartsWith($$installRoot) }"
  StrCpy $4 "$3 | ForEach-Object { Stop-Process -Id $$_.ProcessId -Force -ErrorAction SilentlyContinue }"
  nsExec::ExecToLog '"$0" -NoProfile -ExecutionPolicy Bypass -Command "$1; $4"'
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; Keep behavior aligned with NSIS checkbox: only remove user data when user asked for it.
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    ExpandEnvStrings $0 "%USERPROFILE%"
    RmDir /r "$0\.astrbot"
  ${EndIf}
!macroend
