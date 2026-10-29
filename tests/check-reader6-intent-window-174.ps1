$OutputEncoding=[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$run=Join-Path $root 'reports\reader6-maintenance-174\intent-window'
if(Test-Path -LiteralPath (Join-Path $run 'started.json')){throw 'Preserve original window action run'}
if(Test-Path -LiteralPath $run){if(@(Get-ChildItem -LiteralPath $run -Recurse -File).Count){throw 'Unexpected preexisting action evidence'}}else{$null=New-Item -ItemType Directory -Path $run}
$allCases=Get-Content -LiteralPath (Join-Path $root 'reports\reader6-maintenance-174\fixtures\cases.json') -Raw -Encoding UTF8|ConvertFrom-Json
$fixture=@($allCases|Where-Object {$_.mode -eq 'window'})[0]
$fixture|Add-Member -NotePropertyName install -NotePropertyValue (Join-Path $run 'install')
if(-not(Test-Path -LiteralPath $fixture.install)){$null=New-Item -ItemType Directory -Path $fixture.install}
$stage=Join-Path $run 'maintenance';$null=New-Item -ItemType Directory -Path $stage
$data=$fixture.data.Replace('\\?\','')
$config=@{schema=1;ownerSid=[Security.Principal.WindowsIdentity]::GetCurrent().User.Value;directory=$stage;version='0.1.10';installationDirectory=$fixture.install;dataDirectory=$data;artifacts=@()}
foreach($name in @('manage-windows-update.ps1','test-windows-package.ps1','maintenance-window.ps1','cerulean-runtime.exe')){
    $from=if($name -eq 'cerulean-runtime.exe'){Join-Path $root 'reports\reader6-maintenance-174\runtime\cerulean-runtime.exe'}else{Join-Path $root ('scripts\'+$name)}
    $to=Join-Path $stage $name;Copy-Item -LiteralPath $from -Destination $to
    $config.artifacts+=@{path=$name;bytes=(Get-Item -LiteralPath $to).Length;sha256=(Get-FileHash -LiteralPath $to).Hash.ToLowerInvariant()}
}
$session=Join-Path $stage 'session.json'
[IO.File]::WriteAllText($session,($config|ConvertTo-Json -Depth 8),[Text.UTF8Encoding]::new($false))
$sessionDigest=(Get-FileHash -LiteralPath $session).Hash.ToLowerInvariant()
$hook=@'
    $rows=[Collections.Generic.List[object]]::new()
    $oldTokens=$null;$oldErrors=$null
    $oldPath=Join-Path (Split-Path -Parent (Split-Path -Parent $directory)) 'baseline\scripts\maintenance-window.ps1'
    $oldAst=[Management.Automation.Language.Parser]::ParseFile($oldPath,[ref]$oldTokens,[ref]$oldErrors)
    $oldCondition=$oldAst.Find({param($n) $n -is [Management.Automation.Language.IfStatementAst] -and $n.Extent.Text.StartsWith('if($value.schema -notin')},$true)
    if(-not $oldCondition -or $oldErrors.Count){throw 'Original window condition missing'}
    foreach($generation in @(3,4,5,6,'5','6',7)){
        $case=Join-Path (Split-Path -Parent $directory) ('intent-'+$rows.Count)
        $null=New-Item -ItemType Directory -Path $case
        $transaction.Text=$case
        $value=@{schema=2;controllerReaderGeneration=$generation;ownerSid=$config.ownerSid;transaction=$case;data=$data.Text;install=$install.Text}
        $path=Join-Path $case 'intent.json'
        [IO.File]::WriteAllText($path,($value|ConvertTo-Json),[Text.UTF8Encoding]::new($false))
        $hash=Digest $path
        $oldAccepted=$true
        try{& ([scriptblock]::Create($oldCondition.Extent.Text))}catch{$oldAccepted=$false}
        $expected=($generation -is [int] -and $generation -in @(3,4,5,6))
        StartAction 'intentInfo'
        $accepted=($intentDigest.Text -ceq $hash -and $status.Text.StartsWith('已读取原准备意图'))
        if($accepted -ne $expected -or $script:operation){throw "Wrong intent-info result: $generation / $($status.Text)"}
        if($oldAccepted -ne ($generation -is [int] -and $generation -in @(3,4,5))){throw 'Original reader6 rejection not reproduced'}
        if((Digest $path) -cne $hash){throw 'Original intent changed'}
        $rows.Add(@{generation=$generation;accepted=$accepted;originalAccepted=$oldAccepted;unchanged=$true})
    }
    if(Test-Path -LiteralPath (Join-Path $directory 'operations')){throw 'Intent read started a process'}
    $result=@{status='passed';cases=$rows;productChildProcesses=0;controlledHiddenWindow=$true;originalReader6RejectionReproduced=$true}
    [IO.File]::WriteAllText((Join-Path (Split-Path -Parent $directory) 'result.json'),($result|ConvertTo-Json -Depth 10),[Text.UTF8Encoding]::new($false))
    $form.Dispose();return
'@
$product=[IO.File]::ReadAllText((Join-Path $stage 'maintenance-window.ps1'),[Text.Encoding]::UTF8)
$needle='    if($Describe){'
if(([regex]::Matches($product,[regex]::Escape($needle))).Count -ne 2){throw 'Hook anchor changed'}
$harness=Join-Path $stage 'controlled-window.ps1'
[IO.File]::WriteAllText($harness,$product.Insert($product.IndexOf($needle),$hook+"`n"),[Text.UTF8Encoding]::new($true))
[IO.File]::WriteAllText((Join-Path $run 'started.json'),(@{sourceHash=(Get-FileHash -LiteralPath (Join-Path $stage 'maintenance-window.ps1')).Hash;harnessHash=(Get-FileHash -LiteralPath $harness).Hash;fixture=$fixture}|ConvertTo-Json -Depth 8),[Text.UTF8Encoding]::new($false))
& ([scriptblock]::Create([IO.File]::ReadAllText($harness).Replace('$'+'PSScriptRoot',("'"+$stage.Replace("'","''")+"'")))) -Session $session -SessionDigest $sessionDigest -Describe
Get-Content -LiteralPath (Join-Path $run 'result.json') -Raw -Encoding UTF8
