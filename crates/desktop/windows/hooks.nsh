; diskmap NSIS hooks, included by the Tauri installer template.
; InstallDir tells the app it runs installed; any other copy of the exe is portable.
; The template's "Delete the application data" checkbox only removes the
; identifier folders (WebView2 data). Settings and data live in diskmap folders.

!macro NSIS_HOOK_POSTINSTALL
  WriteRegStr HKCU "Software\diskmap" "InstallDir" "$INSTDIR"
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $UpdateMode <> 1
    DeleteRegKey HKCU "Software\diskmap"
  ${EndIf}
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    SetShellVarContext current
    RmDir /r "$APPDATA\diskmap"
    RmDir /r "$LOCALAPPDATA\diskmap" ; install folder and its data subfolder
  ${EndIf}
!macroend
