$ErrorActionPreference='Stop'
Set-StrictMode -Version Latest
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:GITHUB_EVENT_NAME -ne 'push') { throw 'Push CI candidate required' }
$root=Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
Set-Location -LiteralPath $root
$commit=git rev-parse HEAD
if ($LASTEXITCODE -ne 0 -or $commit -cne $env:GITHUB_SHA) { throw 'CI checkout identity mismatch' }
git diff --exit-code -- apps/desktop/src/contracts.ts
if ($LASTEXITCODE -ne 0) { throw 'Generated contracts changed during packaging' }
$installers=@(Get-ChildItem -LiteralPath (Join-Path $root 'target/release/bundle/nsis') -Filter '*-setup.exe' -File)
if ($installers.Count -ne 1) { throw 'Expected exactly one packaged installer' }
$output=Join-Path $root 'reports/ci-release-candidate'
& (Join-Path $root 'scripts/freeze-windows-package.ps1') -Installer $installers[0].FullName -OutputDirectory $output
$manifestHash=(Get-FileHash -LiteralPath (Join-Path $output 'manifest.json') -Algorithm SHA256).Hash.ToLowerInvariant()
$verification=& (Join-Path $root 'scripts/test-windows-package.ps1') -PackageDirectory $output -ManifestSha256 $manifestHash -AllowUnsignedInternal
$verified=$verification | ConvertFrom-Json
if ($verified.status -ne 'passed' -or $verified.manifestSha256 -cne $manifestHash) { throw 'Cloud package validation failed' }
$verification | Set-Content -LiteralPath (Join-Path $output 'package-verification.json') -Encoding utf8
$schedulingFiles=@(Get-ChildItem -LiteralPath (Join-Path $root 'docs/build-cache/scheduling-logs') -Filter '*.json' -File | Sort-Object LastWriteTimeUtc -Descending)
if (-not $schedulingFiles.Count) { throw 'Packaging resource receipt missing' }
$schedule=ConvertFrom-Json ([IO.File]::ReadAllText($schedulingFiles[0].FullName))
if ($schedule.state -ne 'completed' -or $schedule.exitCode -ne 0 -or $schedule.policy.CpuPercent -ne 20 -or $schedule.policy.MemoryLimitBytes -ne 3GB -or $schedule.policy.ReserveBytes -ne 1GB) { throw 'Packaging resource policy mismatch' }
$evidence=[ordered]@{
    schemaVersion=1
    commit=$commit
    repository=$env:GITHUB_REPOSITORY
    runId=$env:GITHUB_RUN_ID
    runAttempt=$env:GITHUB_RUN_ATTEMPT
    manifestSha256=$manifestHash
    packageValidated=$true
    installerExecuted=$false
    schedule=$schedule
}
$evidence | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $output 'build-evidence.json') -Encoding utf8
