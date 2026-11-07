$ErrorActionPreference='Stop'
Set-StrictMode -Version Latest
if ($env:CERULEAN_RUN_NATIVE_ACCOUNT_TESTS -ne '1' -or -not $env:CERULEAN_CACHE_SESSION -or $env:CERULEAN_BUILD_SCHEDULE_ACTIVE -ne '1') {
    throw 'Explicit native suite and managed scheduler required'
}
$identity=[Security.Principal.WindowsIdentity]::GetCurrent()
$principal=[Security.Principal.WindowsPrincipal]::new($identity)
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw 'Administrator required for native provisioning preflight' }
$repository=Split-Path -Parent $PSScriptRoot
$fixture=Join-Path $repository ('reports\native-recheck-preflight-'+[Guid]::NewGuid().ToString('N'))
$data=Join-Path $fixture 'data'
$shared=Join-Path $fixture 'project'
$chief=Join-Path $fixture 'chief'
$workerIdentity=[Guid]::NewGuid().ToString('N')+[Guid]::NewGuid().ToString('N')
$workspace=Join-Path $shared ('executors\'+$workerIdentity)
foreach ($directory in @($data,$chief,$workspace)) { $null=[IO.Directory]::CreateDirectory($directory) }
$planFile=Join-Path $fixture 'native-account-plan.json'
$plan=[ordered]@{
    schemaVersion=1;ownerSid=$identity.User.Value;projectId='project'
    dataRoot=$data;sharedRoot=$shared;chiefRoot=$chief
    slots=@(@{identity=$workerIdentity;workspace=$workspace;username=('CLW'+$workerIdentity.Substring(0,16))})
}
[IO.File]::WriteAllText($planFile,($plan|ConvertTo-Json -Depth 5),[Text.UTF8Encoding]::new($false))
$cleanupComplete=$false
try {
    & powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File (Join-Path $repository 'scripts/provision-worker-accounts.ps1') -PlanPath $planFile
    $provisionCode=$LASTEXITCODE
    $receipt=ConvertFrom-Json ([IO.File]::ReadAllText([IO.Path]::ChangeExtension($planFile,'result.json')))
    if ($provisionCode -ne 0 -or -not $receipt.success) { throw ('Native provisioning preflight failed: '+$receipt.error) }
    Write-Host 'Native provisioning preflight passed; temporary account cleanup follows.'
} finally {
    & powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File (Join-Path $repository 'crates/core/tests/strict_workflow/recheck-account-cleanup.ps1') -PlanPath $planFile
    if ($LASTEXITCODE -eq 0) { $cleanupComplete=$true }
    if ($cleanupComplete) {
        $resolved=[IO.Path]::GetFullPath($fixture)
        $parent=[IO.Path]::GetFullPath((Join-Path $repository 'reports'))+'\'
        if (-not $resolved.StartsWith($parent,[StringComparison]::OrdinalIgnoreCase) -or (Split-Path -Leaf $resolved) -notmatch '^native-recheck-preflight-[a-f0-9]{32}$') { throw 'Fixture cleanup escaped owned root' }
        Remove-Item -LiteralPath $resolved -Recurse -Force
    } else { throw ('Native preflight cleanup requires reconciliation; retained '+$fixture) }
}
