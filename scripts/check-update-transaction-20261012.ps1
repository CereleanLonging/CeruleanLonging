$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$run=Join-Path $root ('reports\update-transaction-native-'+[Guid]::NewGuid().ToString('N'))
$install=Join-Path $run 'installed app';$data=Join-Path $run 'data';$project=Join-Path $run 'project'
$uninstallKey='HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\CeruleanLonging'
$productKey='HKCU:\Software\ceruleanlonging\CeruleanLonging'
foreach($key in @($uninstallKey,$productKey)){if(Test-Path -LiteralPath $key){throw 'Existing user installation; test refuses to modify it'}}
$source=Join-Path $root 'reports\p6-windows-package-011-20261012';$candidate=Join-Path $root 'reports\p6-windows-package-012-20261012'
$sourceDigest=(Get-FileHash -LiteralPath (Join-Path $source 'manifest.json')).Hash.ToLowerInvariant()
$candidateDigest=(Get-FileHash -LiteralPath (Join-Path $candidate 'manifest.json')).Hash.ToLowerInvariant()
$null=New-Item -ItemType Directory -Path $run;$null=New-Item -ItemType Directory -Path $project
$tool=Join-Path $PSScriptRoot 'manage-windows-update.ps1'
$calls=[Collections.Generic.List[object]]::new();$transaction=$null;$digest=$null;$coreStarted=$false;$failure=$null
function Native([string]$file,[string[]]$arguments){
    $child=Start-Process -FilePath $file -ArgumentList $arguments -WindowStyle Hidden -PassThru
    if(-not $child.WaitForExit(60000)){throw "Test installer still running: $($child.Id); do not repeat"}
    $child.Refresh();if($child.ExitCode -ne 0){throw "Installer exit $($child.ExitCode)"}
}
function Core([string[]]$arguments,[string]$rootData=$data){
    $old=$ErrorActionPreference;$ErrorActionPreference='Continue'
    try{$raw=& (Join-Path $install 'cerulean-runtime.exe') --workbench --data-dir $rootData @arguments 2>&1;$code=$LASTEXITCODE}finally{$ErrorActionPreference=$old}
    if($code -ne 0){throw "Test core failed $code $($arguments -join ' ')"}
    return (($raw -join "`n") | ConvertFrom-Json)
}
function Update([string]$action,[string]$expected=''){
    $message='';$value=$null
    try{$value=& $tool -Action $action -TransactionDirectory $script:transaction -PlanDigest $script:digest -AllowUnsignedInternal | ConvertFrom-Json}catch{$message=$_.Exception.Message}
    if($expected){if(-not $message.Contains($expected)){throw "$action expected '$expected', got '$message'"}}
    elseif($message){throw "$action failed: $message"}
    $calls.Add(@{action=$action;expectedRejection=$expected;message=$message;state=if($value){$value.state}else{$null}})
    return $value
}
function Prepare([string]$suffix,[string]$hold){
    $script:transaction=Join-Path $run ('transaction-'+$suffix)
    $value=& $tool -Action Prepare -TransactionDirectory $transaction -InstalledDirectory $install -DataDirectory $data -HoldDigest $hold -SourcePackage $source -SourceManifestSha256 $sourceDigest -CandidatePackage $candidate -CandidateManifestSha256 $candidateDigest -AllowUnsignedInternal | ConvertFrom-Json
    if($value.state -ne 'prepared'){throw 'Preparation failed'}
    $script:digest=$value.planDigest;$calls.Add(@{action='Prepare';state=$value.state;transaction=$transaction})
}
function AssertVersion([string]$version){
    $raw=& (Join-Path $install 'cerulean-runtime.exe') --build-info
    if($LASTEXITCODE -ne 0 -or ($raw | ConvertFrom-Json).version -cne $version -or (Get-ItemProperty -LiteralPath $uninstallKey).DisplayVersion -cne $version){throw 'Actual and registered program versions differ'}
}
function SimulateBoundary([string]$kind){
    # Controlled interruption fixture, not evidence of power loss or killing a live updater.
    $plan=Get-Content -LiteralPath (Join-Path $transaction 'plan.json') -Raw | ConvertFrom-Json
    $previous=Join-Path $transaction 'previous-program';$stage=Join-Path $transaction 'staged-program'
    if([IO.Path]::GetFullPath($install) -ine $plan.install -or -not $previous.StartsWith($run+'\')){throw 'Fixture path ownership mismatch'}
    [IO.Directory]::Move($install,$previous)
    if($kind -eq 'after-publish'){[IO.Directory]::Move($stage,$install)}
    $first=Join-Path $transaction 'event-001.json'
    $record=@{schema=1;sequence=2;previousDigest=(Get-FileHash -LiteralPath $first).Hash.ToLowerInvariant();kind='apply-intent';at=[DateTimeOffset]::Now.ToString('o')}
    [IO.File]::WriteAllText((Join-Path $transaction 'event-002.json'),($record|ConvertTo-Json),[Text.UTF8Encoding]::new($false))
}
function InterruptOwnUpdater {
    $watch=[IO.FileSystemWatcher]::new($script:transaction,'event-002.json')
    $watch.NotifyFilter=[IO.NotifyFilters]::FileName;$watch.EnableRaisingEvents=$true
    $child=$null
    try{
        $args=@('-NoProfile','-ExecutionPolicy','RemoteSigned','-File',('"'+$tool+'"'),'-Action','Apply','-TransactionDirectory',('"'+$script:transaction+'"'),'-PlanDigest',$script:digest,'-AllowUnsignedInternal')
        $child=Start-Process -FilePath powershell.exe -ArgumentList $args -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $run 'interrupted-updater.stdout') -RedirectStandardError (Join-Path $run 'interrupted-updater.stderr')
        $observed=$watch.WaitForChanged(([IO.WatcherChangeTypes]::Created -bor [IO.WatcherChangeTypes]::Renamed),15000)
        if($observed.TimedOut){throw 'Own updater did not publish apply intent; inspect process, do not reapply'}
        $record=Get-Content -LiteralPath (Join-Path $script:transaction 'event-002.json') -Raw | ConvertFrom-Json
        if($record.kind -cne 'apply-intent'){throw 'Unexpected updater checkpoint'}
        $child.Refresh();if($child.HasExited){throw 'Updater finished before interruption; cannot claim crash evidence'}
        $child.Kill();if(-not $child.WaitForExit(10000)){throw 'Own updater termination not confirmed'}
        return @{pid=$child.Id;exit=$child.ExitCode;checkpoint='durably-published-apply-intent';processTerminated=$true}
    }finally{
        $watch.Dispose()
        if($child){if(-not $child.HasExited){$child.Kill();$null=$child.WaitForExit(10000)};$child.Dispose()}
    }
}
try{
    Native (Join-Path $source 'setup.exe') @('/S','/NS',"/D=$install")
    AssertVersion '0.1.1'
    $coreStarted=$true;$null=Core @('service','start')
    $null=Core @('project','add','--id','update-test','--root',$project,'--timezone','Asia/Shanghai')
    $review=Core @('upgrade','review')
    $hold=(Core @('upgrade','prepare','--review-digest',$review.upgradeReview.digest)).upgradeHold.holdDigest;$coreStarted=$false
    Prepare 'rollback' $hold
    $wrong=$digest;$script:digest='0'*64;Update 'Apply' 'Transaction identity or plan digest changed' | Out-Null;$script:digest=$wrong
    $eventFile=Join-Path $transaction 'event-001.json';$eventBytes=[IO.File]::ReadAllBytes($eventFile)
    $bad=Get-Content -LiteralPath $eventFile -Raw | ConvertFrom-Json;$bad.kind='unrecognized'
    [IO.File]::WriteAllText($eventFile,($bad|ConvertTo-Json),[Text.UTF8Encoding]::new($false))
    Update 'Review' 'Invalid transaction journal transition' | Out-Null
    [IO.File]::WriteAllBytes($eventFile,$eventBytes)
    $stagedRuntime=Join-Path $transaction 'staged-program\cerulean-runtime.exe'
    $file=[IO.File]::Open($stagedRuntime,[IO.FileMode]::Append,[IO.FileAccess]::Write)
    try{$file.WriteByte(0)}finally{$file.Dispose()}
    Update 'Apply' 'Program inventory changed' | Out-Null
    Copy-Item -LiteralPath (Join-Path $candidate 'cerulean-runtime.exe') -Destination $stagedRuntime -Force
    $backupHold=Join-Path $transaction 'data-backup\payload\upgrade-hold.json';$savedHold=[IO.File]::ReadAllBytes($backupHold)
    [IO.File]::WriteAllText($backupHold,'invalid test payload')
    Update 'Apply' 'Core maintenance command rejected' | Out-Null
    [IO.File]::WriteAllBytes($backupHold,$savedHold)
    $otherData=Join-Path $run 'other-profile'
    $null=Core @('service','start') $otherData
    try{Update 'Apply' 'Program is running or unavailable' | Out-Null}finally{$null=Core @('service','stop') $otherData}
    $applied=Update 'Apply';if($applied.state -ne 'installed-held' -or -not $applied.holdPresent){throw 'Update did not retain hold'}
    AssertVersion '0.1.2';Update 'Apply' 'Apply is single-attempt' | Out-Null
    $rolled=Update 'Rollback';if($rolled.state -ne 'rolled-back' -or -not $rolled.holdPresent){throw 'Rollback did not retain hold'}
    AssertVersion '0.1.1';Update 'Rollback' 'Already rolled back' | Out-Null
    foreach($boundary in @('after-preserve','after-publish')){
        Prepare $boundary $hold;SimulateBoundary $boundary
        $null=Update 'Review';Update 'Apply' 'Apply is single-attempt' | Out-Null
        $null=Update 'Rollback';AssertVersion '0.1.1'
    }
    Prepare 'process-crash' $hold
    $crash=InterruptOwnUpdater
    $crashReview=Update 'Review';$crash.observed=$crashReview.observed;$crash.journalState=$crashReview.state
    Update 'Apply' 'Apply is single-attempt' | Out-Null
    $null=Update 'Rollback';AssertVersion '0.1.1'
    Prepare 'commit' $hold
    $null=Update 'Apply';AssertVersion '0.1.2'
    $committed=Update 'Commit';if($committed.state -ne 'committed' -or $committed.holdPresent){throw 'Commit did not release exact hold'}
    Update 'Rollback' 'Commit has started' | Out-Null
    $coreStarted=$true;$null=Core @('service','start')
    $workflow=(Core @('project','show','--project','update-test')).result.workflow
    if(($workflow|ConvertTo-Json -Depth 30 -Compress) -cne ($review.result.workflows[0]|ConvertTo-Json -Depth 30 -Compress)){throw 'Project state changed across update'}
    $null=Core @('service','stop');$coreStarted=$false
    $result=[ordered]@{status='passed';run=$run;sourceVersion='0.1.1';targetVersion='0.1.2';actualProgramReplacement=$true;actualProgramRollback=$true;registryVersionRestored=$true;holdPreservedUntilCommit=$true;projectUnchanged=$true;controlledInterruptionBoundaries=@('after-preserve','after-publish');realProcessCrashTested=$true;crash=$crash;schemaMigration=$false;modelCalls=0;userDesktopActions=0;calls=$calls}
}catch{$failure=$_;$result=@{status='failed';run=$run;message=$_.Exception.Message;calls=$calls};throw}
finally{
    if($coreStarted){try{$null=Core @('service','stop')}catch{Write-Warning 'Own core stop unconfirmed'}}
    $cleanup='not-run'
    if(Test-Path -LiteralPath (Join-Path $install 'uninstall.exe')){
        try{
            $runner=Join-Path $run 'test-uninstall.exe';Copy-Item -LiteralPath (Join-Path $install 'uninstall.exe') -Destination $runner
            Native $runner @('/S',"_?=$install")
            if(Test-Path -LiteralPath $uninstallKey){throw 'Uninstall registration remains'}
            if(Test-Path -LiteralPath $productKey){if((Get-Item -LiteralPath $productKey).GetValue('') -ine $install){throw 'Product registration owner changed'};Remove-Item -LiteralPath $productKey -Recurse}
            $cleanup='passed'
        }catch{$cleanup=$_.Exception.Message}
    }
    $result.cleanup=$cleanup
    [IO.File]::WriteAllText((Join-Path $run 'result.json'),($result | ConvertTo-Json -Depth 20),[Text.UTF8Encoding]::new($false))
    $result | ConvertTo-Json -Depth 20 -Compress
    if(-not $failure -and $cleanup -ne 'passed'){throw 'Test cleanup incomplete; inspect before another installation'}
}
