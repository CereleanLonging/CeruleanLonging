$ErrorActionPreference='Stop'
trap {
    $failurePath='D:\test\CeruleanLonging\reports\hyperv-media-360\primitive-final\fatal-error.txt'
    if(-not (Test-Path -LiteralPath $failurePath)) {
        $message=$_.Exception.GetType().FullName+': '+$_.Exception.Message
        if($message.Length -gt 4096){$message=$message.Substring(0,4096)}
        [IO.File]::WriteAllText($failurePath,$message,[Text.UTF8Encoding]::new($false))
    }
    break
}
$root='D:\test\CeruleanLonging'
$report=Join-Path $root 'reports\hyperv-media-360\primitive-final'
$isoRecord=Get-Content -LiteralPath (Join-Path $root 'reports\hyperv-acceptance-356\iso-identity.json') -Raw -Encoding UTF8|ConvertFrom-Json
function Save-New($path,$value){
    # Only primitive metadata is supplied. Serialize before opening the receipt.
    $json=$value|ConvertTo-Json -Depth 6
    $bytes=[Text.UTF8Encoding]::new($false).GetBytes($json)
    if($bytes.Length -gt 262144){throw 'Metadata receipt exceeds the 256KiB bound.'}
    $stream=[IO.File]::Open($path,[IO.FileMode]::CreateNew)
    try{$stream.Write($bytes,0,$bytes.Length);$stream.Flush($true)}finally{$stream.Dispose()}
}
if(-not ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'Windows administrator token required.'}
if(Test-Path -LiteralPath (Join-Path $report 'start.json')){throw 'Read-only inspection already started; inspect receipt instead of replay.'}
Save-New (Join-Path $report 'start.json') ([ordered]@{at=[DateTimeOffset]::Now.ToString('o');pid=$PID;iso=$isoRecord.path;readOnlyMedia=$true;vmStart=$false})
$failure=$null;$mountedByThisRun=$false;$dismounted=$false;$images=@();$ei=$null;$keyboard=$null;$vmInfo=$null
try{
    if((Get-FileHash -LiteralPath $isoRecord.path -Algorithm SHA256).Hash.ToLower() -ne $isoRecord.sha256){throw 'ISO digest changed.'}
    $image=Get-DiskImage -ImagePath $isoRecord.path
    if(-not $image.Attached){$image=Mount-DiskImage -ImagePath $isoRecord.path -Access ReadOnly -PassThru;$mountedByThisRun=$true}
    $volume=@($image|Get-Volume|Where-Object {$_.DriveLetter})
    if($volume.Count -ne 1){throw 'ISO did not resolve to exactly one drive.'}
    $mediaRoot=([string]$volume[0].DriveLetter)+':\'
    $wim=Join-Path $mediaRoot 'sources\install.wim'
    $scratch=Join-Path $report 'scratch';New-Item -ItemType Directory -Path $scratch -ErrorAction Stop|Out-Null
    $list=Get-WindowsImage -ImagePath $wim -LogPath (Join-Path $report 'dism-list.log') -ScratchDirectory $scratch
    foreach($entry in $list){
        $detail=Get-WindowsImage -ImagePath $wim -Index $entry.ImageIndex -LogPath (Join-Path $report ('dism-index-'+$entry.ImageIndex+'.log')) -ScratchDirectory $scratch
        $images+=[ordered]@{
            imageIndex=[int]$detail.ImageIndex;imageName=[string]$detail.ImageName
            imageDescription=[string]$detail.ImageDescription;imageSize=[int64]$detail.ImageSize
            architecture=[string]$detail.Architecture;version=[string]$detail.Version
            editionId=[string]$detail.EditionId;installationType=[string]$detail.InstallationType
            productType=[string]$detail.ProductType
            languages=@($detail.Languages|ForEach-Object {if($_ -is [Globalization.CultureInfo]){[string]$_.Name}else{[string]$_}})
            defaultLanguage=$(if($detail.DefaultLanguage -is [Globalization.CultureInfo]){[string]$detail.DefaultLanguage.Name}else{[string]$detail.DefaultLanguage})
        }
        Save-New (Join-Path $report ('image-'+$entry.ImageIndex+'.json')) $images[-1]
    }
    $eiPath=Join-Path $mediaRoot 'sources\EI.CFG';if(Test-Path -LiteralPath $eiPath){$ei=[IO.File]::ReadAllText($eiPath)}
    Import-Module Hyper-V
    $vm=Get-VM -Id '1925ebd9-f07c-4432-847d-97b6ba1e69c7'
    $disks=@(Get-VMHardDiskDrive -VM $vm)
    if($disks.Count -ne 1){throw 'Reviewed VM must have exactly one virtual hard disk.'}
    $vhd=Get-VHD -Path $disks[0].Path
    $vmInfo=[ordered]@{id=[string]$vm.Id;name=[string]$vm.Name;state=[string]$vm.State;diskPath=[string]$disks[0].Path;virtualDiskBytes=[int64]$vhd.Size;allocatedDiskBytes=[int64]$vhd.FileSize;logicalSectorBytes=[int]$vhd.LogicalSectorSize;physicalSectorBytes=[int]$vhd.PhysicalSectorSize;checkpointCount=@(Get-VMSnapshot -VM $vm).Count}
    $class=Get-CimClass -Namespace root/virtualization/v2 -ClassName Msvm_Keyboard
    $keyboard=@($class.CimClassMethods|ForEach-Object {[pscustomobject]@{name=$_.Name;parameters=@($_.Parameters|ForEach-Object {[pscustomobject]@{name=$_.Name;type=$_.CimType.ToString()}})}})
}catch{$failure=$_.Exception.Message}
finally{
    if($mountedByThisRun){try{Dismount-DiskImage -ImagePath $isoRecord.path|Out-Null;$dismounted=$true}catch{if(-not $failure){$failure=$_.Exception.Message}}}
    Save-New (Join-Path $report 'result.json') ([ordered]@{at=[DateTimeOffset]::Now.ToString('o');success=($null -eq $failure);error=$failure;isoSha256=$isoRecord.sha256;images=$images;eiConfiguration=$ei;vm=$vmInfo;keyboardMethods=$keyboard;mountedReadOnlyByThisRun=$mountedByThisRun;dismountedByThisRun=$dismounted;hostDiskPartitionsModified=$false;vmStarted=$false;accountCreated=$false;formalAcceptance=$false})
}
if($failure){exit 1}
