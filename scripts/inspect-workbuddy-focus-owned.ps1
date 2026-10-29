param([Parameter(Mandatory=$true)][string]$ReportDirectory,[switch]$ObserveOnly)
$ErrorActionPreference='Stop'
$root=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$report=[IO.Path]::GetFullPath($ReportDirectory)
if (-not $report.StartsWith((Join-Path $root 'reports')+'\',[StringComparison]::OrdinalIgnoreCase)) { throw 'Expected reports directory.' }
if (Test-Path -LiteralPath $report) { throw 'Refusing evidence overwrite.' }
$null=New-Item -ItemType Directory -Path $report
$text=[IO.File]::ReadAllText((Join-Path $PSScriptRoot 'test-codex-cancel-live.ps1'))
$m=[regex]::Match($text,"(?s)Add-Type -TypeDefinition @'\r?\n(.*?)\r?\n'@")
if (-not $m.Success) { throw 'Owned runner unavailable.' }
Add-Type -TypeDefinition $m.Groups[1].Value
$prior=$env:CERULEAN_NATIVE_OWNED_JOB
try {
 $env:CERULEAN_NATIVE_OWNED_JOB='1'
 $probeArgs=@('tests/workbuddy-focus-diagnostic.mjs','5.8.3.0')
 if ($ObserveOnly) { $probeArgs+='--observe-only' }
 $result=[CeruleanCancelLive.OwnedRunner]::Run('D:\qqagent\node\node.exe',$probeArgs,$root,(Join-Path $report 'stdout.log'),(Join-Path $report 'stderr.log'),45,$false)
} finally {$env:CERULEAN_NATIVE_OWNED_JOB=$prior}
[IO.File]::WriteAllText((Join-Path $report 'cleanup.json'),($result|ConvertTo-Json -Depth 10),[Text.UTF8Encoding]::new($false))
if ($result.ExitCode -ne 0 -or $result.Error -or $result.TimedOut -or -not $result.CleanupConfirmed) { throw 'Focus observation incomplete; do not retry a click.' }
Write-Output 'Focus metadata observed. Inspect productionFocusAccepted separately; no message was sent.'
