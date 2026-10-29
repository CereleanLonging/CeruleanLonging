param([switch]$Create, [string]$SwitchName = '')
$ErrorActionPreference = 'Stop'
$workspace = 'D:\test\CeruleanLonging'
$report = Join-Path $workspace 'reports\hyperv-acceptance-356'
$planPath = Join-Path $report 'vm-resource-plan.json'
if ((Get-FileHash -LiteralPath $planPath -Algorithm SHA256).Hash.ToLower() -ne '340b30743fb4deb00aa67edb561a1a6ba2a491b0d503a8943b90c9fade858544') { throw 'Reviewed resource plan changed; inspect before allocation.' }
$plan = Get-Content -LiteralPath $planPath -Raw -Encoding UTF8 | ConvertFrom-Json
$isoRecord = Get-Content -LiteralPath (Join-Path $report 'iso-identity.json') -Raw -Encoding UTF8 | ConvertFrom-Json
if ($isoRecord.sha256 -ne 'd420152d7cb905426ae6fd6cd47607ac8dc7b29e00f4f929c7555285c98485d8' -or $isoRecord.bytes -ne 6093518848) { throw 'Reviewed ISO record changed.' }
$admin = Get-Content -LiteralPath (Join-Path $report 'host-admin-result.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$intentPath = Join-Path $report 'guest-creation-intent.json'
$resultPath = Join-Path $report 'guest-creation-result.json'
$vmName = 'CeruleanLonging-Win10-358'
$vmRoot = [IO.Path]::GetFullPath([string]$plan.vmRoot)
$expectedRoot = Join-Path $report 'vm'
function Save-NewJson($path, $value) {
    $file = [IO.File]::Open($path, [IO.FileMode]::CreateNew)
    try {
        $bytes = [Text.UTF8Encoding]::new($false).GetBytes(($value | ConvertTo-Json -Depth 10))
        $file.Write($bytes,0,$bytes.Length)
        $file.Flush($true)
    } finally { $file.Dispose() }
}
function Assert-OrdinaryPath($path) {
    $part = [IO.Path]::GetFullPath($path)
    while ($part) {
        if (Test-Path -LiteralPath $part) {
            if ((Get-Item -LiteralPath $part).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Reparse path refused.' }
        }
        $part = [IO.Path]::GetDirectoryName($part)
    }
}
if (-not $vmRoot.Equals($expectedRoot,[StringComparison]::OrdinalIgnoreCase)) { throw 'Unexpected VM allocation root.' }
Assert-OrdinaryPath $vmRoot
Assert-OrdinaryPath ([string]$isoRecord.path)
if ((Test-Path -LiteralPath $intentPath) -or (Test-Path -LiteralPath $resultPath) -or (Test-Path -LiteralPath $vmRoot)) { throw 'Prior guest attempt exists; inspect it instead of replaying.' }
if (-not $admin.success) { throw 'Host feature preparation did not succeed.' }
$os = Get-CimInstance Win32_OperatingSystem
$featureStarted = [DateTimeOffset]::Parse((Get-Content -LiteralPath (Join-Path $report 'host-admin-start.json') -Raw | ConvertFrom-Json).startedAt)
if ($admin.restartNeeded -and $os.LastBootUpTime.ToUniversalTime() -le $featureStarted.UtcDateTime) { throw 'Required host reboot has not occurred; no VM mutation.' }
if (([int64]$os.FreePhysicalMemory * 1024) -lt $plan.minimumHostAvailableBytesBeforeStart) { throw 'Host has less than the reviewed 6 GiB available-memory floor.' }
$principal = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw 'Windows administrator token required; no mutation.' }
Import-Module Hyper-V
if ((Get-Service vmms).Status -ne 'Running') { throw 'Hyper-V VM management service is not running.' }
if (Get-VM -Name $vmName -ErrorAction SilentlyContinue) { throw 'VM name already exists; do not adopt or overwrite it.' }
if ($plan.diskMaximumVirtualBytes -ne 34359738368 -or $plan.additionalBudgetBytes -ne 42000000000 -or $plan.automaticCheckpointsEnabled) { throw 'Resource plan differs from reviewed limits.' }
$iso = Get-Item -LiteralPath $isoRecord.path
if ($iso.Length -ne $isoRecord.bytes -or (Get-FileHash -LiteralPath $iso.FullName -Algorithm SHA256).Hash.ToLower() -ne $isoRecord.sha256) { throw 'User ISO identity changed.' }
$selectedSwitch = $null
if ($SwitchName) {
    $selectedSwitch = Get-VMSwitch -Name $SwitchName
    if (@($selectedSwitch).Count -ne 1) { throw 'Expected exactly one existing network switch.' }
}
$stamp = [DateTimeOffset]::Now.ToString('yyyyMMddTHHmmssfffffff')
$storagePath = Join-Path $report ('storage-before-guest-' + $stamp + '.json')
$python = Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'Programs\Python\Python311\python.exe'
& $python -B -X utf8 (Join-Path $workspace 'scripts\check-test-storage.py') --reserve-bytes $plan.additionalBudgetBytes --external-file $iso.FullName --output $storagePath
if ($LASTEXITCODE -ne 0) { throw 'Storage preflight failed; no VM mutation.' }
$storage = Get-Content -LiteralPath $storagePath -Raw -Encoding UTF8 | ConvertFrom-Json
if (-not $storage.passed) { throw 'Storage evidence did not pass.' }
$review = [ordered]@{at=[DateTimeOffset]::Now.ToString('o');vmName=$vmName;vmRoot=$vmRoot;lastBoot=$os.LastBootUpTime.ToUniversalTime().ToString('o');availableMemoryBytes=([int64]$os.FreePhysicalMemory*1024);resourcePlanSha256=(Get-FileHash -LiteralPath $planPath).Hash.ToLower();storageReport=$storagePath;isoSha256=$isoRecord.sha256;switchId=$(if($selectedSwitch){$selectedSwitch.Id.ToString()}else{$null});startsVm=$false;hostRestart=$false;proxyChange=$false;automaticCheckpoints=$false}
if (-not $Create) {
    Save-NewJson (Join-Path $report ('guest-creation-review-' + $stamp + '.json')) $review
    [pscustomobject]$review | ConvertTo-Json -Depth 6
    return
}
Save-NewJson $intentPath $review
$failure = $null
$createdVm = $null
$vhdPath = Join-Path $vmRoot 'system.vhdx'
try {
    New-Item -ItemType Directory -Path $vmRoot -ErrorAction Stop | Out-Null
    New-VHD -Path $vhdPath -SizeBytes $plan.diskMaximumVirtualBytes -Dynamic | Out-Null
    $createdVm = New-VM -Name $vmName -Generation 2 -MemoryStartupBytes $plan.memoryStartupBytes -VHDPath $vhdPath -Path $vmRoot
    Set-VM -VM $createdVm -AutomaticCheckpointsEnabled $false -CheckpointType Disabled -AutomaticStartAction Nothing -AutomaticStopAction ShutDown
    Set-VMProcessor -VM $createdVm -Count 2
    Set-VMMemory -VM $createdVm -DynamicMemoryEnabled $true -StartupBytes $plan.memoryStartupBytes -MinimumBytes $plan.memoryMinimumBytes -MaximumBytes $plan.memoryMaximumBytes
    if ($selectedSwitch) { Connect-VMNetworkAdapter -VMName $createdVm.Name -SwitchName $selectedSwitch.Name }
    else { Disconnect-VMNetworkAdapter -VMName $createdVm.Name }
    $dvd = Add-VMDvdDrive -VM $createdVm -Path $iso.FullName -Passthru
    Set-VMFirmware -VM $createdVm -EnableSecureBoot On -SecureBootTemplate MicrosoftWindows -FirstBootDevice $dvd
    $observed = Get-VM -Id $createdVm.Id
    $memory = Get-VMMemory -VM $observed
    $disk = Get-VHD -Path $vhdPath
    if ($observed.State -ne 'Off' -or $observed.AutomaticCheckpointsEnabled -or $observed.CheckpointType -ne 'Disabled' -or $disk.Size -ne $plan.diskMaximumVirtualBytes -or $memory.Maximum -ne $plan.memoryMaximumBytes) { throw 'Created VM differs from the reviewed limits; leave stopped for inspection.' }
} catch { $failure = $_.Exception.Message }
finally {
    Save-NewJson $resultPath ([ordered]@{at=[DateTimeOffset]::Now.ToString('o');success=($null -eq $failure);error=$failure;vmId=$(if($createdVm){$createdVm.Id.ToString()}else{$null});vmName=$vmName;root=$vmRoot;vhdPath=$vhdPath;vmStarted=$false;hostRestarted=$false;forceStopUsed=$false;partialStatePreserved=$true;automaticRetry=$false;formalAcceptance=$false})
}
if ($failure) { throw $failure }
Write-Output 'VM created and left Off. Windows is not installed. Review evidence before first boot.'
