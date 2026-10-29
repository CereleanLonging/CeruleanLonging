param([Parameter(Mandatory=$true)][string]$ReportDirectory,
      [Parameter(Mandatory=$true)][ValidateSet('5.8.2.0','5.8.3.0')][string]$ExpectedVersion,
      [switch]$NavigateVerificationChat,
      [switch]$InspectChatStructure,
      [switch]$ProductionObserve,
      [switch]$ProductionPrepare)
$ErrorActionPreference='Stop'
if (([int]$NavigateVerificationChat.IsPresent+[int]$InspectChatStructure.IsPresent+[int]$ProductionObserve.IsPresent+[int]$ProductionPrepare.IsPresent) -gt 1) { throw 'Choose one operation.' }
$root=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$report=[IO.Path]::GetFullPath($ReportDirectory)
if (-not $report.StartsWith((Join-Path $root 'reports')+'\',[StringComparison]::OrdinalIgnoreCase)) { throw 'Expected new reports directory.' }
if (Test-Path -LiteralPath $report) { throw 'Refusing to overwrite evidence.' }
$null=New-Item -ItemType Directory -Path $report
$framework=Join-Path $env:WINDIR 'Microsoft.NET/Framework64/v4.0.30319'
$exe=Join-Path $report 'Metadata.exe'
$refs=@('/r:System.Web.Extensions.dll')+@('UIAutomationClient','UIAutomationTypes','WindowsBase'|ForEach-Object {'/r:'+(Join-Path $framework ('WPF/'+$_+'.dll'))})
$productionSources=@('DesktopObserver.cs','DesktopTrial.cs','DesktopStop.cs'|ForEach-Object {Join-Path $root ('crates/core/native/'+$_)})
& (Join-Path $framework 'csc.exe') /nologo /target:exe /platform:x64 /main:WorkBuddyLayoutMetadata /codepage:65001 $refs ('/out:'+$exe) (Join-Path $PSScriptRoot 'WorkBuddyLayoutMetadata.cs') (Join-Path $PSScriptRoot 'WorkBuddyVerificationChat.cs') $productionSources
if ($LASTEXITCODE -ne 0) { throw 'Metadata tool build failed.' }
$source=[IO.File]::ReadAllText((Join-Path $PSScriptRoot 'test-codex-cancel-live.ps1'))
$match=[regex]::Match($source,"(?s)Add-Type -TypeDefinition @'\r?\n(.*?)\r?\n'@")
if (-not $match.Success) { throw 'Owned runner unavailable.' }
if (-not ('CeruleanCancelLive.OwnedRunner' -as [type])) { Add-Type -TypeDefinition $match.Groups[1].Value }
$probeArgs=@($ExpectedVersion)
if ($NavigateVerificationChat) { $probeArgs+='--open-verification-chat' }
if ($InspectChatStructure) { $probeArgs+='--chat-structure' }
if ($ProductionObserve) { $probeArgs+='--production-observe' }
if ($ProductionPrepare) { $probeArgs+='--production-prepare' }
$result=[CeruleanCancelLive.OwnedRunner]::Run($exe,$probeArgs,$root,(Join-Path $report 'stdout.json'),(Join-Path $report 'stderr.log'),25,$false)
[IO.File]::WriteAllText((Join-Path $report 'cleanup.json'),($result|ConvertTo-Json -Depth 10),[Text.UTF8Encoding]::new($false))
if ($result.ExitCode -ne 0 -or $result.Error -or $result.TimedOut -or -not $result.CleanupConfirmed) { throw 'Metadata observation did not complete; preserve evidence.' }
Write-Output 'Requested diagnostic operation completed; inspect its observation for navigation confirmation.'
