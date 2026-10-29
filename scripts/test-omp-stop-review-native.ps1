[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][string]$ReportDirectory,
    [ValidateSet('tests/omp-stop-review-native.mjs','tests/data-diagnostic-native.mjs','tests/startup-closure-native.mjs','tests/omp-controls-native.mjs','tests/management-diagnostic-native.mjs','tests/management-review-native.mjs','tests/provider-discovery-native.mjs','tests/provider-credentials-native.mjs','tests/api-approval-render.mjs','tests/api-project-form.mjs','tests/worktree-project-form.mjs','tests/api-controls-ui.mjs','tests/api-publication-native.mjs','tests/api-dag-native.mjs','tests/desktop-binding-native.mjs','tests/desktop-trial-native.mjs','tests/desktop-trial-read-native.mjs','tests/desktop-plan-native.mjs','tests/desktop-task-native.mjs','tests/desktop-managed-native.mjs','tests/desktop-recovery-native.mjs','tests/desktop-stop-native.mjs','tests/desktop-cancellation-native.mjs','tests/desktop-stop-publication.mjs','tests/desktop-diagnostic-native.mjs','tests/desktop-stop-review-native.mjs','tests/desktop-stop-review-crash.mjs','tests/desktop-joint-native.mjs','tests/desktop-live-observation.mjs','tests/desktop-composer-diagnostic.mjs','tests/desktop-workbuddy-composer.mjs','tests/desktop-joint-ui.mjs','tests/desktop-capability-diagnostic.mjs','tests/workbuddy-navigate-test.mjs','tests/workbuddy-history.mjs','tests/workbuddy-reply.mjs','tests/workbuddy-stop-native.mjs','tests/workbuddy-send-native.mjs','tests/workbuddy-send-live.mjs','tests/workbuddy-send-reconcile.mjs','tests/workbuddy-focus-diagnostic.mjs','tests/workbuddy-draft-diagnostic.mjs','tests/workbuddy-draft-submit.mjs','tests/workbuddy-draft-native.mjs','tests/workbuddy-restart-review.mjs','tests/workbuddy-restart-send.mjs','tests/workbuddy-rich-reply.mjs','tests/workbuddy-coding-live.mjs','tests/workbuddy-coding-readback.mjs')]
    [string]$TestScript='tests/omp-stop-review-native.mjs'
)
$ErrorActionPreference='Stop'
$projectRoot=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$reportsRoot=Join-Path $projectRoot 'reports'
$outputRoot=[IO.Path]::GetFullPath($ReportDirectory)
if (-not $outputRoot.StartsWith($reportsRoot.TrimEnd('\')+'\',[StringComparison]::OrdinalIgnoreCase)) { throw 'Expected a fresh report directory in this repository.' }
if (Test-Path -LiteralPath $outputRoot) { throw 'Use a fresh report directory.' }
$null=New-Item -ItemType Directory -Path $outputRoot
# Reuse the established, self-tested ownership implementation only. Do not run
# the live Codex test or any top-level statements from that script.
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
if ($result.ExitCode -ne 0 -or $result.Error -or $result.TimedOut -or -not $result.CleanupConfirmed) { throw 'Owned native validation failed; inspect stdout/stderr/cleanup.' }
