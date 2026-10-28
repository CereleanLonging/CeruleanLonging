; Product policy: maintenance never kills the UI, core or user applications.
; Tauri 2.12.1 includes this file after utils.nsh and before invoking its macros.
; Fail compilation if the upstream integration point changes.
!ifmacrodef CheckIfAppIsRunning
  !macroundef CheckIfAppIsRunning
!else
  !error "Re-review the Tauri process guard integration before packaging"
!endif

Var CeruleanDataGuard
Var CeruleanGuardOverlap
Var CeruleanGuardResult
Var CeruleanGuardError

!macro CeruleanReleaseGuard
  ${If} $CeruleanDataGuard != ""
  ${AndIf} $CeruleanDataGuard != -1
  ${AndIf} $CeruleanDataGuard != 0
    ; Close the original handle to release its exclusive byte-range lock.
    ; Stack returns are consumed immediately; caller registers remain unchanged.
    System::Call 'kernel32::CloseHandle(p $CeruleanDataGuard) i .s'
    Pop $CeruleanGuardResult
    ${If} $CeruleanGuardResult == 0
      SetErrorLevel 20
      Abort "CeruleanLonging could not release its maintenance guard. Preserve this operation for review. No process was terminated."
    ${EndIf}
  ${EndIf}
  StrCpy $CeruleanDataGuard ""
  ${If} $CeruleanGuardOverlap != ""
  ${AndIf} $CeruleanGuardOverlap != 0
    System::Free $CeruleanGuardOverlap
  ${EndIf}
  StrCpy $CeruleanGuardOverlap ""
!macroend

!macro CeruleanRefuseBusy
  !insertmacro CeruleanReleaseGuard
  SetErrorLevel 20
  Abort "CeruleanLonging is running or its files are unavailable. Stop the core through the workbench, close its windows, then retry. No process was terminated."
!macroend

!macro CeruleanCheckBinary path
  Push $0
  ${If} ${FileExists} "${path}"
    ; GENERIC_WRITE is refused for an executable with a live image mapping.
    ; OPEN_EXISTING only: no file is created, truncated or written.
    System::Call 'kernel32::CreateFileW(w "${path}", i 0x40000000, i 7, p 0, i 3, i 0x00200000, p 0) p .r0'
    ${If} $0 = -1
      Pop $0
      !insertmacro CeruleanRefuseBusy
    ${Else}
      System::Call 'kernel32::CloseHandle(p r0)'
    ${EndIf}
  ${EndIf}
  Pop $0
!macroend

!macro CheckIfAppIsRunning executablePath productName
  !insertmacro CeruleanCheckBinary "${executablePath}"
!macroend

!macro CeruleanAcquireGuard
  !insertmacro CeruleanCheckBinary "$INSTDIR\CeruleanLonging.exe"
  !insertmacro CeruleanCheckBinary "$INSTDIR\cerulean-runtime.exe"
  ; No recursive acquisition: never lose an already-owned handle or allocation.
  ${If} $CeruleanDataGuard != ""
  ${OrIf} $CeruleanGuardOverlap != ""
    !insertmacro CeruleanRefuseBusy
  ${EndIf}
  ; Skip the guard only when the entire data workspace is genuinely absent.
  ; Never create a data directory or lock as an installer side effect.
  ${If} ${FileExists} "$APPDATA\local.ceruleanlonging.workbench\workspace\writer.lock"
    ; RW/share3 permits a parent's continuous read/share3 name pin. DELETE is
    ; deliberately not shared. Mutual exclusion uses the same range as fs2 0.4.3.
    System::Call 'kernel32::CreateFileW(w "$APPDATA\local.ceruleanlonging.workbench\workspace\writer.lock", i 0xC0000000, i 3, p 0, i 3, i 0x00200000, p 0) p .s'
    Pop $CeruleanDataGuard
    ${If} $CeruleanDataGuard == -1
    ${OrIf} $CeruleanDataGuard == 0
      !insertmacro CeruleanRefuseBusy
    ${EndIf}
    ; Pointer-width fields follow the actual NSIS/System-plugin architecture:
    ; zeroed Internal, InternalHigh, Offset, OffsetHigh and hEvent.
    ; CreateFile is synchronous; no outstanding OVERLAPPED I/O survives the call.
    System::Call '*(p 0, p 0, i 0, i 0, p 0) p .s'
    Pop $CeruleanGuardOverlap
    ${If} $CeruleanGuardOverlap == 0
      !insertmacro CeruleanRefuseBusy
    ${EndIf}
    System::Call 'kernel32::LockFileEx(p $CeruleanDataGuard, i 3, i 0, i -1, i -1, p $CeruleanGuardOverlap) i .s'
    Pop $CeruleanGuardResult
    ${If} $CeruleanGuardResult == 0
      !insertmacro CeruleanRefuseBusy
    ${EndIf}
  ${Else}
    ; Existing workspace with a missing/unavailable writer is not clean install.
    ; Preserve caller $0 while capturing GetLastError directly from System.
    Push $0
    System::Call 'kernel32::GetFileAttributesW(w "$APPDATA\local.ceruleanlonging.workbench\workspace") i .r0 ?e'
    Pop $CeruleanGuardError
    StrCpy $CeruleanGuardResult $0
    Pop $0
    ${If} $CeruleanGuardResult != -1
      !insertmacro CeruleanRefuseBusy
    ${ElseIf} $CeruleanGuardError != 2
    ${AndIf} $CeruleanGuardError != 3
      ; Only ERROR_FILE_NOT_FOUND/PATH_NOT_FOUND can establish absence.
      !insertmacro CeruleanRefuseBusy
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro CeruleanAcquireGuard
!macroend
!macro NSIS_HOOK_POSTINSTALL
  !insertmacro CeruleanReleaseGuard
!macroend
!macro NSIS_HOOK_PREUNINSTALL
  ; The upstream checkbox would recursively remove authoritative data. Keep this
  ; unavailable until the separate backup/reviewed cleanup flow is implemented.
  ${If} $DeleteAppDataCheckboxState = 1
    SetErrorLevel 21
    Abort "This version preserves workbench data. Uncheck Delete application data to uninstall. Data cleanup requires a separate reviewed backup workflow."
  ${EndIf}
  !insertmacro CeruleanAcquireGuard
!macroend
!macro NSIS_HOOK_POSTUNINSTALL
  !insertmacro CeruleanReleaseGuard
!macroend
