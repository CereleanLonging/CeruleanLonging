$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$run=Join-Path $root 'reports\reader-window-124-native'
if(-not (Test-Path -LiteralPath (Join-Path $run 'native-started.json')) -or (Test-Path -LiteralPath (Join-Path $run 'maintenance\operations'))){throw 'Resume only original pre-action window'}
$fixture=Get-Content -LiteralPath (Join-Path $run 'fixture.json') -Raw -Encoding UTF8|ConvertFrom-Json
$data=$fixture.data.Replace('\\?\','');$install=$fixture.install;$stage=Join-Path $run 'maintenance'
# Existing staged tools and original window are retained unchanged.
$source=Get-Content -LiteralPath (Join-Path $root 'scripts\check-migration-recovery-window-20261012.ps1') -Raw -Encoding UTF8
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type -AssemblyName System.Windows.Forms
$nativeSource=[regex]::Match($source,"(?s)Add-Type -TypeDefinition @'\r?\n(.*?)\r?\n'@").Groups[1].Value
Add-Type -TypeDefinition $nativeSource
$product=Get-Content -LiteralPath (Join-Path $root 'scripts\maintenance-window.ps1') -Raw -Encoding UTF8
$captureSource=[regex]::Match($product,"(?s)Add-Type -ReferencedAssemblies System.Windows.Forms -TypeDefinition @'\r?\n(.*?)\r?\n'@").Groups[1].Value
Add-Type -ReferencedAssemblies System.Windows.Forms -TypeDefinition $captureSource
function FunctionText([string]$text,[string]$name){
    $t=$null;$e=$null;$ast=[Management.Automation.Language.Parser]::ParseInput($text,[ref]$t,[ref]$e)
    if($e){throw 'Prior accepted driver syntax error'}
    $n=$ast.Find({param($x)$x -is [Management.Automation.Language.FunctionDefinitionAst] -and $x.Name -eq $name},$true)
    if(-not $n){throw "Missing prior helper $name"};return $n.Extent.Text
}
foreach($name in @('FindControl','ReadEdit','SetEdit','Check','Click')){. ([scriptblock]::Create((FunctionText $source $name)))}
$actionSource=(FunctionText $source 'Action').Replace('$outputIndex=if($script:dataWindowMode){7}else{9}','$outputIndex=9')
. ([scriptblock]::Create($actionSource))
. ([scriptblock]::Create((FunctionText $product 'QuoteArgument')))
$calls=[Collections.Generic.List[object]]::new();$pendingOperation=$null;$commitWatcher=$null;$window=$null;$serverCapture=$null
$script:dataWindowMode=$true;$script:internalSelected=$false
function Save([string]$name,$value){
    $p=Join-Path $run $name;if(Test-Path -LiteralPath $p){throw "Evidence exists $name"}
    [IO.File]::WriteAllText($p,($value|ConvertTo-Json -Depth 40),[Text.UTF8Encoding]::new($false))
}
function Hash([string]$p){return (Get-FileHash -LiteralPath $p).Hash.ToLowerInvariant()}
function StartCaptured([string]$file,[string[]]$arguments,[string]$label){
    $quoted=($arguments|ForEach-Object {QuoteArgument $_}) -join ' '
    $c=[CeruleanMaintenanceOperation]::Start($file,$quoted,(Join-Path $run ($label+'.stdout')),(Join-Path $run ($label+'.stderr')))
    Save ($label+'-process.json') @{pid=$c.Process.Id;binary=$file;arguments=$arguments}
    return $c
}
function CloseCaptured($capture,[string]$label){
    if(-not $capture.Completion.Wait(10000)){throw "Owned $label process or pipe unresolved; do not kill or replay"}
    $capture.Completion.GetAwaiter().GetResult()
    Save ($label+'-exit.json') @{pid=$capture.Process.Id;exit=$capture.Process.ExitCode;stdoutEOF=$true;stderrEOF=$true}
    if($capture.Process.ExitCode -ne 0){throw "Owned $label exited with failure"}
    $capture.Process.Dispose()
}
function RejectLocally([string]$label,[string]$expected){
    $before=@(Get-ChildItem -LiteralPath (Join-Path $stage 'operations') -Filter '*.request.json').Count
    Check '已审阅当前状态、目录和包版本，确认执行所选变更';Click $label
    $deadline=[DateTime]::UtcNow.AddSeconds(4);$observed=$false
    do{
        $texts=@($ui.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)|ForEach-Object {$_.Current.Name})
        if(@($texts|Where-Object {$_.Contains($expected)}).Count){$observed=$true;break}
        Start-Sleep -Milliseconds 100
    }while([DateTime]::UtcNow -lt $deadline)
    if(-not $observed -or @(Get-ChildItem -LiteralPath (Join-Path $stage 'operations') -Filter '*.request.json').Count -ne $before){throw 'Local rejection did not match or launched an operation'}
    $calls.Add(@{action='local-rejection';label=$label;message=$expected;newOperations=0})
}
$session=Join-Path $stage 'session.json';$sessionDigest=Hash $session
$started=Get-Content -LiteralPath (Join-Path $run 'native-started.json') -Raw -Encoding UTF8|ConvertFrom-Json
if($sessionDigest -cne $started.sessionDigest){throw 'Original session changed'}
$config=Get-Content -LiteralPath $session -Raw -Encoding UTF8|ConvertFrom-Json
foreach($entry in $config.artifacts){if((Hash (Join-Path $stage $entry.path)) -cne $entry.sha256){throw 'Original staged tools changed'}}
$windowInfo=Get-Content -LiteralPath (Join-Path $run 'window-process.json') -Raw -Encoding UTF8|ConvertFrom-Json
$nativeOwner=Get-CimInstance Win32_Process -Filter ('ProcessId = '+$windowInfo.pid)
if(-not $nativeOwner.CommandLine.Contains((Join-Path $stage 'maintenance-window.ps1'))){throw 'Original window owner differs'}
$window=Get-Process -Id $windowInfo.pid
$capturedHandle=$window.Handle
Save 'resume-original-window.json' @{pid=$window.Id;sessionDigest=$sessionDigest;reopened=$false;priorDataActions=0;reason='Legacy UIA exposes control text instead of AccessibleName; verify adjacent captions and native class, keeping same window.'}
function CloseAttachedWindow {
    if(-not $window.WaitForExit(10000)){throw 'Original window did not exit'}
    $deadline=[DateTime]::UtcNow.AddSeconds(10);$closed=$false
    do{
        try{
            foreach($name in @('window.stdout','window.stderr')){
                $stream=[IO.File]::Open((Join-Path $run $name),[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::None)
                $stream.Dispose()
            }
            $closed=$true;break
        }catch{Start-Sleep -Milliseconds 100}
    }while([DateTime]::UtcNow -lt $deadline)
    if(-not $closed){throw 'Original capture writers remain open'}
    Save 'window-exit.json' @{pid=$window.Id;exit=$window.ExitCode;stdoutWriterClosed=$true;stderrWriterClosed=$true;observedByResumedOwner=$true}
    if($window.ExitCode -ne 0){throw 'Original window exit failed'}
    $window.Dispose()
}

try{
    $deadline=[DateTime]::UtcNow.AddSeconds(25)
    do{$window.Refresh();if($window.HasExited){throw 'Own window exited before readiness'};if($window.MainWindowHandle -ne 0){break};Start-Sleep -Milliseconds 150}while([DateTime]::UtcNow -lt $deadline)
    if($window.MainWindowTitle -cne 'CeruleanLonging · 数据迁移与恢复'){throw 'Own window identity differs'}
    $ui=[Windows.Automation.AutomationElement]::FromHandle($window.MainWindowHandle)
    if($ui.Current.ProcessId -ne $window.Id){throw 'Window owner differs'}
    [MaintenanceControls]::Pid=$window.Id;[MaintenanceControls]::Root=$window.MainWindowHandle
    $deadline=[DateTime]::UtcNow.AddSeconds(5)
    do{
        $edits=@($ui.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)|Where-Object {$_.Current.NativeWindowHandle -ne 0 -and [MaintenanceControls]::Class([IntPtr]$_.Current.NativeWindowHandle) -like 'WindowsForms10.EDIT.*'})
        if($edits.Count -eq 11){break};Start-Sleep -Milliseconds 100
    }while([DateTime]::UtcNow -lt $deadline)
    $expected=@('原安装目录','数据目录','数据备份目录','已审阅备份摘要','迁移审阅摘要','监督配置启用摘要','当前数据锁定摘要','旧备份原锁定摘要','原恢复审阅摘要','审阅结果','维护副本位置')
    if($edits.Count -ne 11){throw 'Unexpected native editor count'}
    for($i=0;$i -lt $expected.Count;$i++){if([Windows.Automation.TreeWalker]::RawViewWalker.GetPreviousSibling($edits[$i]).Current.Name -cne $expected[$i]){throw "Native editor order mismatch $i"}}
    Save 'controls.json' @($edits|ForEach-Object {@{name=$_.Current.Name;handle=$_.Current.NativeWindowHandle}})
    if((ReadEdit 0) -ine $install -or (ReadEdit 1) -ine $data -or (ReadEdit 10) -ine $stage){throw 'Owned data binding differs'}
    $old=(Action 'coreStatus' '读取现有锁定').result.holdDigest
    if($old -cne $fixture.holdDigest){throw 'Initial hold differs'}
    SetEdit 2 (Join-Path $run 'cold-backup')
    $manifest=(Action 'dataBackup' '创建锁定数据备份' $true).result.manifestDigest
    $review=(Action 'readerReview' '审阅启用监督配置').result
    if($review.toReaderGeneration -ne 3 -or (ReadEdit 5) -cne $review.digest){throw 'Reader review differs'}
    SetEdit 3 ('a'*64)
    RejectLocally '确认启用监督配置' '请先审阅启用监督配置的当前计划'
    SetEdit 3 $manifest
    $review=(Action 'readerReview' '审阅启用监督配置').result
    $readerReviewed=$review
    $promoted=(Action 'readerPromote' '确认启用监督配置' $true).result
    if($promoted.minimumReaderGeneration -ne 3){throw 'Reader not promoted'}
    $format=(Action 'formatStatus' '读取当前数据格式').result
    if($format.minimumReaderGeneration -ne 3 -or $format.dataSchema -ne 1){throw 'Format changed unexpectedly'}
    $null=Action 'coreStatus' '读取现有锁定'
    $null=Action 'dataRelease' '明确解除数据锁定' $true
    $serverCapture=StartCaptured (Join-Path $stage 'cerulean-runtime.exe') @('--serve','--data-dir',$data) 'core'
    $deadline=[DateTime]::UtcNow.AddSeconds(15)
    # A one-shot read-only readiness CLI is polled; no mutation or model execution.
    do{
        $serverCapture.Process.Refresh();if($serverCapture.Process.HasExited){throw 'Own core exited before ready'}
        $oldPreference=$ErrorActionPreference;$ErrorActionPreference='Continue'
        try{$raw=& (Join-Path $stage 'cerulean-runtime.exe') --workbench --data-dir $data status 2>&1;$code=$LASTEXITCODE}finally{$ErrorActionPreference=$oldPreference}
        if($code -eq 0){$ready=($raw -join "`n")|ConvertFrom-Json;break};Start-Sleep -Milliseconds 150
    }while([DateTime]::UtcNow -lt $deadline)
    if($code -ne 0 -or $ready.serverPid -ne $serverCapture.Process.Id){throw 'Own core readiness differs'}
    $review=(Action 'coreReview' '审阅升级状态').upgradeReview
    if(-not $review.ready -or $review.serverPid -ne $serverCapture.Process.Id){throw 'Core review differs'}
    $newHold=(Action 'corePrepare' '确认锁定并停止核心' $true).upgradeHold.holdDigest
    CloseCaptured $serverCapture 'core';$serverCapture=$null
    if($newHold -ceq $old -or (ReadEdit 6) -cne $newHold){throw 'New current hold missing'}
    SetEdit 7 $old
    $recovery=(Action 'recoveryPreviousReview' '审阅旧锁定备份恢复').result
    if($recovery.schema -ne 2 -or (ReadEdit 8) -cne $recovery.digest){throw 'Previous recovery review differs'}
    RejectLocally '准备恢复副本' '请先成功审阅对应类型的恢复计划'
    $null=Action 'recoveryPreviousPrepare' '准备旧备份恢复' $true
    $restored=(Action 'recoveryApply' '保留原目录并恢复' $true).result
    if($restored.originalState -cne 'restored' -or -not $restored.restorationComplete){throw 'Recovery not restored'}
    $payload=Join-Path (Join-Path $run 'cold-backup') 'payload'
    $expectedFiles=@(Get-ChildItem -LiteralPath $payload -File -Recurse -Force|ForEach-Object {($_.FullName.Substring($payload.Length+1))+' '+(Hash $_.FullName)}|Sort-Object)
    $actualFiles=@(Get-ChildItem -LiteralPath $data -File -Recurse -Force|ForEach-Object {($_.FullName.Substring($data.Length+1))+' '+(Hash $_.FullName)}|Sort-Object)
    if(($expectedFiles -join "`n") -cne ($actualFiles -join "`n")){throw 'Restored bytes differ from original backup'}
    $null=Action 'recoveryFinish' '核验并结束恢复' $true
    $restoredHold=(Action 'coreStatus' '读取现有锁定').result.holdDigest
    if($restoredHold -cne $old -or (ReadEdit 6) -cne $old){throw 'Window did not display restored original hold'}
    $format=(Action 'formatStatus' '读取当前数据格式').result
    if($format.minimumReaderGeneration -ne 1 -or $format.dataSchema -ne 1){throw 'Old data format not restored'}
    $null=Action 'dataRelease' '明确解除数据锁定' $true
    if(-not $window.CloseMainWindow()){throw 'Own window refused close'}
    CloseAttachedWindow;$window=$null
    Save 'result.json' @{status='passed';at=[DateTimeOffset]::Now.ToString('o');calls=$calls;oldHold=$old;currentHold=$newHold;manifestDigest=$manifest;readerReview=$readerReviewed;restoredBackupBytesMatched=$true;readerGenerationBeforeRelease=3;readerGenerationAfterRecovery=1;schema2Recovery=$recovery;realModelCalls=0;userDesktopActions=0;installed=$false;limits='Actual owned native data window with copied current tools. Initial hold synthetic; new hold/stop, promotion and previous-backup recovery are actual production UI actions. No installed React entry or program replacement.'}
    Write-Output 'Actual native reader promotion and previous-backup recovery passed.'
}catch{
    Save 'failure-resume.json' @{at=[DateTimeOffset]::Now.ToString('o');message=$_.Exception.Message;stack=$_.ScriptStackTrace;calls=$calls;pending=$pendingOperation;windowPid=if($window){$window.Id}else{$null};corePid=if($serverCapture){$serverCapture.Process.Id}else{$null};automaticRetry=$false}
    throw
}
