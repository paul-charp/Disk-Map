; diskmap NSIS hooks, included by the Tauri installer template.
; The template's "Delete the application data" checkbox only removes the
; identifier folders (WebView2 data). Settings and data live in diskmap folders.

!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    SetShellVarContext current
    RmDir /r "$APPDATA\diskmap"
    RmDir /r "$LOCALAPPDATA\diskmap"
  ${EndIf}
!macroend
