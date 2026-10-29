$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$run=Join-Path $root ('reports\update-interlock-native-'+[Guid]::NewGuid().ToString('N'))
$install=Join-Path $run 'installed app';$data=Join-Path $run 'data';$project=Join-Path $run 'project'
$uninstallKey='HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\CeruleanLonging'
$productKey='HKCU:\Software\ceruleanlonging\CeruleanLonging'
foreach($key in @($uninstallKey,$productKey)){if(Test-Path -LiteralPath $key){throw 'Existing user installation; test refuses to modify it'}}
$source=Join-Path $root 'reports\p6-windows-package-015-20261012'
$candidate=Join-Path $root 'reports\p6-windows-package-016-20261012'
$sourceDigest=(Get-FileHash -LiteralPath (Join-Path $source 'manifest.json')).Hash.ToLowerInvariant()
$candidateDigest=(Get-FileHash -LiteralPath (Join-Path $candidate 'manifest.json')).Hash.ToLowerInvariant()
$null=New-Item -ItemType Directory -Path $run
$null=New-Item -ItemType Directory -Path $project
$helper=Join-Path $run 'reviewed-maintenance-core.exe'
Copy-Item -LiteralPath (Join-Path $root 'target\debug\cerulean-core.exe') -Destination $helper
$helperDigest=(Get-FileHash -LiteralPath $helper).Hash.ToLowerInvariant()
$tool=Join-Path $PSScriptRoot 'manage-windows-update.ps1'
$calls=[Collections.Generic.List[object]]::new();$nativeProcesses=[Collections.Generic.List[object]]::new()
$transaction=$null;$digest=$null;$coreStarted=$false;$result=$null;$pending=$null;$guardPin=$null
function Native([string]$file,[string[]]$arguments){
    $child=Start-Process -FilePath $file -ArgumentList $arguments -WindowStyle Hidden -PassThru
    $script:pending=$child
    # Keep the handle and mark uncertainty on timeout; no automatic relaunch/cleanup.
    if(-not $child.WaitForExit(60000)){throw "Original native process still pending: $($child.Id)"}
    $child.Refresh();$nativeProcesses.Add(@{pid=$child.Id;exit=$child.ExitCode;file=$file})
    $script:pending=$null
    if($child.ExitCode -ne 0){throw "Installer exit $($child.ExitCode)"}
}
function Core([string[]]$arguments,[switch]$Development,[switch]$Reject){
    $binary=if($Development){$helper}else{Join-Path $install 'cerulean-runtime.exe'}
    $old=$ErrorActionPreference;$ErrorActionPreference='Continue'
    try{$raw=& $binary --workbench --data-dir $data @arguments 2>&1;$code=$LASTEXITCODE}finally{$ErrorActionPreference=$old}
    $number=$calls.Count+1
    [IO.File]::WriteAllText((Join-Path $run ('core-'+$number+'.txt')),($raw -join "`n"),[Text.UTF8Encoding]::new($false))
    $calls.Add(@{kind='core';args=$arguments;exit=$code;expectedRejection=[bool]$Reject})
    if($Reject){if($code -eq 0){throw 'Core unexpectedly accepted forbidden overlap'};return}
    if($code -ne 0){throw "Core failed $code $($arguments -join ' ')"}
    return (($raw -join "`n") | ConvertFrom-Json)
}
function Update([string]$action,[string]$expected=''){
    $message='';$value=$null
    try{$value=& $tool -Action $action -TransactionDirectory $script:transaction -PlanDigest $script:digest -AllowUnsignedInternal | ConvertFrom-Json}catch{$message=$_.Exception.Message}
    $calls.Add(@{kind='update';action=$action;expectedRejection=$expected;message=$message;reply=$value})
    if($expected){if(-not $message.Contains($expected)){throw "$action expected '$expected', got '$message'"}}
    elseif($message){throw "$action failed: $message"}
    return $value
}
function Prepare([string]$suffix){
    $script:transaction=Join-Path $run ('transaction-'+$suffix)
    $value=& $tool -Action Prepare -TransactionDirectory $transaction -InstalledDirectory $install -DataDirectory $data -HoldDigest $hold -SourcePackage $source -SourceManifestSha256 $sourceDigest -CandidatePackage $candidate -CandidateManifestSha256 $candidateDigest -AllowUnsignedInternal -MaintenanceCore $helper -MaintenanceCoreSha256 $helperDigest | ConvertFrom-Json
    $calls.Add(@{kind='update';action='Prepare';reply=$value})
    if($value.state -cne 'prepared' -or $value.maintenance.state -cne 'reserved'){throw 'Preparation did not reserve maintenance'}
    $script:digest=$value.planDigest
    return $value
}
function AssertVersion([string]$version){
    $raw=& (Join-Path $install 'cerulean-runtime.exe') --build-info
    if($LASTEXITCODE -ne 0 -or ($raw | ConvertFrom-Json).version -cne $version -or (Get-ItemProperty -LiteralPath $uninstallKey).DisplayVersion -cne $version){throw 'Actual and registered program versions differ'}
}
function DataDigest {
    return ((Get-ChildItem -LiteralPath $data -Recurse -File | Sort-Object FullName | ForEach-Object {$_.FullName.Substring($data.Length)+':'+(Get-FileHash -LiteralPath $_.FullName).Hash}) -join "`n")
}
try{
    Native (Join-Path $source 'setup.exe') @('/S','/NS',"/D=$install")
    AssertVersion '0.1.5'
    $coreStarted=$true;$null=Core @('service','start')
    $null=Core @('project','add','--id','interlock-test','--root',$project,'--timezone','Asia/Shanghai')
    $review=Core @('upgrade','review')
    $hold=(Core @('upgrade','prepare','--review-digest',$review.upgradeReview.digest)).upgradeHold.holdDigest
    $coreStarted=$false;$before=DataDigest
    $prepared=Prepare 'rollback'
    Core @('service','start') -Development -Reject
    Core @('upgrade','release','--hold-digest',$hold) -Development -Reject
    $plan=Get-Content -LiteralPath (Join-Path $transaction 'plan.json') -Raw | ConvertFrom-Json
    Core @('recovery','review','--from',(Join-Path $transaction 'data-backup'),'--manifest-digest',$plan.backupDigest,'--hold-digest',$hold) -Development -Reject
    $applied=Update 'Apply'
    if($applied.state -cne 'installed-held' -or $applied.maintenance.state -cne 'reserved'){throw 'Apply lost maintenance reservation'}
    AssertVersion '0.1.6'
    $rolled=Update 'Rollback'
    if($rolled.state -cne 'rolled-back' -or $rolled.maintenance.state -cne 'released' -or -not $rolled.holdPresent){throw 'Rollback did not release reservation while retaining hold'}
    AssertVersion '0.1.5'
    if((DataDigest) -cne $before){throw 'Rollback changed data'}
    $null=Update 'Review';$null=Update 'FinishMaintenance'
    $null=Update 'Rollback' 'Maintenance reservation is not active'
    # Explicit preparation fixture: complete initial intent and real reserve,
    # without a prepared journal/receipt. This is not a killed Prepare process.
    $fixture=Get-Content -LiteralPath (Join-Path $transaction 'intent.json') -Raw | ConvertFrom-Json
    $script:transaction=Join-Path $run 'transaction-incomplete-fixture'
    $null=New-Item -ItemType Directory -Path $transaction
    $fixture.transaction=$transaction
    [IO.File]::WriteAllText((Join-Path $transaction 'intent.json'),($fixture|ConvertTo-Json -Depth 30),[Text.UTF8Encoding]::new($false))
    [IO.File]::WriteAllBytes((Join-Path $transaction 'transaction.lock'),[byte[]]@())
    $intentHash=(Get-FileHash -LiteralPath (Join-Path $transaction 'intent.json')).Hash.ToLowerInvariant()
    $reserved=Core @('maintenance','reserve','--transaction',$transaction,'--intent-digest',$intentHash,'--hold-digest',$hold) -Development
    $abandoned=& $tool -Action AbandonPreparation -TransactionDirectory $transaction -IntentDigest $intentHash -AllowUnsignedInternal | ConvertFrom-Json
    $calls.Add(@{kind='update';action='AbandonPreparation';syntheticPreparation=$true;reply=$abandoned})
    if($abandoned.maintenance.state -cne 'released' -or (DataDigest) -cne $before){throw 'Preparation abandonment changed original data or did not release'}
    $readback=& $tool -Action AbandonPreparation -TransactionDirectory $transaction -IntentDigest $intentHash -GuardDigest $reserved.result.guardDigest -AllowUnsignedInternal | ConvertFrom-Json
    if($readback.maintenance.state -cne 'released' -or (DataDigest) -cne $before){throw 'Lost-receipt explicit guard readback failed'}
    $calls.Add(@{kind='update';action='AbandonPreparation readback';reply=$readback})
    $prepared=Prepare 'commit-release-conflict'
    $null=Update 'Apply'
    # Real sharing violation only at reservation archival, after actual Commit.
    # Permit readers, deny deletion; do not fabricate a terminal journal.
    $guardPin=[IO.File]::Open($prepared.maintenance.guardPath,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::Read)
    $null=Update 'Commit' 'Core maintenance command rejected'
    $guardPin.Dispose();$guardPin=$null
    $unresolved=Update 'Review'
    if($unresolved.state -cne 'committed' -or $unresolved.maintenance.state -cne 'reserved' -or $unresolved.holdPresent){throw 'Expected committed update with unresolved reservation'}
    Core @('service','start') -Development -Reject
    $committedData=DataDigest
    $finished=Update 'FinishMaintenance'
    if($finished.state -cne 'committed' -or $finished.maintenance.state -cne 'released'){throw 'Terminal maintenance did not finish'}
    if((DataDigest) -cne $committedData){throw 'FinishMaintenance mutated data or replayed hold release'}
    $null=Update 'FinishMaintenance'
    if((DataDigest) -cne $committedData){throw 'Repeated finish observation changed data'}
    $null=Update 'Commit' 'Maintenance reservation is not active'
    AssertVersion '0.1.6'
    $coreStarted=$true;$null=Core @('service','start')
    $workflow=(Core @('project','show','--project','interlock-test')).result.workflow
    if(($workflow|ConvertTo-Json -Depth 30 -Compress) -cne ($review.result.workflows[0]|ConvertTo-Json -Depth 30 -Compress)){throw 'Original project changed'}
    $null=Core @('service','stop');$coreStarted=$false
    # Uninstall only this successfully verified, exact owned test installation.
    if([IO.Path]::GetFullPath($install) -ine (Join-Path $run 'installed app') -or (Get-Item -LiteralPath $productKey).GetValue('') -ine $install){throw 'Test installation ownership changed'}
    $runner=Join-Path $run 'test-uninstall.exe'
    Copy-Item -LiteralPath (Join-Path $install 'uninstall.exe') -Destination $runner
    Native $runner @('/S',"_?=$install")
    if(Test-Path -LiteralPath $uninstallKey){throw 'Uninstall registration remains'}
    if(Test-Path -LiteralPath $productKey){
        if((Get-Item -LiteralPath $productKey).GetValue('') -ine $install){throw 'Product registration owner changed'}
        Remove-Item -LiteralPath $productKey -Recurse
    }
    $result=[ordered]@{status='passed';run=$run;actualInstaller=$true;actualUpdater=$true;actualRollback=$true;actualCommit=$true;realReservationArchiveSharingFailure=$true;terminalFinishDidNotReplayCommit=$true;syntheticIncompletePreparationAbandoned=$true;missingReceiptExplicitGuardReadback=$true;projectUnchanged=$true;developmentHelperSha256=$helperDigest;sourceVersion='0.1.5';targetVersion='0.1.6';shippedReleaseAccepted=$false;modelCalls=0;userDesktopActions=0;cleanup='passed';calls=$calls;nativeProcesses=$nativeProcesses}
}catch{
    $result=[ordered]@{status='failed';run=$run;message=$_.Exception.Message;scriptStack=$_.ScriptStackTrace;calls=$calls;nativeProcesses=$nativeProcesses;pendingPid=if($pending){$pending.Id}else{$null};cleanup='preserved-for-review';coreStarted=$coreStarted;transaction=$transaction;planDigest=$digest}
    throw
}finally{
    if($guardPin){$guardPin.Dispose()}
    if($result){[IO.File]::WriteAllText((Join-Path $run 'result.json'),($result|ConvertTo-Json -Depth 30),[Text.UTF8Encoding]::new($false));$result|ConvertTo-Json -Depth 30 -Compress}
}
