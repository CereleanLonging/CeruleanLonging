$ErrorActionPreference = 'Stop'
$rootPath = 'D:\test\CeruleanLonging\reports\hyperv-acceptance-356'
$startPath = Join-Path $rootPath 'host-admin-start.json'
$resultPath = Join-Path $rootPath 'host-admin-result.json'
if ((Test-Path -LiteralPath $startPath) -or (Test-Path -LiteralPath $resultPath)) { throw 'Original operation exists. Inspect it; do not replay.' }
function Write-NewJson($path, $value) {
    $stream = [IO.File]::Open($path, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::Read)
    try {
        $bytes = [Text.UTF8Encoding]::new($false).GetBytes(($value | ConvertTo-Json -Depth 8))
        $stream.Write($bytes, 0, $bytes.Length)
        $stream.Flush($true)
    } finally { $stream.Dispose() }
}
$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) { throw 'Administrator token required; no change made.' }
Write-NewJson $startPath ([ordered]@{pid=$PID;startedAt=[DateTimeOffset]::Now.ToString('o');scope='Enable existing Windows Hyper-V component only; never reboot, create VM, change trust or create keys';noRestart=$true})
$before = $null
$after = $null
$operation = $null
$failure = $null
try {
    $os = Get-CimInstance Win32_OperatingSystem
    $computer = Get-CimInstance Win32_ComputerSystem
    $before = Get-WindowsOptionalFeature -Online -FeatureName Microsoft-Hyper-V-All
    if ($before.State.ToString() -eq 'Disabled') {
        $operation = Enable-WindowsOptionalFeature -Online -FeatureName Microsoft-Hyper-V-All -All -NoRestart
    } elseif ($before.State.ToString() -notin @('Enabled', 'EnablePending')) {
        throw ('Unexpected feature state: ' + $before.State)
    }
    $after = Get-WindowsOptionalFeature -Online -FeatureName Microsoft-Hyper-V-All
} catch {
    $failure = $_.Exception.Message
} finally {
    $state = if ($after) { $after.State.ToString() } else { $null }
    $restartNeeded = ($state -eq 'EnablePending') -or ($operation -and $operation.RestartNeeded)
    Write-NewJson $resultPath ([ordered]@{
        finishedAt=[DateTimeOffset]::Now.ToString('o');pid=$PID;success=($null -eq $failure);error=$failure;
        beforeState=$(if($before){$before.State.ToString()}else{$null});afterState=$state;
        feature='Microsoft-Hyper-V-All';restartNeeded=[bool]$restartNeeded;restartPerformed=$false;
        os=$(if($os){$os | Select-Object Caption,Version,FreePhysicalMemory,TotalVisibleMemorySize}else{$null});
        computer=$(if($computer){$computer | Select-Object TotalPhysicalMemory,NumberOfLogicalProcessors,HypervisorPresent}else{$null});
        vmCreated=$false;keyCreated=$false;trustChanged=$false;existingProxyUnchanged=$true
    })
}
if ($failure) { exit 1 }
