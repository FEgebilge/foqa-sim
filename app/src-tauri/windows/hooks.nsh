; FOQA-Sim installer hooks (Tauri NSIS). The app, not the installer, installs
; and repairs the MSFS Community packages; the installer only checks
; prerequisites and, on uninstall, removes the packages the app recorded.
; Neither runs for an update ($UpdateMode, set by the template from /UPDATE).

!macro NSIS_HOOK_PREINSTALL
  ${If} $UpdateMode <> 1
    ; VC++ 2015-2022 x64 runtime (the statically linked SimConnect is /MD).
    SetRegView 64
    ReadRegDWORD $0 HKLM "SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes\x64" "Installed"
    ${If} $0 <> 1
      MessageBox MB_YESNOCANCEL|MB_ICONEXCLAMATION "FOQA-Sim needs the Microsoft Visual C++ Redistributable (x64), which was not found.$\r$\n$\r$\nYes: open the download page and cancel this installation (install it, then run this setup again).$\r$\nNo: install FOQA-Sim anyway (it will not start without the runtime).$\r$\nCancel: cancel this installation." /SD IDNO IDYES vcDownload IDNO vcDone
      Abort
      vcDownload:
        ExecShell "open" "https://aka.ms/vs/17/release/vc_redist.x64.exe"
        Abort
      vcDone:
    ${EndIf}

    ; MSFS 2024: Microsoft Store or Steam user config.
    ${IfNot} ${FileExists} "$LOCALAPPDATA\Packages\Microsoft.Limitless_8wekyb3d8bbwe\LocalCache\UserCfg.opt"
    ${AndIfNot} ${FileExists} "$APPDATA\Microsoft Flight Simulator 2024\UserCfg.opt"
      MessageBox MB_YESNO|MB_ICONQUESTION "Microsoft Flight Simulator 2024 was not found on this PC. FOQA-Sim records flights from it.$\r$\n$\r$\nInstall anyway?" /SD IDYES IDYES +2
      Abort
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
!macroend

; The list the app keeps of the package folders it installed; copied aside
; first, as "Delete app data" removes it before POSTUNINSTALL.
!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
    InitPluginsDir
    CopyFiles /SILENT "$APPDATA\com.egebilge.foqa-sim\community-packages.txt" "$PLUGINSDIR"
  ${EndIf}
!macroend

; After a completed uninstall, removes those folders. The list (UTF-16LE,
; written by the app) has one absolute folder per line; one is deleted only if
; it is named foqa-sim-panel / foqa-sim-efb and its manifest.json names
; FOQA-Sim as the manufacturer, the app's own rule (setup.rs `ours`).
!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $UpdateMode <> 1
    ClearErrors
    FileOpen $R8 "$PLUGINSDIR\community-packages.txt" r
    ${IfNot} ${Errors}
      foqaReadLine:
        FileReadUTF16LE $R8 $R0
        ${If} ${Errors}
          Goto foqaDone
        ${EndIf}
        ; trim trailing CR/LF/space/backslash
        foqaTrim:
          StrCpy $R1 $R0 1 -1
          ${If} $R1 == "$\r"
          ${OrIf} $R1 == "$\n"
          ${OrIf} $R1 == " "
          ${OrIf} $R1 == "\"
            StrCpy $R0 $R0 -1
            Goto foqaTrim
          ${EndIf}
        StrCpy $R2 $R0 1 1
        StrCpy $R3 $R0 "" -15
        StrCpy $R4 $R0 "" -13
        ${If} $R3 == "\foqa-sim-panel"
        ${OrIf} $R4 == "\foqa-sim-efb"
        ${AndIf} $R2 == ":"
          ClearErrors
          FileOpen $R6 "$R0\manifest.json" r
          ${IfNot} ${Errors}
            foqaManifestLine:
              FileRead $R6 $R5
              ${IfNot} ${Errors}
                ${WordFind} $R5 '"manufacturer"' "E+1}" $R5
                ${IfNot} ${Errors}
                  ${WordFind} $R5 '"FOQA-Sim"' "E+1{" $R5
                  ${IfNot} ${Errors}
                    FileClose $R6
                    RMDir /r "$R0"
                    Goto foqaReadLine
                  ${EndIf}
                ${EndIf}
                ClearErrors
                Goto foqaManifestLine
              ${EndIf}
            FileClose $R6
          ${EndIf}
        ${EndIf}
        ClearErrors
        Goto foqaReadLine
      foqaDone:
      FileClose $R8
    ${EndIf}
    ClearErrors
  ${EndIf}
!macroend
