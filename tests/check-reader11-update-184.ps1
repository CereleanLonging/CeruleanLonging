$OutputEncoding=[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$run=Join-Path $root ('reports\api-currency-184\controller-'+[Guid]::NewGuid().ToString('N'))
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

$runtime=Join-Path $root 'reports\api-currency-184\runtime'
$info=Native (Join-Path $runtime 'cerulean-runtime.exe') @('--build-info')
if($info.dataReaderGeneration -ne 11){throw 'Wrong current binary'}
$eleven=[pscustomobject]@{packageDirectory=$runtime;capabilities=$info;version=$info.version}
$prior=Join-Path $root 'reports\api-retirement-183\runtime'
$priorInfo=Native (Join-Path $prior 'cerulean-runtime.exe') @('--build-info')
if($priorInfo.dataReaderGeneration -ne 10){throw 'Wrong prior binary'}
$ten=[pscustomobject]@{packageDirectory=$prior;capabilities=$priorInfo;version=$priorInfo.version}
$four=Package (Join-Path $root 'reports\p6-windows-package-0110-20261015')
# Exercise the production declaration decoder across every supported explicit generation.
foreach($generation in @(3,4,5,6,7,8,9,10,11)){
 $declared=Clone $eleven;$declared.capabilities.dataReaderGeneration=$generation
 if((ReaderGeneration $declared) -ne $generation){throw 'Supported numeric reader rejected'}
 $checks.Add(@{name=('numeric reader '+$generation+' declaration accepted');passed=$true})
}

& {
 . (Definition (Join-Path $root 'reports\api-currency-184\baseline\scripts\manage-windows-update.ps1') 'ReaderGeneration')
 $failure='';try{$null=ReaderGeneration $eleven}catch{$failure=$_.Exception.Message}
 if(-not $failure.Contains('Unsupported or inconsistent')){throw 'Original reader11 rejection not reproduced'}
 $checks.Add(@{name='original controller refuses reader11 declaration';passed=$true})
}
$cases=Get-Content -LiteralPath (Join-Path $root 'reports\api-currency-184\fixtures\cases.json') -Raw -Encoding UTF8|ConvertFrom-Json
$before=@{};foreach($case in $cases){Get-ChildItem -LiteralPath $case.data -Recurse -File|ForEach-Object {$before[$_.FullName]=(Get-FileHash -LiteralPath $_.FullName).Hash}}
foreach($case in @($cases|Where-Object {$_.mode -eq 'cli'})){
 Check "schema $($case.schema) reader11 supported" $eleven $eleven $case.data
 foreach($old in @($four,$ten)){
  Check 'old source refuses reader11 data' $old $eleven $case.data $(if((ReaderGeneration $old) -lt 9){'cannot both read generation 9'}else{'cannot both read generation 11'})
  Check 'old target refuses reader11 data' $eleven $old $case.data $(if((ReaderGeneration $old) -lt 9){'cannot both read generation 9'}else{'cannot both read generation 11'})
 }
 foreach($reply in @(@{dataSchema=$case.schema;minimumReaderGeneration=10;readerGuardDigest=('a'*64)},@{dataSchema=$case.schema;minimumReaderGeneration='11';readerGuardDigest=('a'*64)},@{dataSchema=$case.schema;minimumReaderGeneration=11;readerGuardDigest='bad'})){
  $script:mockFormat=[pscustomobject]$reply
  Check 'inconsistent typed format reply refused' $eleven $eleven $case.data 'declaration is missing or inconsistent'
 }
 $script:mockFormat=$null
}
foreach($case in @($cases|Where-Object {$_.mode -eq 'window'})){
 Check 'reader10 data remains compatible before promotion' $ten $eleven $case.data
}
$plain=Join-Path $run 'controlled';$events=Join-Path $plain 'events';$null=New-Item -ItemType Directory -Path $events
$script:mockFormat=[pscustomobject]@{dataSchema=1;minimumReaderGeneration=1}
foreach($generation in @(12,'11',11.5,$null)){
 $bad=Clone $eleven;$bad.capabilities.dataReaderGeneration=$generation
 Check 'unknown text or missing capability refused' $eleven $bad $plain $(if($null -eq $generation){'Reader generation missing'}else{'Unsupported or inconsistent'})
}
$script:mockFormat=$null
[IO.File]::WriteAllText((Join-Path $events '00000000.reader11.json'),'{}')
Check 'reader11 without original reader10 fails before probe' $eleven $eleven $plain 'original reader10 guard is missing'
[IO.File]::WriteAllText((Join-Path $events '00000000.reader12.json'),'{}')
Check 'future guard refuses shortcuts' $ten $eleven $plain 'Unknown reader guard'
[IO.File]::WriteAllText((Join-Path $events '.format-migration-api-currency.pending'),'partial')
Check 'pending promotion refuses update' $eleven $eleven $plain 'Data migration is incomplete'
foreach($entry in $before.GetEnumerator()){if((Get-FileHash -LiteralPath $entry.Key).Hash -cne $entry.Value){throw 'Original fixture changed'}}
$result=@{status='passed';checks=$checks;actualCoreProbes=$probes.Count;priorFixtureFilesChecked=$before.Count;runtimeSha256=(Get-FileHash -LiteralPath (Join-Path $runtime 'cerulean-runtime.exe')).Hash.ToLowerInvariant();programReplacementTested=$false;installed=$false;modelCalls=0}
[IO.File]::WriteAllText((Join-Path $run 'result.json'),($result|ConvertTo-Json -Depth 12),[Text.UTF8Encoding]::new($false))
@{status='passed';checks=$checks.Count;actualCoreProbes=$probes.Count;run=$run}|ConvertTo-Json -Compress
