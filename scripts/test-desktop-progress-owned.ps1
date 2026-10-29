[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][string]$ReportDirectory,
    [ValidateSet('tests/desktop-progress-native.mjs','tests/desktop-progress-ui.mjs')]
    [string]$TestScript='tests/desktop-progress-native.mjs'
)
$ErrorActionPreference='Stop'
$projectRoot=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$reportsRoot=Join-Path $projectRoot 'reports'
$outputRoot=[IO.Path]::GetFullPath($ReportDirectory)
if (-not $outputRoot.StartsWith($reportsRoot.TrimEnd('\')+'\',[StringComparison]::OrdinalIgnoreCase)) { throw 'Expected a fresh report directory in this repository.' }
if (Test-Path -LiteralPath $outputRoot) { throw 'Use a fresh report directory.' }
$null=New-Item -ItemType Directory -Path $outputRoot
$source=[IO.File]::ReadAllText((Join-Path $PSScriptRoot 'test-codex-cancel-live.ps1'))
$match=[regex]::Match($source,"(?s)Add-Type -TypeDefinition @'\r?\n(.*?)\r?\n'@")
if (-not $match.Success) { throw 'Owned runner source not found.' }
if (-not ('CeruleanCancelLive.OwnedRunner' -as [type])) { Add-Type -TypeDefinition $match.Groups[1].Value }
$node=(Get-Command node.exe -ErrorAction Stop).Source
$saved=$env:CERULEAN_NATIVE_OWNED_JOB
try {
    $env:CERULEAN_NATIVE_OWNED_JOB='1'
    $result=[CeruleanCancelLive.OwnedRunner]::Run($node,@($TestScript),$projectRoot,(Join-Path $outputRoot 'stdout.log'),(Join-Path $outputRoot 'stderr.log'),210,$false)
} finally { $env:CERULEAN_NATIVE_OWNED_JOB=$saved }
[IO.File]::WriteAllText((Join-Path $outputRoot 'cleanup.json'),($result|ConvertTo-Json -Depth 10),[Text.UTF8Encoding]::new($false))
$result|ConvertTo-Json -Depth 10
if ($result.ExitCode -ne 0 -or $result.Error -or $result.TimedOut -or -not $result.CleanupConfirmed) { throw 'Owned progress validation failed; inspect stdout/stderr/cleanup.' }
