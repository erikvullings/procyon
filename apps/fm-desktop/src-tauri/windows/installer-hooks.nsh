; Tauri's uninstaller offers a "Delete the application data" checkbox, which only
; covers the bundle-identifier directories. When it is ticked (never on updates or
; silent uninstalls, where it defaults to unticked), also remove the `procyon`
; directories holding settings, workspaces, the semantic index, logs and caches.
!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    RMDir /r "$APPDATA\procyon"
    RMDir /r "$LOCALAPPDATA\procyon"
  ${EndIf}
!macroend
