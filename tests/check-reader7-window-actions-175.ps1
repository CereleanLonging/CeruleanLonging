param([ValidateSet(1,2)][int]$Schema=1)
$OutputEncoding=[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$run=Join-Path $root ('reports\count-resolution-175\window-actions-'+$Schema)
if(Test-Path -LiteralPath (Join-Path $run 'started.json')){throw 'Preserve original window action run'}
if(Test-Path -LiteralPath $run){if(@(Get-ChildItem -LiteralPath $run -Recurse -File).Count){throw 'Unexpected preexisting action evidence'}}else{$null=New-Item -ItemType Directory -Path $run}
$allCases=Get-Content -LiteralPath (Join-Path $root 'reports\count-resolution-175\fixtures\cases.json') -Raw -Encoding UTF8|ConvertFrom-Json
$fixture=@($allCases|Where-Object {$_.mode -eq 'window' -and $_.schema -eq $Schema})[0]
$fixture|Add-Member -NotePropertyName install -NotePropertyValue (Join-Path $run 'install')
if(-not(Test-Path -LiteralPath $fixture.install)){$null=New-Item -ItemType Directory -Path $fixture.install}
$stage=Join-Path $run 'maintenance';$null=New-Item -ItemType Directory -Path $stage
$data=$fixture.data.Replace('\\?\','')
$config=@{schema=1;ownerSid=[Security.Principal.WindowsIdentity]::GetCurrent().User.Value;directory=$stage;version='0.1.10';installationDirectory=$fixture.install;dataDirectory=$data;artifacts=@()}
foreach($name in @('manage-windows-update.ps1','test-windows-package.ps1','maintenance-window.ps1','cerulean-runtime.exe')){
    $from=if($name -eq 'cerulean-runtime.exe'){Join-Path $root 'reports\count-resolution-175\runtime\cerulean-runtime.exe'}else{Join-Path $root ('scripts\'+$name)}
    $to=Join-Path $stage $name;Copy-Item -LiteralPath $from -Destination $to
    $config.artifacts+=@{path=$name;bytes=(Get-Item -LiteralPath $to).Length;sha256=(Get-FileHash -LiteralPath $to).Hash.ToLowerInvariant()}
}
$session=Join-Path $stage 'session.json'
[IO.File]::WriteAllText($session,($config|ConvertTo-Json -Depth 8),[Text.UTF8Encoding]::new($false))
$sessionDigest=(Get-FileHash -LiteralPath $session).Hash.ToLowerInvariant()
$hook=@'
    # Test-only hook: original controls, StartAction and timer callback; no ShowDialog.
    function InvokeOwnedAction([string]$action,[bool]$confirmed=$false){
        $consent.Checked=$confirmed
        StartAction $action
        if(-not $script:operation){throw "Action did not launch: $action / $($status.Text)"}
        $id=$script:operation.id
        $deadline=[DateTime]::UtcNow.AddSeconds(25)
        while($script:operation -and [DateTime]::UtcNow -lt $deadline){[Windows.Forms.Application]::DoEvents();Start-Sleep -Milliseconds 20}
        if($script:operation){throw 'Original child unresolved; no retry'}
        $done=Get-Content -LiteralPath (Join-Path $directory ('operations\'+$id+'.finished.json')) -Raw -Encoding UTF8|ConvertFrom-Json
        if($done.exit -ne 0){throw "Original action failed: $action"}
        return ($output.Text|ConvertFrom-Json)
    }
    function RefuseOwnedAction([string]$action,[string]$expected){
        $prior=@(Get-ChildItem -LiteralPath (Join-Path $directory 'operations') -Filter '*.request.json').Count
        $consent.Checked=$true;StartAction $action
        if($script:operation -or -not $status.Text.Contains($expected)){throw "Wrong local refusal: $($status.Text)"}
        if(@(Get-ChildItem -LiteralPath (Join-Path $directory 'operations') -Filter '*.request.json').Count -ne $prior){throw 'Refused action created request'}
    }
    $hold=(InvokeOwnedAction 'coreStatus').result.holdDigest
    $backup.Text=Join-Path (Split-Path -Parent $directory) 'cold-backup'
    $manifest=(InvokeOwnedAction 'dataBackup' $true).result.manifestDigest
    $review=(InvokeOwnedAction 'countResolutionReview').result
    $countResolutionDigest.Text=('0'*64)
    RefuseOwnedAction 'countResolutionPromote' '请先审阅'
    $countResolutionDigest.Text=$review.digest
    # An edited input must invalidate the prior review, even after restoring its text.
    $backupDigest.Text=('0'*64);$backupDigest.Text=$manifest
    RefuseOwnedAction 'countResolutionPromote' '请先审阅'
    $review=(InvokeOwnedAction 'countResolutionReview').result
    $promoted=(InvokeOwnedAction 'countResolutionPromote' $true).result
    if($promoted.minimumReaderGeneration -ne 7 -or $script:lastCountResolutionReview -or $script:lastHold -or $script:lastFormatStatus -or $countResolutionDigest.Text){throw 'Promotion result or review invalidation differs'}
    RefuseOwnedAction 'countResolutionPromote' '请先读取有效核心锁定'
    $observed=(InvokeOwnedAction 'formatStatus').result
    if($observed.minimumReaderGeneration -ne 7 -or -not (Test-Path -LiteralPath (Join-Path $data.Text 'upgrade-hold.json'))){throw 'Readback differs or hold auto-released'}
    $result=@{status='passed';actions=6;localRefusals=3;reviewDigest=$review.digest;manifestDigest=$manifest;holdDigest=$hold;format=$observed;holdAutoReleased=$false;modelCalls=0;installed=$false;limits='Controlled hidden WinForms hook invokes unchanged production StartAction and timer against real isolated core; not an installed React entry, UIAutomation click or visible user session'}
    [IO.File]::WriteAllText((Join-Path (Split-Path -Parent $directory) 'result.json'),($result|ConvertTo-Json -Depth 12),[Text.UTF8Encoding]::new($false))
    $form.Dispose()
    return
'@
$product=[IO.File]::ReadAllText((Join-Path $stage 'maintenance-window.ps1'),[Text.Encoding]::UTF8)
$needle='    if($Describe){'
if(([regex]::Matches($product,[regex]::Escape($needle))).Count -ne 2){throw 'Hook anchor changed'}
$harness=Join-Path $stage 'controlled-window.ps1'
[IO.File]::WriteAllText($harness,$product.Insert($product.IndexOf($needle),$hook+"`n"),[Text.UTF8Encoding]::new($true))
[IO.File]::WriteAllText((Join-Path $run 'started.json'),(@{sourceHash=(Get-FileHash -LiteralPath (Join-Path $stage 'maintenance-window.ps1')).Hash;harnessHash=(Get-FileHash -LiteralPath $harness).Hash;fixture=$fixture}|ConvertTo-Json -Depth 8),[Text.UTF8Encoding]::new($false))
& ([scriptblock]::Create([IO.File]::ReadAllText($harness).Replace('$'+'PSScriptRoot',("'"+$stage.Replace("'","''")+"'")))) -Session $session -SessionDigest $sessionDigest -DataMaintenance -Describe
Get-Content -LiteralPath (Join-Path $run 'result.json') -Raw -Encoding UTF8
