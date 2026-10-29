$OutputEncoding=[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$run=Join-Path $root ('reports\reader9-update-gates-179-'+[Guid]::NewGuid().ToString('N'))
$null=New-Item -ItemType Directory -Path $run
$checks=[Collections.Generic.List[object]]::new();$probes=[Collections.Generic.List[object]]::new()
function Definition([string]$file,[string]$name){
    $tokens=$null;$errors=$null
    $ast=[Management.Automation.Language.Parser]::ParseFile($file,[ref]$tokens,[ref]$errors)
    if($errors.Count){throw "PowerShell syntax failure: $file"}
    $node=$ast.Find({param($n) $n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq $name},$true)
    if(-not $node){throw "Missing product function: $name"}
    return [scriptblock]::Create($node.Extent.Text)
}
. (Definition (Join-Path $root 'scripts\manage-windows-update.ps1') 'ReaderGeneration')
. (Definition (Join-Path $root 'scripts\manage-windows-update.ps1') 'CheckDataFormat')
. (Definition (Join-Path $root 'scripts\maintenance-window.ps1') 'QuoteArgument')
function Native([string]$binary,[string[]]$arguments){
    $start=[Diagnostics.ProcessStartInfo]::new();$start.FileName=$binary
    $start.Arguments=($arguments|ForEach-Object {QuoteArgument $_}) -join ' '
    $start.UseShellExecute=$false;$start.CreateNoWindow=$true;$start.RedirectStandardOutput=$true;$start.RedirectStandardError=$true
    $start.StandardOutputEncoding=[Text.UTF8Encoding]::new($false);$start.StandardErrorEncoding=[Text.UTF8Encoding]::new($false)
    $child=[Diagnostics.Process]::new();$child.StartInfo=$start
    if(-not $child.Start()){throw 'Core child did not start'}
    $out=$child.StandardOutput.ReadToEndAsync();$err=$child.StandardError.ReadToEndAsync()
    if(-not $child.WaitForExit(20000)){throw 'Child unresolved; no automatic retry'}
    $stdout=$out.GetAwaiter().GetResult();$stderr=$err.GetAwaiter().GetResult()
    $record=@{pid=$child.Id;exit=$child.ExitCode;binary=$binary;arguments=$arguments;waited=$true;stdoutEOF=$true;stderrEOF=$true;stdout=$stdout;stderr=$stderr}
    $probes.Add($record);$child.Dispose()
    [IO.File]::WriteAllText((Join-Path $run ('probe-{0:D3}.json' -f $probes.Count)),($record|ConvertTo-Json -Depth 8),[Text.UTF8Encoding]::new($false))
    if($record.exit -ne 0){throw 'Actual core rejected format probe'}
    return ($stdout|ConvertFrom-Json)
}
$script:mockFormat=$null
function Core([string]$binary,[string[]]$arguments){
    if($null -ne $script:mockFormat){return [pscustomobject]@{result=$script:mockFormat}}
    return Native $binary (@('--workbench','--data-dir',$script:plan.data)+$arguments)
}
function Package([string]$directory){
    $manifest=Get-Content -LiteralPath (Join-Path $directory 'manifest.json') -Raw -Encoding UTF8|ConvertFrom-Json
    return [pscustomobject]@{packageDirectory=$directory;capabilities=$manifest.coreBuildInfo;version=$manifest.version}
}
function Clone($value){return ($value|ConvertTo-Json -Depth 15 -Compress|ConvertFrom-Json)}
function Check([string]$name,$source,$target,[string]$data,[string]$expected=''){
    $script:plan=[pscustomobject]@{data=$data};$before=$probes.Count;$errorText=''
    try{CheckDataFormat $source $target}catch{$errorText=$_.Exception.Message}
    if($expected){if(-not $errorText.Contains($expected)){throw "$name unexpected rejection: $errorText"}}
    elseif($errorText){throw "$name failed: $errorText"}
    $checks.Add(@{name=$name;passed=$true;rejection=$errorText;actualCoreProbes=$probes.Count-$before;controlledReply=($null -ne $script:mockFormat)})
}

$runtime=Join-Path $root 'reports\scoped-count-ledger-179\runtime'
$info=Native (Join-Path $runtime 'cerulean-runtime.exe') @('--build-info')
if($info.dataReaderGeneration -ne 9){throw 'Wrong current binary'}
$nine=[pscustomobject]@{packageDirectory=$runtime;capabilities=$info;version=$info.version}
$prior=Join-Path $root 'reports\scoped-ledger-177\runtime'
$priorInfo=Native (Join-Path $prior 'cerulean-runtime.exe') @('--build-info')
if($priorInfo.dataReaderGeneration -ne 8){throw 'Wrong prior binary'}
$eight=[pscustomobject]@{packageDirectory=$prior;capabilities=$priorInfo;version=$priorInfo.version}
$four=Package (Join-Path $root 'reports\p6-windows-package-0110-20261015')
# Exercise the production declaration decoder across every supported explicit generation.
foreach($generation in @(3,4,5,6,7,8,9)){
 $declared=Clone $nine;$declared.capabilities.dataReaderGeneration=$generation
 if((ReaderGeneration $declared) -ne $generation){throw 'Supported numeric reader rejected'}
 $checks.Add(@{name=('numeric reader '+$generation+' declaration accepted');passed=$true})
}

& {
 . (Definition (Join-Path $root 'reports\scoped-count-ledger-179\baseline\scripts\manage-windows-update.ps1') 'ReaderGeneration')
 $failure='';try{$null=ReaderGeneration $nine}catch{$failure=$_.Exception.Message}
 if(-not $failure.Contains('Unsupported or inconsistent')){throw 'Original reader9 rejection not reproduced'}
 $checks.Add(@{name='original controller refuses reader9 declaration';passed=$true})
}
$cases=Get-Content -LiteralPath (Join-Path $root 'reports\scoped-count-ledger-179\fixtures\cases.json') -Raw -Encoding UTF8|ConvertFrom-Json
$before=@{};foreach($case in $cases){Get-ChildItem -LiteralPath $case.data -Recurse -File|ForEach-Object {$before[$_.FullName]=(Get-FileHash -LiteralPath $_.FullName).Hash}}
foreach($case in @($cases|Where-Object {$_.mode -eq 'cli'})){
 Check "schema $($case.schema) reader9 supported" $nine $nine $case.data
 foreach($old in @($four,$eight)){
  Check 'old source refuses reader9 data' $old $nine $case.data 'cannot both read generation 9'
  Check 'old target refuses reader9 data' $nine $old $case.data 'cannot both read generation 9'
 }
 foreach($reply in @(@{dataSchema=$case.schema;minimumReaderGeneration=8;readerGuardDigest=('a'*64)},@{dataSchema=$case.schema;minimumReaderGeneration='9';readerGuardDigest=('a'*64)},@{dataSchema=$case.schema;minimumReaderGeneration=9;readerGuardDigest='bad'})){
  $script:mockFormat=[pscustomobject]$reply
  Check 'inconsistent typed format reply refused' $nine $nine $case.data 'declaration is missing or inconsistent'
 }
 $script:mockFormat=$null
}
foreach($case in @($cases|Where-Object {$_.mode -eq 'window'})){
 Check 'reader8 data remains compatible before promotion' $eight $nine $case.data
}
$plain=Join-Path $run 'controlled';$events=Join-Path $plain 'events';$null=New-Item -ItemType Directory -Path $events
$script:mockFormat=[pscustomobject]@{dataSchema=1;minimumReaderGeneration=1}
foreach($generation in @(10,'9',$null)){
 $bad=Clone $nine;$bad.capabilities.dataReaderGeneration=$generation
 Check 'unknown text or missing capability refused' $nine $bad $plain $(if($null -eq $generation){'Reader generation missing'}else{'Unsupported or inconsistent'})
}
$script:mockFormat=$null
[IO.File]::WriteAllText((Join-Path $events '00000000.reader9.json'),'{}')
Check 'reader9 without original reader8 fails before probe' $nine $nine $plain 'original reader8 guard is missing'
[IO.File]::WriteAllText((Join-Path $events '00000000.reader10.json'),'{}')
Check 'future guard refuses shortcuts' $eight $nine $plain 'Unknown reader guard'
[IO.File]::WriteAllText((Join-Path $events '.format-migration-scoped-count.pending'),'partial')
Check 'pending promotion refuses update' $nine $nine $plain 'Data migration is incomplete'
foreach($entry in $before.GetEnumerator()){if((Get-FileHash -LiteralPath $entry.Key).Hash -cne $entry.Value){throw 'Original fixture changed'}}
$result=@{status='passed';checks=$checks;actualCoreProbes=$probes.Count;priorFixtureFilesChecked=$before.Count;runtimeSha256=(Get-FileHash -LiteralPath (Join-Path $runtime 'cerulean-runtime.exe')).Hash.ToLowerInvariant();programReplacementTested=$false;installed=$false;modelCalls=0}
[IO.File]::WriteAllText((Join-Path $run 'result.json'),($result|ConvertTo-Json -Depth 12),[Text.UTF8Encoding]::new($false))
@{status='passed';checks=$checks.Count;actualCoreProbes=$probes.Count;run=$run}|ConvertTo-Json -Compress
