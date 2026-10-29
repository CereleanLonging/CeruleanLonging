$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$run=Join-Path $root ('reports\data-format-update-gate-'+[Guid]::NewGuid().ToString('N'))
$data=Join-Path $run 'data';$events=Join-Path $data 'events'
$null=New-Item -ItemType Directory -Path $events
$script:plan=[pscustomobject]@{data=$data}
$package=Join-Path $root 'reports\p6-windows-package-015-20261012'
$legacy=Join-Path $root 'reports\p6-windows-package-014-20261012'
$target=[pscustomobject]@{packageDirectory=$package;capabilities=(Get-Content -LiteralPath (Join-Path $package 'manifest.json') -Raw -Encoding UTF8|ConvertFrom-Json).coreBuildInfo}
$old=[pscustomobject]@{packageDirectory=$legacy;capabilities=(Get-Content -LiteralPath (Join-Path $legacy 'manifest.json') -Raw -Encoding UTF8|ConvertFrom-Json).coreBuildInfo}
$tokens=$null;$parseErrors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot 'manage-windows-update.ps1'),[ref]$tokens,[ref]$parseErrors)
if($parseErrors.Count){throw 'Updater syntax invalid'}
$definition=$ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'CheckDataFormat'},$true)
if(-not $definition){throw 'Updater format gate missing'}
. ([scriptblock]::Create($definition.Extent.Text))
$checks=[Collections.Generic.List[object]]::new();$probes=0
function Core([string]$file,[string[]]$arguments){
    $oldPreference=$ErrorActionPreference;$ErrorActionPreference='Continue'
    try{$raw=& $file --workbench --data-dir $data @arguments 2>&1;$code=$LASTEXITCODE}finally{$ErrorActionPreference=$oldPreference}
    $script:probes++
    [IO.File]::WriteAllText((Join-Path $run ('probe-'+$script:probes+'.txt')),($raw -join "`n"),[Text.UTF8Encoding]::new($false))
    if($code -ne 0){throw 'Actual core rejected format probe'}
    return (($raw -join "`n")|ConvertFrom-Json)
}
function Check([string]$name,$source,$candidate,[string]$expected=''){
    $errorText='';try{CheckDataFormat $source $candidate}catch{$errorText=$_.Exception.Message}
    if($expected){if(-not $errorText.Contains($expected)){throw "$name got unexpected result: $errorText"}}
    elseif($errorText){throw "$name failed: $errorText"}
    $checks.Add(@{name=$name;passed=$true;rejection=$errorText})
}
Check 'legacy format retains legacy program update path' $old $old
$partial=Join-Path $events '.format-migration-test.pending'
[IO.File]::WriteAllText($partial,'partial')
Check 'incomplete migration prevents program update' $target $target 'Data migration is incomplete'
Remove-Item -LiteralPath $partial
# Controlled marker fixture copied from the real migration, rebound only to this
# isolated test directory; this matrix is not a second migration/crash claim.
$native=Get-Content -LiteralPath (Join-Path $root 'reports\p6-data-format-native1-20261012.log') -Raw -Encoding UTF8|ConvertFrom-Json
$marker=Get-Content -LiteralPath (Join-Path $native.run 'preserved-migrated-data\events\00000000.json') -Raw -Encoding UTF8|ConvertFrom-Json
$marker.root='\\?\'+$data
$markerPath=Join-Path $events '00000000.json'
[IO.File]::WriteAllText($markerPath,($marker|ConvertTo-Json),[Text.UTF8Encoding]::new($false))
Check 'legacy candidate cannot validate schema two' $target $old 'Candidate cannot validate'
Check 'rollback source must also read schema two' $old $target 'Source/candidate cannot both read'
Check 'both real supported capabilities accept schema two' $target $target
$marker.dataSchema=99
[IO.File]::WriteAllText($markerPath,($marker|ConvertTo-Json),[Text.UTF8Encoding]::new($false))
Check 'actual new core rejects future data schema' $target $target 'Actual core rejected'
$result=@{status='passed';run=$run;checks=$checks;passed=$checks.Count;actualCoreProbes=$probes;controlledMarkerFixture=$true;programReplacementTested=$false;realModelCalls=0;userDesktopActions=0}
[IO.File]::WriteAllText((Join-Path $run 'result.json'),($result|ConvertTo-Json -Depth 12),[Text.UTF8Encoding]::new($false))
$result|ConvertTo-Json -Depth 12 -Compress
