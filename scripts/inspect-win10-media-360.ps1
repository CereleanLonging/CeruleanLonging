$ErrorActionPreference='Stop'
$root='D:\test\CeruleanLonging'
$report=Join-Path $root 'reports\hyperv-media-360'
$isoRecord=Get-Content -LiteralPath (Join-Path $root 'reports\hyperv-acceptance-356\iso-identity.json') -Raw -Encoding UTF8|ConvertFrom-Json
function Save-New($path,$value){
    $stream=[IO.File]::Open($path,[IO.FileMode]::CreateNew)
    try{$bytes=[Text.UTF8Encoding]::new($false).GetBytes(($value|ConvertTo-Json -Depth 12));$stream.Write($bytes,0,$bytes.Length);$stream.Flush($true)}finally{$stream.Dispose()}
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
        $images+=($detail|Select-Object ImageIndex,ImageName,ImageDescription,ImageSize,Architecture,Version,EditionId,InstallationType,ProductType,Languages,DefaultLanguage)
    }
    $eiPath=Join-Path $mediaRoot 'sources\EI.CFG';if(Test-Path -LiteralPath $eiPath){$ei=Get-Content -LiteralPath $eiPath -Raw}
    Import-Module Hyper-V
    $vm=Get-VM -Id '1925ebd9-f07c-4432-847d-97b6ba1e69c7'
    $vmInfo=[ordered]@{id=$vm.Id.ToString();name=$vm.Name;state=$vm.State.ToString();disk=@(Get-VMHardDiskDrive -VM $vm|Select-Object Path);checkpointCount=@(Get-VMSnapshot -VM $vm).Count}
    $class=Get-CimClass -Namespace root/virtualization/v2 -ClassName Msvm_Keyboard
    $keyboard=@($class.CimClassMethods|ForEach-Object {[pscustomobject]@{name=$_.Name;parameters=@($_.Parameters|ForEach-Object {[pscustomobject]@{name=$_.Name;type=$_.CimType.ToString()}})}})
}catch{$failure=$_.Exception.Message}
finally{
    if($mountedByThisRun){try{Dismount-DiskImage -ImagePath $isoRecord.path|Out-Null;$dismounted=$true}catch{if(-not $failure){$failure=$_.Exception.Message}}}
    Save-New (Join-Path $report 'result.json') ([ordered]@{at=[DateTimeOffset]::Now.ToString('o');success=($null -eq $failure);error=$failure;isoSha256=$isoRecord.sha256;images=$images;eiConfiguration=$ei;vm=$vmInfo;keyboardMethods=$keyboard;mountedReadOnlyByThisRun=$mountedByThisRun;dismountedByThisRun=$dismounted;hostDiskPartitionsModified=$false;vmStarted=$false;accountCreated=$false;formalAcceptance=$false})
}
if($failure){exit 1}
