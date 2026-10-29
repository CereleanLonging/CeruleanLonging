[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][string]$ReportDirectory,
    [ValidateSet('tests/worker-supervision-ui.mjs','tests/worker-supervision-approval-images.mjs','tests/currency-product-ui.mjs','tests/currency-accounts-ui.mjs','tests/planning-budget-ui.mjs','tests/historical-product-ui.mjs','tests/historical-paging-ui.mjs','tests/historical-result-ui.mjs','tests/workspace-receipt-ui.mjs','tests/workspace-app-ui.mjs','tests/workspace-tasks-ui.mjs','tests/execution-review-ui.mjs')]
    [string]$TestScript='tests/worker-supervision-ui.mjs'
)
$ErrorActionPreference='Stop'
$projectRoot=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$reportsRoot=Join-Path $projectRoot 'reports'
$outputRoot=[IO.Path]::GetFullPath($ReportDirectory)
if (-not $outputRoot.StartsWith($reportsRoot.TrimEnd('\')+'\',[StringComparison]::OrdinalIgnoreCase)) { throw 'Expected a fresh report directory in this repository.' }
if (Test-Path -LiteralPath $outputRoot) { throw 'Use a fresh report directory.' }
$null=New-Item -ItemType Directory -Path $outputRoot
$pathToInspect=Get-Item -LiteralPath $outputRoot
while ($null -ne $pathToInspect) {
    if (($pathToInspect.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw 'Reparse paths are not supported.' }
    $pathToInspect=$pathToInspect.Parent
}
# Extract only the previously self-tested ownership class; do not execute live Codex statements.
$source=[IO.File]::ReadAllText((Join-Path $projectRoot 'scripts/test-codex-cancel-live.ps1'))
$match=[regex]::Match($source,"(?s)Add-Type -TypeDefinition @'\r?\n(.*?)\r?\n'@")
if (-not $match.Success) { throw 'Owned runner source not found.' }
if (-not ('CeruleanCancelLive.OwnedRunner' -as [type])) { Add-Type -TypeDefinition $match.Groups[1].Value }
$node=(Get-Command node.exe -ErrorAction Stop).Source
$saved=$env:CERULEAN_NATIVE_OWNED_JOB
try {
    $env:CERULEAN_NATIVE_OWNED_JOB='1'
    $result=[CeruleanCancelLive.OwnedRunner]::Run($node,@($TestScript),$projectRoot,(Join-Path $outputRoot 'stdout.log'),(Join-Path $outputRoot 'stderr.log'),150,$false)
} finally { $env:CERULEAN_NATIVE_OWNED_JOB=$saved }
[IO.File]::WriteAllText((Join-Path $outputRoot 'cleanup.json'),($result|ConvertTo-Json -Depth 10),[Text.UTF8Encoding]::new($false))
$result|ConvertTo-Json -Depth 10
if ($result.ExitCode -ne 0 -or $result.Error -or $result.TimedOut -or -not $result.CleanupConfirmed) { throw 'Owned UI validation failed; inspect stdout/stderr/cleanup.' }
