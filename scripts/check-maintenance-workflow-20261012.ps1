param([switch]$CurrentController,[switch]$CurrentTools,[switch]$DataMaintenanceFlow,[ValidateSet('none','held','released')][string]$CommitRecovery='none',[ValidateRange(0,2048)][int]$HistoryEntries=0,[string]$SourceVersion='0.1.2',[string]$CandidateVersion='0.1.3')
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
$run=Join-Path $root ('reports\maintenance-workflow-'+[Guid]::NewGuid().ToString('N'))
$install=Join-Path $run 'installed app';$data=Join-Path $run 'data';$project=Join-Path $run 'project';$stage=Join-Path $run 'maintenance'
$source=Join-Path $root ('reports\p6-windows-package-'+$SourceVersion.Replace('.','')+'-20261012');$candidate=Join-Path $root ('reports\p6-windows-package-'+$CandidateVersion.Replace('.','')+'-20261012')
$frozen=Join-Path $root 'reports\maintenance-native-hKaD67\maintenance-cache\session-24924-1791799715438943400'
$uninstallKey='HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\CeruleanLonging'
$productKey='HKCU:\Software\ceruleanlonging\CeruleanLonging'
foreach($key in @($uninstallKey,$productKey)){if(Test-Path -LiteralPath $key){throw 'Existing user installation; do not modify it'}}
$null=New-Item -ItemType Directory -Path $run,$project,$stage
$window=$null;$ui=$null;$edits=$null;$coreStarted=$false;$calls=[Collections.Generic.List[object]]::new();$failure=$null;$result=@{};$pendingOperation=$null
$commitWatcher=$null;$commitProcess=$null;$faultEvidence=$null
$env:PSModulePath=Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\Modules'
function Hash([string]$path){return (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()}
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
        $expectedCount=if($dataMode){7}else{9}
        if($script:edits.Count -eq $expectedCount){break};Start-Sleep -Milliseconds 100
    }
    $inventory=@($script:ui.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition) | ForEach-Object {@{type=$_.Current.ControlType.ProgrammaticName;name=$_.Current.Name;id=$_.Current.AutomationId}})
    [IO.File]::WriteAllText((Join-Path $run ('controls-'+$script:window.Id+'.json')),($inventory|ConvertTo-Json -Depth 4),[Text.UTF8Encoding]::new($false))
    if($script:edits.Count -ne $expectedCount){throw "Unexpected own editor layout: $($script:edits.Count)"}
    $locationIndex=if($dataMode){6}else{8}
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
    $outputIndex=if($script:dataWindowMode){5}else{7}
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
    for($i=0;$i -lt 30;$i++){try{$shown=(ReadEdit 7)|ConvertFrom-Json;if($shown.directory -ieq $source){break}}catch{};Start-Sleep -Milliseconds 100}
    if($shown.digest -cne (Hash (Join-Path $source 'manifest.json'))){throw 'Source displayed digest differs'}
    SetEdit 3 $candidate;Click '读取新安装包'
    for($i=0;$i -lt 30;$i++){try{$shown=(ReadEdit 7)|ConvertFrom-Json;if($shown.directory -ieq $candidate){break}}catch{};Start-Sleep -Milliseconds 100}
    if($shown.digest -cne (Hash (Join-Path $candidate 'manifest.json'))){throw 'Candidate displayed digest differs'}
    Check '使用已核对来源的内部未签名包（不代表正式发布）'
}
function AssertVersion([string]$version){
    $raw=& (Join-Path $install 'cerulean-runtime.exe') --build-info
    if($LASTEXITCODE -ne 0 -or ($raw|ConvertFrom-Json).version -cne $version -or (Get-ItemProperty -LiteralPath $uninstallKey).DisplayVersion -cne $version){throw 'Installed version differs'}
}
try{
    Native (Join-Path $source 'setup.exe') @('/S','/NS',"/D=$install");AssertVersion $SourceVersion
    $coreStarted=$true;$null=Core @('service','start');$null=Core @('project','add','--id','maintenance-ui','--root',$project,'--timezone','Asia/Shanghai')
    $original=(Core @('project','show','--project','maintenance-ui')).result.workflow | ConvertTo-Json -Depth 40 -Compress
    # Controlled staging fixture: exact frozen 100 scripts, installed source runtime,
    # explicit isolated installation/data. Product staging itself was verified in 100.
    $config=Get-Content -LiteralPath (Join-Path $frozen 'session.json') -Raw -Encoding UTF8 | ConvertFrom-Json
    $config.directory=$stage;$config.installationDirectory=$install;$config.dataDirectory=$data
    if($CurrentController -or $CurrentTools){$config.version=(Get-Content -LiteralPath (Join-Path $root 'package.json') -Raw -Encoding UTF8|ConvertFrom-Json).version}
    foreach($entry in $config.artifacts){
        $inputFile=if($entry.path -eq 'cerulean-runtime.exe'){Join-Path $install $entry.path}else{Join-Path $frozen $entry.path}
        if($entry.path -ne 'cerulean-runtime.exe' -and (Hash $inputFile) -cne $entry.sha256){throw 'Frozen maintenance source changed'}
        if(($CurrentTools -and $entry.path.EndsWith('.ps1')) -or ($CurrentController -and $entry.path -eq 'maintenance-window.ps1')){
            [IO.File]::WriteAllText((Join-Path $stage $entry.path),[IO.File]::ReadAllText((Join-Path $PSScriptRoot $entry.path),[Text.Encoding]::UTF8),[Text.UTF8Encoding]::new($true))
        }else{Copy-Item -LiteralPath $inputFile -Destination (Join-Path $stage $entry.path)}
        $entry.sha256=Hash (Join-Path $stage $entry.path);$entry.bytes=(Get-Item -LiteralPath (Join-Path $stage $entry.path)).Length
    }
    [IO.File]::WriteAllText((Join-Path $stage 'session.json'),($config|ConvertTo-Json -Depth 8),[Text.UTF8Encoding]::new($false));$script:sessionDigest=Hash (Join-Path $stage 'session.json')
    OpenWindow
    if($CommitRecovery -ne 'none'){
        $null=Action 'coreReview' '审阅升级状态'
        $held=Action 'corePrepare' '确认锁定并停止核心' $true;$coreStarted=$false
        $holdDigest=$held.upgradeHold.holdDigest
        $history=Join-Path $data 'upgrade-holds'
        if($CommitRecovery -eq 'released'){
            $null=New-Item -ItemType Directory -Path $history
            # A bounded, pre-existing historical archive fixture exercises the
            # real baseline scan after release. No delay/hook changes the updater.
            for($n=1;$n -le $HistoryEntries;$n++){
                Copy-Item -LiteralPath (Join-Path $data 'upgrade-hold.json') -Destination (Join-Path $history ('released-'+$holdDigest+'-'+$n+'.json'))
            }
        }
        Packages
        $script:recoveryTransaction=Join-Path $run ('transaction-recovery-'+$CommitRecovery)
        SetEdit 5 $script:recoveryTransaction
        $prepared=Action 'Prepare' '准备备份和程序' $true
        $null=Action 'Apply' '执行更新' $true;AssertVersion $CandidateVersion
        if($CommitRecovery -eq 'held'){
            # Real Windows sharing violation at hold archival, after durable intent.
            # Only our exact hold file is pinned; no ACL or process policy changes.
            $pin=[IO.File]::Open((Join-Path $data 'upgrade-hold.json'),[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::Read)
            try{$receipt=Action 'Commit' '确认完成升级' $true $true}finally{$pin.Dispose()}
            $script:faultEvidence=@{kind='real-hold-rename-sharing-violation';exit=$receipt.exit;childExited=$true;processKilled=$false}
        }else{
            $script:commitWatcher=[OwnedCommitReleaseWatcher]::new($history,('released-'+$holdDigest+'-*.json'))
            $receipt=Action 'Commit' '确认完成升级' $true $true
            for($i=0;$i -lt 100 -and -not $script:commitWatcher.Done;$i++){Start-Sleep -Milliseconds 50}
            $script:faultEvidence=@{kind='real-commit-process-termination-after-release';pid=$script:commitProcess.Id;done=$script:commitWatcher.Done;wasAlive=$script:commitWatcher.WasAlive;hasExited=$script:commitWatcher.HasExited;exit=$script:commitWatcher.ExitCode;error=$script:commitWatcher.Error;observedPath=$script:commitWatcher.ObservedPath;processKilled=$true}
            [IO.File]::WriteAllText((Join-Path $run 'fault-evidence.json'),($script:faultEvidence|ConvertTo-Json),[Text.UTF8Encoding]::new($false))
            if(-not $script:commitWatcher.Done -or $script:commitWatcher.Error -or -not $script:commitWatcher.HasExited){throw 'Original release interruption not verified'}
            $script:commitWatcher.Dispose();$script:commitWatcher=$null
        }
        CloseWindow;OpenWindow
        SetEdit 5 $script:recoveryTransaction;SetEdit 6 $prepared.planDigest
        Check '使用已核对来源的内部未签名包（不代表正式发布）'
        $observed=Action 'Review' '读取事务'
        if($observed.state -cne 'commit-intent'){throw 'Original commit is not unresolved; do not create another fault'}
        if($CommitRecovery -eq 'held'){
            if($observed.releaseObservation.outcome -cne 'still-held'){throw 'Original hold is not proven'}
            $abandoned=Action 'AbandonCommit' '放弃未完成提交' $true
            if($abandoned.state -cne 'commit-abandoned'){throw 'Abandon did not preserve original outcome'}
            $rolled=Action 'Rollback' '恢复原程序' $true
            if($rolled.state -cne 'rolled-back' -or -not $rolled.holdPresent){throw 'Rollback lost original hold'}
            AssertVersion $SourceVersion
        }else{
            if($observed.releaseObservation.outcome -cne 'released'){throw 'Original release is not proven'}
            $before=@(Get-ChildItem -LiteralPath $data -Recurse -File | Sort-Object FullName | ForEach-Object {$_.FullName+':'+(Hash $_.FullName)}) -join "`n"
            $reconciled=Action 'Reconcile' '核对提交结果' $true
            if($reconciled.state -cne 'committed' -or $reconciled.holdPresent){throw 'Reconcile did not acknowledge original release'}
            $after=@(Get-ChildItem -LiteralPath $data -Recurse -File | Sort-Object FullName | ForEach-Object {$_.FullName+':'+(Hash $_.FullName)}) -join "`n"
            if($after -cne $before){throw 'Reconcile changed data or released again'}
            AssertVersion $CandidateVersion
        }
        CloseWindow
        if($CommitRecovery -eq 'held'){$null=Core @('upgrade','release','--hold-digest',$holdDigest)}
        $coreStarted=$true;$null=Core @('service','start')
        $after=(Core @('project','show','--project','maintenance-ui')).result.workflow|ConvertTo-Json -Depth 40 -Compress
        if($after -cne $original){throw 'Project changed after commit recovery'}
        $null=Core @('service','stop');$coreStarted=$false
        $result=@{status='passed';run=$run;sourceVersion=$SourceVersion;candidateVersion=$CandidateVersion;commitRecovery=$CommitRecovery;preexistingArchiveEntries=$HistoryEntries;faultEvidence=$script:faultEvidence;actualWinFormsControls=$true;displayedResultsVerified=$true;reopenedBeforeRecovery=$true;projectUnchanged=$true;stagingFixture=$true;productDesktopLauncherRetested=$false;realModelCalls=0;userDesktopActions=0;calls=$calls}
    }elseif($DataMaintenanceFlow){
        $priorWindows=@(Get-ChildItem -LiteralPath $stage -Filter 'data-window-*.json' | ForEach-Object Name)
        Click '打开数据迁移与恢复'
        $deadline=[DateTime]::UtcNow.AddSeconds(10);$entry=$null
        do{
            $opened=@(Get-ChildItem -LiteralPath $stage -Filter 'data-window-*.json' | Where-Object {$_.Name -notin $priorWindows})
            if($opened.Count -gt 1){throw 'One click opened multiple data windows'}
            if($opened.Count -eq 1){$entry=Get-Content -LiteralPath $opened[0].FullName -Raw -Encoding UTF8|ConvertFrom-Json;break}
            Start-Sleep -Milliseconds 100
        }while([DateTime]::UtcNow -lt $deadline)
        if(-not $entry -or $entry.sessionDigest -cne $script:sessionDigest -or $entry.automaticDataActions -ne 0){throw 'Data window launch receipt not confirmed; do not repeat'}
        $dataWindow=[Diagnostics.Process]::GetProcessById($entry.pid)
        CloseWindow;OpenWindow $true $dataWindow
        if(Test-Path -LiteralPath (Join-Path $stage 'operations')){throw 'Opening the data window changed data'}
        $null=Action 'coreReview' '审阅升级状态'
        $null=Action 'corePrepare' '确认锁定并停止核心' $true;$coreStarted=$false
        $backup=Join-Path $run 'data-backup';SetEdit 2 $backup
        $saved=Action 'dataBackup' '创建锁定数据备份' $true
        $savedDigest=$saved.result.manifestDigest
        if((ReadEdit 3) -cne $savedDigest){throw 'Backup digest not displayed'}
        $null=Action 'dataVerify' '核对备份文件'
        $migration=Action 'formatReview' '审阅数据迁移'
        if((ReadEdit 4) -cne $migration.result.digest){throw 'Migration review not displayed'}
        $migrated=Action 'formatMigrate' '确认迁移到格式2' $true
        if($migrated.result.dataSchema -ne 2){throw 'Migration did not publish format two'}
        CloseWindow;OpenWindow $true
        $null=Action 'coreStatus' '读取现有锁定'
        $observed=Action 'formatStatus' '读取当前数据格式'
        if($observed.result.dataSchema -ne 2 -or $observed.result.sourceBackupDigest -cne $savedDigest){throw 'Reopened migration result differs'}
        $null=Action 'dataRelease' '明确解除数据锁定' $true
        CloseWindow
        $coreStarted=$true;$null=Core @('service','start')
        $after=(Core @('project','show','--project','maintenance-ui')).result.workflow|ConvertTo-Json -Depth 40 -Compress
        if($after -cne $original){throw 'Project changed after migration'}
        $null=Core @('service','stop');$coreStarted=$false
        $preserved=Join-Path $run 'preserved-migrated-data'
        $owned=[IO.Path]::GetFullPath($run).TrimEnd('\')+'\'
        foreach($path in @($data,$preserved)){if(-not ([IO.Path]::GetFullPath($path)).StartsWith($owned,[StringComparison]::OrdinalIgnoreCase)){throw 'Preservation path outside owned fixture'}}
        if(Test-Path -LiteralPath $preserved){throw 'Preserve prior recovery evidence'}
        Move-Item -LiteralPath $data -Destination $preserved
        OpenWindow $true;SetEdit 2 $backup
        $null=Action 'dataVerify' '核对备份文件'
        $restored=Action 'dataRestore' '从备份恢复原位置' $true
        if($restored.result.manifestDigest -cne $savedDigest){throw 'Restore backup differs'}
        $files=@(Get-ChildItem -LiteralPath (Join-Path $backup 'payload') -Recurse -File)
        $actual=@(Get-ChildItem -LiteralPath $data -Recurse -File)
        if($files.Count -ne $actual.Count){throw 'Restored file set differs'}
        foreach($item in $files){$relative=$item.FullName.Substring((Join-Path $backup 'payload').Length+1);if((Hash $item.FullName) -cne (Hash (Join-Path $data $relative))){throw 'Restored original bytes differ'}}
        $null=Action 'coreStatus' '读取现有锁定'
        $observed=Action 'formatStatus' '读取当前数据格式'
        if($observed.result.dataSchema -ne 1){throw 'Restore did not recover legacy format'}
        $null=Action 'dataRelease' '明确解除数据锁定' $true
        CloseWindow
        $coreStarted=$true;$null=Core @('service','start')
        $after=(Core @('project','show','--project','maintenance-ui')).result.workflow|ConvertTo-Json -Depth 40 -Compress
        if($after -cne $original){throw 'Project changed after restore'}
        $null=Core @('service','stop');$coreStarted=$false
        $result=@{status='passed';run=$run;sourceVersion=$SourceVersion;dataMaintenanceFlow=$true;currentTools=[bool]$CurrentTools;actualWinFormsControls=$true;displayedResultsVerified=$true;stagingFixture=$true;productDesktopLauncherRetested=$false;parentButtonLaunchedDataWindow=$true;formatMigration=$true;reopenedResultVerified=$true;originalLocationRestoreExact=$true;preservedDirectoryByTestDriver=$true;explicitReleaseCount=2;projectUnchanged=$true;realModelCalls=0;userDesktopActions=0;calls=$calls}
    }else{
    $review=Action 'coreReview' '审阅升级状态';if(-not $review.upgradeReview.ready){throw 'Not ready for maintenance'}
    $hold=Action 'corePrepare' '确认锁定并停止核心' $true;$coreStarted=$false
    if(-not $hold.upgradeHold.validRecord){throw 'Core did not create a valid hold'}
    $null=Action 'coreStatus' '读取现有锁定';Packages
    $tx=Join-Path $run 'transaction-rollback';SetEdit 5 $tx
    $prepared=Action 'Prepare' '准备备份和程序' $true
    if($prepared.state -cne 'prepared' -or (ReadEdit 6) -cne $prepared.planDigest){throw 'Prepared digest was not populated'}
    $plan=$prepared.planDigest;$null=Action 'Review' '读取事务'
    $applied=Action 'Apply' '执行更新' $true;if($applied.state -cne 'installed-held'){throw 'Update did not retain hold'};AssertVersion $CandidateVersion
    CloseWindow;OpenWindow
    SetEdit 5 $tx;SetEdit 6 $plan;Check '使用已核对来源的内部未签名包（不代表正式发布）'
    $reopened=Action 'Review' '读取事务';if($reopened.state -cne 'installed-held'){throw 'Reopened transaction differs'}
    $rolled=Action 'Rollback' '恢复原程序' $true;if($rolled.state -cne 'rolled-back'){throw 'Rollback failed'};AssertVersion $SourceVersion
    $null=Action 'coreStatus' '读取现有锁定';Packages;SetEdit 5 (Join-Path $run 'transaction-commit')
    $null=Action 'Prepare' '准备备份和程序' $true
    $null=Action 'Apply' '执行更新' $true;AssertVersion $CandidateVersion
    $committed=Action 'Commit' '确认完成升级' $true
    if($committed.state -cne 'committed' -or $committed.holdPresent){throw 'Commit did not release hold'}
    CloseWindow
    $coreStarted=$true;$null=Core @('service','start')
    $after=(Core @('project','show','--project','maintenance-ui')).result.workflow|ConvertTo-Json -Depth 40 -Compress
    if($after -cne $original){throw 'Project changed across UI maintenance'}
    $null=Core @('service','stop');$coreStarted=$false
    $result=@{status='passed';run=$run;sourceVersion=$SourceVersion;candidateVersion=$CandidateVersion;currentController=[bool]$CurrentController;currentTools=[bool]$CurrentTools;actualWinFormsControls=$true;displayedResultsVerified=$true;stagingFixture=$true;productDesktopLauncherRetested=$false;updateAndRollback=$true;windowReopenedBeforeRollback=$true;commit=$true;projectUnchanged=$true;realModelCalls=0;userDesktopActions=0;calls=$calls}
    }
}catch{$failure=$_;$result=@{status='failed';run=$run;message=$_.Exception.Message;pendingOperation=$script:pendingOperation;faultEvidence=$script:faultEvidence;calls=$calls}}
finally{
    if($script:commitWatcher){$script:commitWatcher.Dispose()}
    if($script:commitProcess){$script:commitProcess.Dispose()}
    $cleanup='not-run'
    try{
        CloseWindow
        if($coreStarted){$null=Core @('service','stop');$coreStarted=$false}
        if(Test-Path -LiteralPath (Join-Path $install 'uninstall.exe')){
            $runner=Join-Path $run 'test-uninstall.exe';Copy-Item -LiteralPath (Join-Path $install 'uninstall.exe') -Destination $runner
            Native $runner @('/S',"_?=$install")
            if(Test-Path -LiteralPath $uninstallKey){throw 'Test uninstall registration remains'}
            if(Test-Path -LiteralPath $productKey){if((Get-Item -LiteralPath $productKey).GetValue('') -ine $install){throw 'Product registration ownership differs'};Remove-Item -LiteralPath $productKey -Recurse}
        }
        $cleanup='passed'
    }catch{$cleanup=$_.Exception.Message}
    $result.cleanup=$cleanup
    [IO.File]::WriteAllText((Join-Path $run 'result.json'),($result|ConvertTo-Json -Depth 20),[Text.UTF8Encoding]::new($false))
    $result|ConvertTo-Json -Depth 20 -Compress
}
if($failure -or $cleanup -ne 'passed'){exit 1}
