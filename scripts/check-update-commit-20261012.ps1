param([ValidateSet('held','released','legacy','crash','crash-released')][string]$Mode)
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

function AppendCommitIntent([bool]$legacy=$false){
    # Explicit controlled missing-completion-record fixture. The actual release
    # in positive cases below is performed by the real frozen core, once.
    $files=@(Get-ChildItem -LiteralPath $transaction -Filter 'event-*.json' | Sort-Object Name)
    $record=@{schema=1;sequence=$files.Count+1;previousDigest=(Get-FileHash -LiteralPath $files[-1].FullName).Hash.ToLowerInvariant();kind='commit-intent';at=[DateTimeOffset]::Now.ToString('o')}
    if(-not $legacy){$record.detail=@{releaseMode='in-process-v1'}}
    [IO.File]::WriteAllText((Join-Path $transaction ('event-{0:D3}.json' -f $record.sequence)),($record|ConvertTo-Json -Depth 8),[Text.UTF8Encoding]::new($false))
}
function HashData {
    return @((Get-ChildItem -LiteralPath $data -Recurse -File | Sort-Object FullName | ForEach-Object {$_.FullName.Substring($data.Length)+':'+(Get-FileHash -LiteralPath $_.FullName).Hash}) -join "`n")[0]
}
function ConfirmRelease {
    $before=HashData
    $value=Update 'Reconcile'
    if($value.state -cne 'committed' -or $value.releaseObservation.outcome -cne 'released'){throw 'Release was not reconciled'}
    if((HashData) -cne $before){throw 'Reconcile modified core data or released a second time'}
    Update 'Reconcile' 'Only an unresolved commit intent' | Out-Null
    Update 'Rollback' 'Commit has started' | Out-Null
}
function InterruptCommit([switch]$AfterRelease) {
    $watch=if($AfterRelease){[IO.FileSystemWatcher]::new($history,('released-'+$hold+'-*.json'))}else{[IO.FileSystemWatcher]::new($transaction,'event-006.json')}
    $watch.NotifyFilter=[IO.NotifyFilters]::FileName;$watch.EnableRaisingEvents=$true
    $child=$null
    try{
        $launch=@('-NoProfile','-ExecutionPolicy','RemoteSigned','-File',('"'+$tool+'"'),'-Action','Commit','-TransactionDirectory',('"'+$transaction+'"'),'-PlanDigest',$digest,'-AllowUnsignedInternal')
        $child=Start-Process -FilePath powershell.exe -ArgumentList $launch -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $run 'commit-child.stdout') -RedirectStandardError (Join-Path $run 'commit-child.stderr')
        # Cache the exact process handle while alive, before observing the event.
        $processHandle=$child.Handle;$started=$child.StartTime.ToUniversalTime().ToString('o')
        $notice=$watch.WaitForChanged(([IO.WatcherChangeTypes]::Created -bor [IO.WatcherChangeTypes]::Renamed),15000)
        if($notice.TimedOut){throw 'No durable commit intent was observed; inspect original process'}
        $child.Refresh();if($child.HasExited){throw 'Commit process already ended; cannot claim live termination'}
        $child.Kill();$waited=$child.WaitForExit(10000);$child.Refresh()
        if(-not $waited -or -not $child.HasExited){throw 'Own commit process termination unconfirmed'}
        return @{pid=$child.Id;startedAt=$started;waitReturned=$waited;hasExited=$child.HasExited;exit=$child.ExitCode;processHandleCaptured=($processHandle -ne [IntPtr]::Zero);observedCheckpoint=if($AfterRelease){'release-archive-published'}else{'commit-intent-published'}}
    }finally{ $watch.Dispose();if($child){if(-not $child.HasExited){$child.Kill();$null=$child.WaitForExit(10000)};$child.Dispose()} }
}
try{
    Native (Join-Path $source 'setup.exe') @('/S','/NS',"/D=$install")
    AssertVersion '0.1.1'
    $coreStarted=$true;$null=Core @('service','start')
    $null=Core @('project','add','--id','commit-test','--root',$project,'--timezone','Asia/Shanghai')
    $review=Core @('upgrade','review')
    $hold=(Core @('upgrade','prepare','--review-digest',$review.upgradeReview.digest)).upgradeHold.holdDigest;$coreStarted=$false
    $history=Join-Path $data 'upgrade-holds';$holdPath=Join-Path $data 'upgrade-hold.json'
    if($Mode -in @('legacy','crash-released')){
        # A matching archive already present before the transaction must not prove a new release.
        $null=New-Item -ItemType Directory -Path $history
        if($Mode -eq 'legacy'){Copy-Item -LiteralPath $holdPath -Destination (Join-Path $history ('released-'+$hold+'-1.json'))}
    }
    Prepare ('commit-'+$Mode) $hold
    $null=Update 'Apply';AssertVersion '0.1.2'
    $crash=$null;$recovery=''
    if($Mode -eq 'held'){
        AppendCommitIntent
        $null=New-Item -ItemType Directory -Path $history
        $observation=Update 'Review';if($observation.releaseObservation.outcome -cne 'still-held'){throw 'Pending hold misclassified'}
        Update 'Reconcile' 'Release is not proven: still-held' | Out-Null
        $abandoned=Update 'AbandonCommit';if($abandoned.state -cne 'commit-abandoned'){throw 'Abandonment failed'}
        Update 'Commit' 'Commit has started' | Out-Null
        $null=Update 'Rollback';AssertVersion '0.1.1';$recovery='held-abandoned-and-rolled-back'
    }elseif($Mode -in @('released','legacy')){
        AppendCommitIntent ($Mode -eq 'legacy')
        if($Mode -eq 'legacy'){
            Update 'AbandonCommit' 'Legacy release helper lifetime is unproven' | Out-Null
            $missing=Join-Path $run 'original-hold-preserved.json'
            [IO.File]::Move($holdPath,$missing)
            $observation=Update 'Review';if($observation.releaseObservation.outcome -cne 'unproven'){throw 'Pre-existing archive falsely proved release'}
            Update 'Reconcile' 'Release is not proven: unproven' | Out-Null
            [IO.File]::Move($missing,$holdPath)
        }
        $null=Core @('upgrade','release','--hold-digest',$hold)
        $archive=@(Get-ChildItem -LiteralPath $history -Filter ('released-'+$hold+'-*.json') | Where-Object {$_.Name -cne ('released-'+$hold+'-1.json')})[0]
        if($Mode -eq 'released'){
            $saved=[IO.File]::ReadAllBytes($archive.FullName)
            [IO.File]::WriteAllText($archive.FullName,'changed test proof')
            Update 'Reconcile' 'Release is not proven: conflict' | Out-Null
            [IO.File]::WriteAllBytes($archive.FullName,$saved)
            $duplicate=Join-Path $history ('released-'+$hold+'-2.json')
            Copy-Item -LiteralPath $archive.FullName -Destination $duplicate
            Update 'Reconcile' 'Release is not proven: conflict' | Out-Null
            [IO.File]::Move($duplicate,(Join-Path $run 'duplicate-proof-preserved.json'))
            Copy-Item -LiteralPath $archive.FullName -Destination $holdPath
            Update 'Reconcile' 'Release is not proven: conflict' | Out-Null
            [IO.File]::Move($holdPath,(Join-Path $run 'conflicting-hold-preserved.json'))
            $coreStarted=$true;$null=Core @('service','start')
            $observation=Update 'Review';if($observation.releaseObservation.outcome -cne 'unavailable'){throw 'Active core was not respected'}
            Update 'Reconcile' 'Maintenance lock is unavailable' | Out-Null
            $null=Core @('service','stop');$coreStarted=$false
        }
        Update 'AbandonCommit' $(if($Mode -eq 'legacy'){'Legacy release helper lifetime is unproven'}else{'Cannot abandon after a possible release: released'}) | Out-Null
        ConfirmRelease;$recovery='original-release-reconciled-without-data-write'
    }else{
        $crash=InterruptCommit -AfterRelease:($Mode -eq 'crash-released')
        $observation=Update 'Review';$crash.observedState=$observation.state;$crash.releaseObservation=$observation.releaseObservation
        if($observation.state -ceq 'committed'){
            Update 'Rollback' 'Commit has started' | Out-Null;$recovery='durable-commit-confirmed-after-lost-process-output'
        }elseif($observation.releaseObservation.outcome -ceq 'released'){
            ConfirmRelease;$recovery='release-after-crash-reconciled'
        }elseif($observation.releaseObservation.outcome -ceq 'still-held'){
            $null=Update 'AbandonCommit';$null=Update 'Rollback';AssertVersion '0.1.1';$recovery='pre-release-crash-abandoned-and-rolled-back'
        }else{throw 'Crash outcome remains unproven'}
    }
    $result=[ordered]@{status='passed';mode=$Mode;run=$run;recovery=$recovery;crash=$crash;realModelCalls=0;userDesktopActions=0;calls=$calls;automaticCoreStart=$false}
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
