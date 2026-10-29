$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$folder=Join-Path $root 'reports/nsis-delete-guard-20261012'
if(Test-Path -LiteralPath $folder){throw 'Preserve earlier guard evidence'}
$null=New-Item -ItemType Directory -Path $folder
$hook=Join-Path $root 'apps/desktop/src-tauri/installer/hooks.nsh'
$source=@'
Unicode true
Name "Cerulean deletion guard fixture"
OutFile "guard.exe"
RequestExecutionLevel user
SilentInstall silent
!include LogicLib.nsh
!include FileFunc.nsh
!macro CheckIfAppIsRunning executablePath productName
!macroend
Var DeleteAppDataCheckboxState
!include "HOOK_PATH"
Section
  StrCpy $DeleteAppDataCheckboxState 1
  !insertmacro NSIS_HOOK_PREUNINSTALL
  FileOpen $0 "$EXEDIR\unexpected-side-effect" w
  FileWrite $0 "should never reach maintenance"
  FileClose $0
SectionEnd
'@.Replace('HOOK_PATH',$hook)
$script=Join-Path $folder 'guard.nsi'
[IO.File]::WriteAllText($script,$source,[Text.UTF8Encoding]::new($false))
$ErrorActionPreference='Continue'
& "$env:LOCALAPPDATA\tauri\NSIS\makensis.exe" /INPUTCHARSET UTF8 $script *> (Join-Path $folder 'compile.log')
$code=$LASTEXITCODE
$ErrorActionPreference='Stop'
if($code){throw "Guard compiler failed: $code"}
$child=Start-Process -FilePath (Join-Path $folder 'guard.exe') -WindowStyle Hidden -PassThru
if(-not $child.WaitForExit(30000)){throw "Guard still running: $($child.Id)"}
$child.Refresh()
if($child.ExitCode -ne 21){throw "Unexpected delete guard exit: $($child.ExitCode)"}
if(Test-Path -LiteralPath (Join-Path $folder 'unexpected-side-effect')){throw 'Deletion guard did not stop maintenance'}
$result=@{status='passed';exit=$child.ExitCode;maintenanceReached=$false;hookSha256=(Get-FileHash -LiteralPath $hook).Hash.ToLowerInvariant();scope='real NSIS pre-uninstall macro with selected deletion state; no interactive UI'}
[IO.File]::WriteAllText((Join-Path $folder 'result.json'),($result|ConvertTo-Json),[Text.UTF8Encoding]::new($false))
$result|ConvertTo-Json -Compress
