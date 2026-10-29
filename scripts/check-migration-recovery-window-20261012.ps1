param([switch]$CurrentController,[switch]$CurrentTools,[switch]$DataMaintenanceFlow,[ValidateSet('none','held','released')][string]$CommitRecovery='none',[ValidateRange(0,2048)][int]$HistoryEntries=0,[string]$SourceVersion='0.1.7',[string]$CandidateVersion='0.1.7')
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type -TypeDefinition @'
using System;using System.Text;using System.Runtime.InteropServices;
public static class MaintenanceControls {
public static uint Pid;public static IntPtr Root;
[DllImport("user32.dll")]static extern uint GetWindowThreadProcessId(IntPtr h,out uint p);
[DllImport("user32.dll")]static extern bool IsChild(IntPtr p,IntPtr h);
[DllImport("user32.dll")]public static extern bool IsWindowEnabled(IntPtr h);
[DllImport("user32.dll",CharSet=CharSet.Unicode)]static extern int GetClassName(IntPtr h,StringBuilder b,int n);
[DllImport("user32.dll",CharSet=CharSet.Unicode)]static extern IntPtr SendMessageTimeout(IntPtr h,uint m,IntPtr w,IntPtr l,uint f,uint t,out IntPtr r);
[DllImport("user32.dll",CharSet=CharSet.Unicode)]static extern IntPtr SendMessageTimeout(IntPtr h,uint m,IntPtr w,string l,uint f,uint t,out IntPtr r);
[DllImport("user32.dll",CharSet=CharSet.Unicode)]static extern IntPtr SendMessageTimeout(IntPtr h,uint m,IntPtr w,StringBuilder l,uint f,uint t,out IntPtr r);
static void Guard(IntPtr h){uint p;GetWindowThreadProcessId(h,out p);if(p!=Pid||!IsChild(Root,h))throw new Exception("Control is not an owned maintenance child");}
public static string Class(IntPtr h){Guard(h);var b=new StringBuilder(256);GetClassName(h,b,256);return b.ToString();}
public static long Message(IntPtr h,uint m){Guard(h);IntPtr r;if(SendMessageTimeout(h,m,IntPtr.Zero,IntPtr.Zero,2,5000,out r)==IntPtr.Zero)throw new Exception("Own control response unknown; do not resend");return r.ToInt64();}
public static string Read(IntPtr h){Guard(h);var b=new StringBuilder(131072);IntPtr r;if(SendMessageTimeout(h,0xD,(IntPtr)b.Capacity,b,2,5000,out r)==IntPtr.Zero)throw new Exception("Own field read timed out");return b.ToString();}
public static void Write(IntPtr h,string s){Guard(h);IntPtr r;if(SendMessageTimeout(h,0xC,IntPtr.Zero,s,2,5000,out r)==IntPtr.Zero)throw new Exception("Own field write response unknown");}
}
public sealed class OwnedCommitReleaseWatcher : IDisposable {
    readonly System.IO.FileSystemWatcher watcher;
    public volatile System.Diagnostics.Process Target;
    public volatile bool Done; public bool WasAlive,HasExited; public int ExitCode; public string Error,ObservedPath;
    int fired;
    public OwnedCommitReleaseWatcher(string directory,string pattern) {
        watcher=new System.IO.FileSystemWatcher(directory,pattern);
        watcher.NotifyFilter=System.IO.NotifyFilters.FileName;
        watcher.Created+=(s,e)=>Observed(e.FullPath);
        watcher.Renamed+=(s,e)=>Observed(e.FullPath);
        watcher.EnableRaisingEvents=true;
    }
    void Observed(string path) {
        if(System.Threading.Interlocked.Exchange(ref fired,1)!=0)return;
        try {
            ObservedPath=path;var p=Target;
            if(p==null)throw new Exception("Release observed before original process handle was captured");
            p.Refresh();WasAlive=!p.HasExited;
            if(!WasAlive)throw new Exception("Original process already exited; no termination performed");
            p.Kill();HasExited=p.WaitForExit(10000);
            if(!HasExited)throw new Exception("Original termination not yet confirmed");
            ExitCode=p.ExitCode;
        }catch(Exception e){Error=e.Message;}finally{Done=true;}
    }
    public void Dispose(){watcher.Dispose();}
}
'@
$root=Split-Path -Parent $PSScriptRoot
$run=Join-Path $root ('reports\migration-recovery-window-native-'+[Guid]::NewGuid().ToString('N'))
$install=Join-Path $run 'installed app';$data=Join-Path $run 'data';$project=Join-Path $run 'project';$stage=Join-Path $run 'maintenance'
$source=Join-Path $root ('reports\p6-windows-package-'+$SourceVersion.Replace('.','')+'-20261012');$candidate=Join-Path $root ('reports\p6-windows-package-'+$CandidateVersion.Replace('.','')+'-20261012')
$frozen=Join-Path $root 'reports\recovery-window-native-a30fdd86cd224b9fbe7edc066e93d3b8\maintenance'
$uninstallKey='HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\CeruleanLonging'
$productKey='HKCU:\Software\ceruleanlonging\CeruleanLonging'
foreach($key in @($uninstallKey,$productKey)){if(Test-Path -LiteralPath $key){throw 'Existing user installation; do not modify it'}}
$null=New-Item -ItemType Directory -Path $run,$project,$stage
$window=$null;$ui=$null;$edits=$null;$coreStarted=$false;$calls=[Collections.Generic.List[object]]::new();$failure=$null;$result=@{};$pendingOperation=$null
$commitWatcher=$null;$commitProcess=$null;$faultEvidence=$null
$env:PSModulePath=Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\Modules'
function Hash([string]$path){
    if(-not $path.StartsWith('\\?\')){$path='\\?\'+[IO.Path]::GetFullPath($path)}
    $file=[IO.File]::OpenRead($path);$hash=[Security.Cryptography.SHA256]::Create()
    try{return [BitConverter]::ToString($hash.ComputeHash($file)).Replace('-','').ToLowerInvariant()}finally{$hash.Dispose();$file.Dispose()}
}
function Native([string]$file,[string[]]$arguments){
    $p=Start-Process -FilePath $file -ArgumentList $arguments -WindowStyle Hidden -PassThru
    $handle=$p.Handle
    if(-not $p.WaitForExit(60000)){throw "Own installer still running: $($p.Id); do not repeat"}
    if($p.ExitCode -ne 0){throw "Installer failed: $($p.ExitCode)"};$p.Dispose()
}
function Core([string[]]$arguments){
    $old=$ErrorActionPreference;$ErrorActionPreference='Continue'
    try{$raw=& (Join-Path $install 'cerulean-runtime.exe') --workbench --data-dir $data @arguments 2>&1;$code=$LASTEXITCODE}finally{$ErrorActionPreference=$old}
    if($code -ne 0){throw "Own core failed: $code"};return (($raw -join "`n")|ConvertFrom-Json)
}
function FindControl($type,[string]$name){
    $condition=[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty,$name)
    $items=$script:ui.FindAll([Windows.Automation.TreeScope]::Descendants,$condition)
    if($items.Count -ne 1){throw "Expected one own control: $name; found $($items.Count)"}
    $handle=[IntPtr]$items[0].Current.NativeWindowHandle
    if([MaintenanceControls]::Class($handle) -notlike 'WindowsForms10.BUTTON.*'){throw 'Expected a native button/checkbox'}
    return $handle
}
function ReadEdit([int]$index){return [MaintenanceControls]::Read([IntPtr]$script:edits[$index].Current.NativeWindowHandle)}
function SetEdit([int]$index,[string]$value){[MaintenanceControls]::Write([IntPtr]$script:edits[$index].Current.NativeWindowHandle,$value);if((ReadEdit $index) -cne $value){throw 'Own field value differs'}}
function Check([string]$name){
    # WinForms owner-drawn checkboxes keep checked state in managed code;
    # BM_GETCHECK reports zero. Internal policy is clicked once per window.
    # Consent starts false and the product consumes it on every mutating action.
    $isInternal=$name -eq '使用已核对来源的内部未签名包（不代表正式发布）'
    if($isInternal -and $script:internalSelected){return}
    $handle=FindControl ([Windows.Automation.ControlType]::CheckBox) $name
    $null=[MaintenanceControls]::Message($handle,0xF5)
    if($isInternal){$script:internalSelected=$true}
}
function Click([string]$name){$handle=FindControl ([Windows.Automation.ControlType]::Button) $name;if(-not [MaintenanceControls]::IsWindowEnabled($handle)){throw "Own button disabled: $name"};$null=[MaintenanceControls]::Message($handle,0xF5)}
function OpenWindow([bool]$dataMode=$false,[Diagnostics.Process]$existing=$null) {
    $script:internalSelected=$false
    $script:dataWindowMode=$dataMode
    $args=@('-NoProfile','-ExecutionPolicy','RemoteSigned','-WindowStyle','Hidden','-File',('"'+(Join-Path $stage 'maintenance-window.ps1')+'"'),'-Session',('"'+(Join-Path $stage 'session.json')+'"'),'-SessionDigest',$script:sessionDigest)
    # Hide the console, not the first WinForms window (Start-Process SW_HIDE).
    $start=[Diagnostics.ProcessStartInfo]::new()
    $start.FileName=Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
    $start.Arguments=$args -join ' ';$start.UseShellExecute=$false;$start.CreateNoWindow=$true
    if($dataMode){$start.Arguments+=' -DataMaintenance'}
    $script:window=if($existing){$existing}else{[Diagnostics.Process]::Start($start)}
    $handle=$script:window.Handle;$deadline=[DateTime]::UtcNow.AddSeconds(25)
    do{
        $script:window.Refresh();if($script:window.HasExited){throw 'Own maintenance window exited before ready'}
        if($script:window.MainWindowHandle -ne 0 -and $script:window.MainWindowTitle.StartsWith('CeruleanLonging')){break}
        Start-Sleep -Milliseconds 150
    }while([DateTime]::UtcNow -lt $deadline)
    if(-not $script:window.MainWindowTitle.StartsWith('CeruleanLonging')){throw 'Own maintenance window readiness not confirmed'}
    $script:ui=[Windows.Automation.AutomationElement]::FromHandle($script:window.MainWindowHandle)
    if($script:ui.Current.ProcessId -ne $script:window.Id){throw 'Window owner differs'}
    [MaintenanceControls]::Pid=$script:window.Id;[MaintenanceControls]::Root=$script:window.MainWindowHandle
    for($i=0;$i -lt 50;$i++){
        $script:edits=@($script:ui.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition) | Where-Object {$_.Current.NativeWindowHandle -ne 0 -and [MaintenanceControls]::Class([IntPtr]$_.Current.NativeWindowHandle) -like 'WindowsForms10.EDIT.*'})
        $expectedCount=if($dataMode){9}else{11}
        if($script:edits.Count -eq $expectedCount){break};Start-Sleep -Milliseconds 100
    }
    $inventory=@($script:ui.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition) | ForEach-Object {@{type=$_.Current.ControlType.ProgrammaticName;name=$_.Current.Name;id=$_.Current.AutomationId}})
    [IO.File]::WriteAllText((Join-Path $run ('controls-'+$script:window.Id+'.json')),($inventory|ConvertTo-Json -Depth 4),[Text.UTF8Encoding]::new($false))
    if($script:edits.Count -ne $expectedCount){throw "Unexpected own editor layout: $($script:edits.Count)"}
    $locationIndex=if($dataMode){8}else{10}
    if((ReadEdit 0) -ine $install -or (ReadEdit 1) -ine $data -or (ReadEdit $locationIndex) -ine $stage){throw 'Own maintenance binding differs'}
    $calls.Add(@{action='open-window';pid=$script:window.Id;controls=$script:edits.Count})
}
function CloseWindow {
    if(-not $script:window){return}
    $script:window.Refresh()
    if(-not $script:window.HasExited){
        if($script:pendingOperation){throw 'Maintenance child outcome pending; leave window intact'}
        if(-not $script:window.CloseMainWindow() -or -not $script:window.WaitForExit(10000)){throw 'Own maintenance window still active; do not kill it'}
    }
    $script:window.Dispose();$script:window=$null;$script:ui=$null
}
function Action([string]$action,[string]$label,[bool]$mutating=$false,[bool]$expectedFailure=$false){
    if($mutating){Check '已审阅当前状态、目录和包版本，确认执行所选变更'}
    $logRoot=Join-Path $stage 'operations';$before=@(Get-ChildItem -LiteralPath $logRoot -Filter '*.request.json' -ErrorAction SilentlyContinue | ForEach-Object Name)
    Click $label
    $script:pendingOperation='clicked-'+$action+'-awaiting-receipt'
    if($action -eq 'Commit' -and $script:commitWatcher){
        $deadline=[DateTime]::UtcNow.AddSeconds(10);$ownedChild=$null
        do{
            $children=@(Get-CimInstance Win32_Process -Filter ('ParentProcessId = '+$script:window.Id) | Where-Object {$_.CommandLine -and $_.CommandLine.Contains((Join-Path $stage 'manage-windows-update.ps1')) -and $_.CommandLine.Contains($script:recoveryTransaction) -and $_.CommandLine.Contains('Commit')})
            if($children.Count -gt 1){throw 'Multiple matching commit children; do not choose one'}
            if($children.Count -eq 1){$ownedChild=$children[0];break}
            Start-Sleep -Milliseconds 25
        }while([DateTime]::UtcNow -lt $deadline)
        if(-not $ownedChild){throw 'Original commit child not captured; inspect existing operation'}
        $script:commitProcess=[Diagnostics.Process]::GetProcessById($ownedChild.ProcessId)
        $captured=$script:commitProcess.Handle
        $script:commitWatcher.Target=$script:commitProcess
        [IO.File]::WriteAllText((Join-Path $run 'owned-commit-process.json'),(@{pid=$script:commitProcess.Id;parentPid=$script:window.Id;transaction=$script:recoveryTransaction;handleCaptured=($captured -ne [IntPtr]::Zero)}|ConvertTo-Json),[Text.UTF8Encoding]::new($false))
    }
    $deadline=[DateTime]::UtcNow.AddSeconds(55);$request=$null;$receipt=$null
    do{
        $new=@(Get-ChildItem -LiteralPath $logRoot -Filter '*.request.json' -ErrorAction SilentlyContinue | Where-Object {$_.Name -notin $before})
        if($new.Count -gt 1){throw 'One click produced multiple operations'}
        if($new.Count -eq 1){
            $request=Get-Content -LiteralPath $new[0].FullName -Raw -Encoding UTF8 | ConvertFrom-Json
            if($request.action -cne $action){throw 'Different maintenance operation was started'}
            $script:pendingOperation=$new[0].FullName
            $finished=$new[0].FullName.Replace('.request.json','.finished.json')
            if(Test-Path -LiteralPath $finished){$receipt=Get-Content -LiteralPath $finished -Raw -Encoding UTF8 | ConvertFrom-Json;break}
        }
        Start-Sleep -Milliseconds 100
    }while([DateTime]::UtcNow -lt $deadline)
    if(-not $receipt){throw "Own operation completion missing: $action; inspect existing process, do not replay"}
    $script:pendingOperation=$null
    $calls.Add(@{action=$action;receipt=$finished;exit=$receipt.exit})
    # Receipt precedes control re-enabling in the actual window timer.
    $button=FindControl ([Windows.Automation.ControlType]::Button) $label
    for($i=0;$i -lt 50 -and -not [MaintenanceControls]::IsWindowEnabled($button);$i++){Start-Sleep -Milliseconds 100}
    if(-not [MaintenanceControls]::IsWindowEnabled($button)){throw 'UI did not release operation state'}
    $outputIndex=if($script:dataWindowMode){7}else{9}
    if($expectedFailure){
        if($receipt.exit -eq 0){throw 'Expected original commit failure, but it completed; do not repeat'}
        $expectedText=[IO.File]::ReadAllText($receipt.stderr,[Text.Encoding]::UTF8)+"`r`n"+[IO.File]::ReadAllText($receipt.stdout,[Text.Encoding]::UTF8)
        if((ReadEdit $outputIndex) -cne $expectedText){throw 'Window did not display the failed operation output'}
        return $receipt
    }
    if($receipt.exit -ne 0){throw "Maintenance action failed: $action; see $($receipt.stderr)"}
    $value=Get-Content -LiteralPath $receipt.stdout -Raw -Encoding UTF8 | ConvertFrom-Json
    $displayed=(ReadEdit $outputIndex)|ConvertFrom-Json
    if(($displayed|ConvertTo-Json -Depth 50 -Compress) -cne ($value|ConvertTo-Json -Depth 50 -Compress)){throw 'Window did not display the completed operation result'}
    return $value
}
function Packages {
    SetEdit 2 $source;Click '读取原安装包'
    for($i=0;$i -lt 30;$i++){try{$shown=(ReadEdit 9)|ConvertFrom-Json;if($shown.directory -ieq $source){break}}catch{};Start-Sleep -Milliseconds 100}
    if($shown.digest -cne (Hash (Join-Path $source 'manifest.json'))){throw 'Source displayed digest differs'}
    SetEdit 3 $candidate;Click '读取新安装包'
    for($i=0;$i -lt 30;$i++){try{$shown=(ReadEdit 9)|ConvertFrom-Json;if($shown.directory -ieq $candidate){break}}catch{};Start-Sleep -Milliseconds 100}
    if($shown.digest -cne (Hash (Join-Path $candidate 'manifest.json'))){throw 'Candidate displayed digest differs'}
    Check '使用已核对来源的内部未签名包（不代表正式发布）'
}
function AssertVersion([string]$version){
    $raw=& (Join-Path $install 'cerulean-runtime.exe') --build-info
    if($LASTEXITCODE -ne 0 -or ($raw|ConvertFrom-Json).version -cne $version -or (Get-ItemProperty -LiteralPath $uninstallKey).DisplayVersion -cne $version){throw 'Installed version differs'}
}
# Included into the dedicated acceptance driver at generation time, not by product.
function Normal([string]$path){if($path.StartsWith('\\?\')){return $path.Substring(4)};return $path}
function ExactBackup([string]$backupRoot){
    $payload=Join-Path $backupRoot 'payload'
    $expected=@(Get-ChildItem -LiteralPath $payload -Recurse -Force | ForEach-Object {$relative=$_.FullName.Substring($payload.Length+1);if($_.PSIsContainer){'D '+$relative}else{'F '+$relative+' '+(Hash $_.FullName)}} | Sort-Object)
    $actual=@(Get-ChildItem -LiteralPath $data -Recurse -Force | ForEach-Object {$relative=$_.FullName.Substring($data.Length+1);if($_.PSIsContainer){'D '+$relative}else{'F '+$relative+' '+(Hash $_.FullName)}} | Sort-Object)
    if(($actual -join "`n") -cne ($expected -join "`n")){throw 'Restored directory differs from every backed up directory/file'}
    return @{filesAndDirectories=$actual.Count;exact=$true}
}
function RestoreThroughWindow([string]$path,[string]$manifest,[int]$schema){
    SetEdit 2 $path;SetEdit 3 $manifest;SetEdit 5 $hold
    $review=Action 'recoveryReview' '审阅原位置恢复'
    $digest=$review.result.digest;$tx=Normal $review.result.transaction
    $prepared=Action 'recoveryPrepare' '准备恢复副本' $true
    if($prepared.result.reviewDigest -cne $digest){throw 'Prepared recovery identity differs'}
    CloseWindow;OpenWindow $true
    SetEdit 2 $path;SetEdit 3 $manifest;SetEdit 5 $hold;SetEdit 6 $digest
    $null=Action 'recoveryStatus' '读取恢复事务'
    $restored=Action 'recoveryApply' '保留原目录并恢复' $true
    if(-not $restored.result.restorationComplete -or -not $restored.result.preservedOriginal){throw 'Restoration or original preservation incomplete'}
    $exact=ExactBackup $path
    $finished=Action 'recoveryFinish' '核验并结束恢复' $true
    $inspection=Action 'recoveryInspect' '核对恢复归档'
    $archives=@($inspection.result.archives|Where-Object {$_.verified})
    if($archives.Count -ne 1 -or $inspection.result.currentDataVerified -or $inspection.result.guardPresent){throw 'Recovery history or guard state differs'}
    $format=Action 'formatStatus' '读取当前数据格式'
    $currentHold=Action 'coreStatus' '读取现有锁定'
    if($format.result.dataSchema -ne $schema -or -not $currentHold.result.validRecord -or $currentHold.result.holdDigest -cne $hold){throw 'Restoration changed expected format/hold'}
    return @{schema=$schema;backup=$path;manifestDigest=$manifest;reviewDigest=$digest;archive=$archives[0].archive;exact=$exact;holdUnchanged=$true;format=$format.result}
}
try{
    Native (Join-Path $source 'setup.exe') @('/S','/NS',"/D=$install");AssertVersion '0.1.7'
    $coreStarted=$true;$null=Core @('service','start')
    $null=Core @('project','add','--id','migration-recovery-ui','--root',$project,'--timezone','Asia/Shanghai')
    $original=(Core @('project','show','--project','migration-recovery-ui')).result.workflow|ConvertTo-Json -Depth 40 -Compress
    $config=Get-Content -LiteralPath (Join-Path $frozen 'session.json') -Raw -Encoding UTF8|ConvertFrom-Json
    foreach($entry in $config.artifacts){if((Hash (Join-Path $frozen $entry.path)) -cne $entry.sha256){throw 'Frozen 112 maintenance bytes changed'}}
    $config.directory=$stage;$config.installationDirectory=$install;$config.dataDirectory=$data
    foreach($entry in $config.artifacts){Copy-Item -LiteralPath (Join-Path $frozen $entry.path) -Destination (Join-Path $stage $entry.path)}
    [IO.File]::WriteAllText((Join-Path $stage 'session.json'),($config|ConvertTo-Json -Depth 10),[Text.UTF8Encoding]::new($false));$script:sessionDigest=Hash (Join-Path $stage 'session.json')
    OpenWindow $true
    $null=Action 'coreReview' '审阅升级状态'
    $held=Action 'corePrepare' '确认锁定并停止核心' $true;$coreStarted=$false
    $hold=$held.upgradeHold.holdDigest
    $backupOne=Join-Path $run 'before-migration';SetEdit 2 $backupOne
    $backedOne=Action 'dataBackup' '创建锁定数据备份' $true
    $digestOne=$backedOne.result.manifestDigest
    $migration=Action 'formatReview' '审阅数据迁移'
    $migrated=Action 'formatMigrate' '确认迁移到格式2' $true
    if($migrated.result.dataSchema -ne 2 -or $migrated.result.sourceBackupDigest -cne $digestOne){throw 'Migration did not bind exact original backup'}
    CloseWindow;OpenWindow $true
    $current=Action 'coreStatus' '读取现有锁定'
    $format=Action 'formatStatus' '读取当前数据格式'
    if($current.result.holdDigest -cne $hold -or $format.result.dataSchema -ne 2){throw 'Reopened migrated state differs'}
    $marker=Hash (Join-Path $data 'events\00000000.json')
    $backupTwo=Join-Path $run 'after-migration';SetEdit 2 $backupTwo
    $backedTwo=Action 'dataBackup' '创建锁定数据备份' $true
    $digestTwo=$backedTwo.result.manifestDigest
    $undo=RestoreThroughWindow $backupOne $digestOne 1
    $undoArchive=Normal $undo.archive
    if((Hash (Join-Path $undoArchive 'preserved\events\00000000.json')) -cne $marker){throw 'Migrated original marker not preserved exactly'}
    $redo=RestoreThroughWindow $backupTwo $digestTwo 2
    if((Hash (Join-Path $data 'events\00000000.json')) -cne $marker){throw 'Migrated snapshot marker not restored exactly'}
    if($redo.format.sourceBackupDigest -cne $digestOne){throw 'Original migration provenance changed during snapshot restoration'}
    $null=Action 'dataRelease' '明确解除数据锁定' $true
    CloseWindow
    $coreStarted=$true;$null=Core @('service','start')
    $after=(Core @('project','show','--project','migration-recovery-ui')).result.workflow|ConvertTo-Json -Depth 40 -Compress
    if($after -cne $original){throw 'Original project differs after format round trip'}
    $null=Core @('service','stop');$coreStarted=$false
    if([IO.Path]::GetFullPath($install) -ine (Join-Path $run 'installed app') -or (Get-Item -LiteralPath $productKey).GetValue('') -ine $install){throw 'Test installation ownership changed'}
    $runner=Join-Path $run 'test-uninstall.exe';Copy-Item -LiteralPath (Join-Path $install 'uninstall.exe') -Destination $runner
    Native $runner @('/S',"_?=$install")
    if(Test-Path -LiteralPath $uninstallKey){throw 'Installation registration remains'}
    if(Test-Path -LiteralPath $productKey){if((Get-Item -LiteralPath $productKey).GetValue('') -ine $install){throw 'Product ownership differs'};Remove-Item -LiteralPath $productKey -Recurse}
    $result=@{status='passed';run=$run;version='0.1.7';migration=$migrated.result;restoreSchemaOne=$undo;restoreSchemaTwo=$redo;originalMigrationMarkerSha256=$marker;projectUnchanged=$true;holdPreservedAcrossBothRecoveries=$true;originalDirectoriesPreservedByProduct=$true;schemaRoundTrip=@(1,2,1,2);migrationAttempts=1;explicitRestorationAttempts=2;explicitReleaseCount=1;actualWinFormsControls=$true;displayedResultsVerified=$true;stagingFixture=$true;productDesktopLauncherRetested=$false;cleanup='passed';modelCalls=0;userDesktopActions=0;calls=$calls}
}catch{
    $failure=$_;$result=@{status='failed';run=$run;message=$_.Exception.Message;scriptStack=$_.ScriptStackTrace;pendingOperation=$script:pendingOperation;windowPid=if($window){$window.Id}else{$null};cleanup='preserved-for-inspection';calls=$calls}
}finally{
    [IO.File]::WriteAllText((Join-Path $run 'result.json'),($result|ConvertTo-Json -Depth 30),[Text.UTF8Encoding]::new($false));$result|ConvertTo-Json -Depth 30 -Compress
}
if($failure){exit 1}
