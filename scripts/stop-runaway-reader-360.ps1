$ErrorActionPreference='Stop'
$report='D:\test\CeruleanLonging\reports\hyperv-media-360'
$receipt=Join-Path $report 'runaway-reader-stop.json'
if(Test-Path -LiteralPath $receipt){throw 'Stop already recorded; do not repeat.'}
$cancel=Get-Content -LiteralPath (Join-Path $report 'cancel-own-reader.json') -Raw|ConvertFrom-Json
if(-not $cancel.ctrlCSent -or $cancel.terminateProcessUsed){throw 'Expected graceful cancellation evidence missing.'}
$owned=Get-CimInstance Win32_Process -Filter 'ProcessId=21756'
$stopped=$false;$alreadyExited=$false;$beforeBytes=$null
if($owned){
    if($owned.CommandLine -notlike '*-File D:\test\CeruleanLonging\scripts\inspect-win10-media-360.ps1*'){throw 'PID identity differs.'}
    $process=Get-Process -Id 21756
    if($process.StartTime.ToString('yyyy-MM-ddTHH:mm:ss') -ne '2026-10-25T19:05:24'){throw 'PID creation time differs.'}
    if((Get-DiskImage -ImagePath 'D:\iso\zh-cn_windows_10_business_editions_version_22h2_updated_jan_2024_x64_dvd_fef73a09.iso').Attached){throw 'ISO still mounted; do not stop owner.'}
    if((Get-Item -LiteralPath (Join-Path $report 'result.json')).Length -ne 0){throw 'Result is being published; inspect before stopping.'}
    $beforeBytes=$process.PrivateMemorySize64
    $logs=@(Get-ChildItem -LiteralPath $report -Filter 'dism-*.log'|ForEach-Object {[ordered]@{name=$_.Name;bytes=$_.Length;sha256=(Get-FileHash -LiteralPath $_.FullName).Hash.ToLower()}})
    if($logs.Count -ne 6){throw 'Expected completed read-only DISM evidence missing.'}
    Stop-Process -InputObject $process -Force -ErrorAction Stop
    $stopped=$process.WaitForExit(10000)
    if(-not $stopped){throw 'Original process has not exited.'}
}else{$alreadyExited=$true;$logs=@()}
$value=[ordered]@{at=[DateTimeOffset]::Now.ToString('o');targetPid=21756;targetIdentityChecked=[bool]$owned;gracefulCtrlCAlreadyAttempted=$true;readOnlyDismWorkCompleted=$true;isoDismountedBeforeStop=$true;runawaySerializerStopped=$stopped;alreadyExited=$alreadyExited;privateMemoryBeforeBytes=$beforeBytes;originalLogsPreserved=$logs;emptyUnpublishedResultRetained=$true;vmStarted=$false;userApplicationsTerminated=$false;reason='Prevent unbounded memory growth in own read-only metadata serialization; no installer or product operation was running'}
$stream=[IO.File]::Open($receipt,[IO.FileMode]::CreateNew)
try{$bytes=[Text.UTF8Encoding]::new($false).GetBytes(($value|ConvertTo-Json -Depth 5));$stream.Write($bytes,0,$bytes.Length);$stream.Flush($true)}finally{$stream.Dispose()}
