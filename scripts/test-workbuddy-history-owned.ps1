param([Parameter(Mandatory=$true)][string]$ReportDirectory,
      [Parameter(Mandatory=$true)][ValidateSet('tests/workbuddy-583-coding-history.mjs','tests/workbuddy-583-history.mjs','tests/workbuddy-583-observer.mjs','tests/workbuddy-583-tasks.mjs','tests/workbuddy-stop-native.mjs','tests/workbuddy-send-native.mjs','tests/desktop-workbuddy-composer.mjs','tests/workbuddy-history.mjs','tests/workbuddy-rich-reply.mjs')][string]$TestScript,
      [ValidateSet('5.7.7.0','5.8.3.0')][string]$TestVersion)
$ErrorActionPreference='Stop'
$root=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$report=[IO.Path]::GetFullPath($ReportDirectory)
if (-not $report.StartsWith((Join-Path $root 'reports')+'\',[StringComparison]::OrdinalIgnoreCase)) { throw 'Expected reports directory.' }
if (Test-Path -LiteralPath $report) { throw 'Refusing to overwrite evidence.' }
$null=New-Item -ItemType Directory -Path $report
$source=[IO.File]::ReadAllText((Join-Path $PSScriptRoot 'test-codex-cancel-live.ps1'))
$match=[regex]::Match($source,"(?s)Add-Type -TypeDefinition @'\r?\n(.*?)\r?\n'@")
if (-not $match.Success) { throw 'Owned runner unavailable.' }
Add-Type -TypeDefinition $match.Groups[1].Value
$oldOwned=$env:CERULEAN_NATIVE_OWNED_JOB
$oldLive=$env:CERULEAN_WORKBUDDY_HISTORY_LIVE
try {
    $env:CERULEAN_NATIVE_OWNED_JOB='1'
    $env:CERULEAN_WORKBUDDY_HISTORY_LIVE=$null
    $testArgs=@($TestScript)
    if ($TestVersion) { $testArgs+=$TestVersion }
    $result=[CeruleanCancelLive.OwnedRunner]::Run('D:\qqagent\node\node.exe',$testArgs,$root,(Join-Path $report 'stdout.log'),(Join-Path $report 'stderr.log'),180,$false)
} finally {
    $env:CERULEAN_NATIVE_OWNED_JOB=$oldOwned
    $env:CERULEAN_WORKBUDDY_HISTORY_LIVE=$oldLive
}
[IO.File]::WriteAllText((Join-Path $report 'cleanup.json'),($result|ConvertTo-Json -Depth 10),[Text.UTF8Encoding]::new($false))
if ($result.ExitCode -ne 0 -or $result.Error -or $result.TimedOut -or -not $result.CleanupConfirmed) { throw 'History validation did not pass; inspect preserved evidence.' }
Write-Output 'Controlled history validation completed.'
