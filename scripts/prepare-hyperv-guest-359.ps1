$ErrorActionPreference = 'Stop'
$root = 'D:\test\CeruleanLonging'
$report = Join-Path $root 'reports\hyperv-guest-359'
$start = Join-Path $report 'admin-start.json'
$finish = Join-Path $report 'admin-result.json'
$create = Join-Path $root 'scripts\create-hyperv-guest-358.ps1'
function Save-NewJson($path,$value) {
    $stream=[IO.File]::Open($path,[IO.FileMode]::CreateNew)
    try { $bytes=[Text.UTF8Encoding]::new($false).GetBytes(($value|ConvertTo-Json -Depth 10));$stream.Write($bytes,0,$bytes.Length);$stream.Flush($true) }
    finally { $stream.Dispose() }
}
if((Test-Path -LiteralPath $start) -or (Test-Path -LiteralPath $finish)){throw 'Existing admin operation; do not replay.'}
if(-not ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'Windows administrator token required.'}
if((Get-FileHash -LiteralPath $create).Hash.ToLower() -ne '0a406848dcfabcd4e819795f5dc0420175600cd6ffddab579c83c5def7ce8508'){throw 'Reviewed creation script changed.'}
Save-NewJson $start ([ordered]@{at=[DateTimeOffset]::Now.ToString('o');pid=$PID;scope='One dedicated Win10 acceptance VM; create only, no startup, restart, custom switch or proxy changes'})
$failure=$null
$switches=@()
$before=@()
$after=@()
try {
    Import-Module Hyper-V
    $requirements=@{
        'New-VM'=@('Name','Generation','MemoryStartupBytes','VHDPath','Path')
        'New-VHD'=@('Path','SizeBytes','Dynamic')
        'Set-VM'=@('VM','AutomaticCheckpointsEnabled','CheckpointType','AutomaticStartAction','AutomaticStopAction')
        'Set-VMProcessor'=@('VM','Count')
        'Set-VMMemory'=@('VM','DynamicMemoryEnabled','StartupBytes','MinimumBytes','MaximumBytes')
        'Add-VMDvdDrive'=@('VM','Path','Passthru')
        'Set-VMFirmware'=@('VM','EnableSecureBoot','SecureBootTemplate','FirstBootDevice')
        'Connect-VMNetworkAdapter'=@('VMName','SwitchName')
        'Disconnect-VMNetworkAdapter'=@('VMName')
    }
    foreach($name in $requirements.Keys){
        $command=Get-Command $name
        foreach($parameter in $requirements[$name]){if(-not $command.Parameters.ContainsKey($parameter)){throw ('Unsupported parameter: '+$name+'/'+$parameter)}}
    }
    $before=@(Get-VM | Select-Object Name,Id,State,Path)
    $switches=@(Get-VMSwitch | Select-Object Name,Id,SwitchType)
    Save-NewJson (Join-Path $report 'hyperv-before.json') ([ordered]@{at=[DateTimeOffset]::Now.ToString('o');existingVMs=$before;existingSwitches=$switches})
    $defaultSwitch=@($switches|Where-Object {$_.Name -eq 'Default Switch'})
    if($defaultSwitch.Count -gt 1){throw 'Ambiguous default switch.'}
    if($defaultSwitch.Count -eq 1){ & $create -Create -SwitchName $defaultSwitch[0].Name }
    else { & $create -Create }
    $created=Get-Content -LiteralPath (Join-Path $root 'reports\hyperv-acceptance-356\guest-creation-result.json') -Raw -Encoding UTF8 | ConvertFrom-Json
    if(-not $created.success){throw 'Guest creation did not complete; inspect original receipt.'}
    $vm=Get-VM -Id $created.vmId
    $after=[ordered]@{
        vm=($vm|Select-Object Name,Id,State,Path,AutomaticStartAction,AutomaticStopAction,AutomaticCheckpointsEnabled,CheckpointType)
        memory=(Get-VMMemory -VM $vm|Select-Object DynamicMemoryEnabled,Startup,Minimum,Maximum)
        processor=(Get-VMProcessor -VM $vm|Select-Object Count)
        disk=(Get-VMHardDiskDrive -VM $vm|Select-Object Path)
        optical=(Get-VMDvdDrive -VM $vm|Select-Object Path)
        network=(Get-VMNetworkAdapter -VMName $vm.Name|Select-Object Name,SwitchName,SwitchId)
        firmware=(Get-VMFirmware -VM $vm|Select-Object SecureBoot,SecureBootTemplate)
    }
} catch { $failure=$_.Exception.Message }
finally {
    Save-NewJson $finish ([ordered]@{at=[DateTimeOffset]::Now.ToString('o');pid=$PID;success=($null -eq $failure);error=$failure;observed=$after;vmStarted=$false;windowsInstalled=$false;existingProxyChanged=$false;customSwitchCreated=$false;hostRestarted=$false;formalAcceptance=$false;automaticRetry=$false})
}
if($failure){exit 1}
