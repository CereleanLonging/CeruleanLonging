$ErrorActionPreference='Stop'
$report='D:\test\CeruleanLonging\reports\hyperv-media-360'
$receipt=Join-Path $report 'cancel-own-reader.json'
if(Test-Path -LiteralPath $receipt){throw 'Cancellation already attempted; inspect evidence.'}
$owned=Get-CimInstance Win32_Process -Filter 'ProcessId=21756'
if(-not $owned){throw 'Original reader is already absent.'}
if($owned.CommandLine -notlike '*-File D:\test\CeruleanLonging\scripts\inspect-win10-media-360.ps1*'){throw 'PID identity differs; no signal.'}
$image=Get-DiskImage -ImagePath 'D:\iso\zh-cn_windows_10_business_editions_version_22h2_updated_jan_2024_x64_dvd_fef73a09.iso'
if($image.Attached){throw 'ISO still mounted; preserve original owner.'}
$process=Get-Process -Id 21756
if($process.StartTime.ToString('yyyy-MM-ddTHH:mm:ss') -ne '2026-10-25T19:05:24'){throw 'PID creation time differs.'}
Add-Type -TypeDefinition @'
using System.Runtime.InteropServices;
public static class OwnedConsole360 {
 [DllImport("kernel32.dll", SetLastError=true)] public static extern bool FreeConsole();
 [DllImport("kernel32.dll", SetLastError=true)] public static extern bool AttachConsole(uint pid);
 [DllImport("kernel32.dll", SetLastError=true)] public static extern bool SetConsoleCtrlHandler(System.IntPtr handler, bool add);
 [DllImport("kernel32.dll", SetLastError=true)] public static extern bool GenerateConsoleCtrlEvent(uint signal, uint group);
}
'@
$attached=$false;$sent=$false;$failure=$null
try{
    [OwnedConsole360]::FreeConsole()|Out-Null
    $attached=[OwnedConsole360]::AttachConsole(21756)
    if(-not $attached){throw ('AttachConsole failed: '+[Runtime.InteropServices.Marshal]::GetLastWin32Error())}
    if(-not [OwnedConsole360]::SetConsoleCtrlHandler([IntPtr]::Zero,$true)){throw 'Cannot protect sender from its own console signal.'}
    $sent=[OwnedConsole360]::GenerateConsoleCtrlEvent(0,0)
    if(-not $sent){throw ('Ctrl+C failed: '+[Runtime.InteropServices.Marshal]::GetLastWin32Error())}
}catch{$failure=$_.Exception.Message}
finally{if($attached){[OwnedConsole360]::FreeConsole()|Out-Null}}
$exited=$process.WaitForExit(5000)
$result=[ordered]@{at=[DateTimeOffset]::Now.ToString('o');targetPid=21756;identityChecked=$true;isoAlreadyDismounted=$true;ctrlCSent=$sent;targetExited=$exited;error=$failure;terminateProcessUsed=$false;vmStarted=$false;userAppsTouched=$false}
$stream=[IO.File]::Open($receipt,[IO.FileMode]::CreateNew)
try{$bytes=[Text.UTF8Encoding]::new($false).GetBytes(($result|ConvertTo-Json -Depth 3));$stream.Write($bytes,0,$bytes.Length);$stream.Flush($true)}finally{$stream.Dispose()}
