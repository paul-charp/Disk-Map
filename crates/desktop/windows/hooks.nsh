; diskmap NSIS hooks, included by the Tauri installer template.
; InstallDir tells the app it runs installed; any other copy of the exe is portable.
; The Explorer menu entries (HKCU Classes) open folders and drives in diskmap.
; The template's "Delete the application data" checkbox only removes the
; identifier folders (WebView2 data). Settings and data live in diskmap folders.

!macro DISKMAP_EXPLORER_ENTRY KEY
  WriteRegStr HKCU "${KEY}" "MUIVerb" "Open in Diskmap"
  WriteRegStr HKCU "${KEY}" "Icon" '"$INSTDIR\diskmap.exe",0'
  WriteRegStr HKCU "${KEY}\command" "" '"$INSTDIR\diskmap.exe" "%V"'
!macroend

!macro NSIS_HOOK_POSTINSTALL
  WriteRegStr HKCU "Software\diskmap" "InstallDir" "$INSTDIR"
  !insertmacro DISKMAP_EXPLORER_ENTRY "Software\Classes\Directory\shell\diskmap"
  !insertmacro DISKMAP_EXPLORER_ENTRY "Software\Classes\Directory\Background\shell\diskmap"
  !insertmacro DISKMAP_EXPLORER_ENTRY "Software\Classes\Drive\shell\diskmap"
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $UpdateMode <> 1
    DeleteRegKey HKCU "Software\diskmap"
    DeleteRegKey HKCU "Software\Classes\Directory\shell\diskmap"
    DeleteRegKey HKCU "Software\Classes\Directory\Background\shell\diskmap"
    DeleteRegKey HKCU "Software\Classes\Drive\shell\diskmap"
  ${EndIf}
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    SetShellVarContext current
    RmDir /r "$APPDATA\diskmap"
    RmDir /r "$LOCALAPPDATA\diskmap" ; install folder and its data subfolder
  ${EndIf}
!macroend
