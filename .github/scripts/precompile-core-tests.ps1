$ErrorActionPreference='Stop'
if($env:GITHUB_ACTIONS -ne 'true' -or $env:CERULEAN_CI_CORE_TEST_MEMORY_GIB -ne '4'){
    throw 'Explicit CI core regression allowance required'
}
if(-not $env:CERULEAN_CACHE_SESSION -or $env:CERULEAN_BUILD_SCHEDULE_ACTIVE -ne '1'){
    throw 'Managed build scheduler required'
}
$root=Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
Add-Type -Path (Join-Path $root 'scripts/build-resource-job.cs')
Add-Type -Path (Join-Path $PSScriptRoot 'core-test-job.cs')
$memory=[CeruleanBuildJob]::ReadMemory()
if($memory.AvailablePhysical -lt 4GB -or $memory.AvailablePageFile -lt 4GB){
    throw 'Core regression requires 4 GiB available memory before startup'
}
$cursor=Get-CimInstance Win32_Process -Filter ('ProcessId='+$PID)
$guard=$null
for($index=0;$index -lt 12 -and $cursor.ParentProcessId;$index++){
    $cursor=Get-CimInstance Win32_Process -Filter ('ProcessId='+$cursor.ParentProcessId)
    if($cursor.Name -ieq 'powershell.exe' -and $cursor.CommandLine -match 'build-resource-guard\.ps1'){
        $guard=$cursor
        break
    }
}
if(-not $guard){throw 'Owned scheduler ancestor unavailable'}
$record=[ordered]@{startedAt=[DateTime]::UtcNow.ToString('o');passed=$false;normalMemoryGiB=3;coreCompileMemoryGiB=4;cpuPercent=20;reserveGiB=1;restored=$false}
$changed=$false
try {
    [CeruleanCiCoreJob]::AttachOwnedGuard([uint32]$guard.ProcessId)
    [CeruleanCiCoreJob]::ChangeMemory(3GB,4GB)
    $changed=$true
    cargo test -p cerulean-core --lib --locked --no-run
    $record.code=$LASTEXITCODE
    $record.passed=$LASTEXITCODE -eq 0
    $record.peakJobMemoryBytes=[CeruleanCiCoreJob]::PeakMemoryBytes()
} catch {$record.error=$_.Exception.Message}
finally {
    if($changed){
        try {[CeruleanCiCoreJob]::ChangeMemory(4GB,3GB);$record.restored=$true}
        catch {$record.restoreError=$_.Exception.Message;$record.passed=$false}
    }
    [CeruleanCiCoreJob]::Close()
    $record.finishedAt=[DateTime]::UtcNow.ToString('o')
    $record|ConvertTo-Json -Compress|Write-Host
}
if(-not $record.passed){exit 1}
cargo test -p cerulean-core --lib --locked windows_worker_account::acl_tests::parent_acl_accepts_attributes_and_rejects_listing_writes_or_inheritance -- --exact --test-threads=1
exit $LASTEXITCODE
