$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$run=Join-Path $root ('reports\reader4-update-gates-167-'+[Guid]::NewGuid().ToString('N'))
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
$runtime=Join-Path $run 'reader4';$null=New-Item -ItemType Directory -Path $runtime
Copy-Item -LiteralPath (Join-Path $root 'target\debug\cerulean-core.exe') -Destination (Join-Path $runtime 'cerulean-runtime.exe')
$info=Native (Join-Path $runtime 'cerulean-runtime.exe') @('--build-info')
if($info.dataReaderGeneration -ne 4){throw 'Wrong current binary'}
$four=[pscustomobject]@{packageDirectory=$runtime;capabilities=$info;version=$info.version}
$one=Package (Join-Path $root 'reports\p6-windows-package-014-20261012')
$two=Package (Join-Path $root 'reports\p6-windows-package-018-20261012')
$three=Package (Join-Path $root 'reports\p6-windows-package-019-20261013')
# Reproduce the pre-fix production controller's rejection without executing it.
& {
    . (Definition (Join-Path $root 'reports\reader4-maintenance-167\before\scripts\manage-windows-update.ps1') 'ReaderGeneration')
    $failure='';try{$null=ReaderGeneration $four}catch{$failure=$_.Exception.Message}
    if(-not $failure.Contains('Unsupported or inconsistent')){throw 'Original reader4 rejection not reproduced'}
    $checks.Add(@{name='original controller rejects actual reader4 build capabilities';passed=$true;rejection=$failure})
}
$cases=Get-Content -LiteralPath (Join-Path $root 'reports\currency-reader-fixtures-130\cases.json') -Raw -Encoding UTF8|ConvertFrom-Json
$before=@{}
foreach($case in $cases){
    foreach($file in Get-ChildItem -LiteralPath $case.data -Recurse -File){$before[$file.FullName]=(Get-FileHash -LiteralPath $file.FullName).Hash}
    Check "schema $($case.schema) reader4 supported" $four $four $case.data
    foreach($old in @($one,$two,$three)){
        Check "reader4 rejects old source $($old.version)" $old $four $case.data 'cannot both read generation 4'
        Check "reader4 rejects old target $($old.version)" $four $old $case.data 'cannot both read generation 4'
    }
    foreach($reply in @(@{dataSchema=$case.schema;minimumReaderGeneration=3;readerGuardDigest=('a'*64)},@{dataSchema=$case.schema;minimumReaderGeneration='4';readerGuardDigest=('a'*64)},@{dataSchema=$case.schema;minimumReaderGeneration=4;readerGuardDigest='bad'})){
        $script:mockFormat=[pscustomobject]$reply
        Check 'invalid guard status cannot authorize update' $four $four $case.data 'declaration is missing or inconsistent'
    }
    $script:mockFormat=$null
}
foreach($entry in $before.GetEnumerator()){if((Get-FileHash -LiteralPath $entry.Key).Hash -cne $entry.Value){throw 'Original fixture changed'}}
$legacy=Get-Content -LiteralPath (Join-Path $root 'reports\recovery-holds-122-cli-cases\cases.json') -Raw -Encoding UTF8|ConvertFrom-Json
foreach($case in @($legacy|Where-Object {$_.mode -eq 'restore'})){
    $data=$case.data;if($data.StartsWith('\\?\')){$data=$data.Substring(4)}
    Check "legacy schema $($case.schema) to reader4" $two $four $data
    Check "legacy schema $($case.schema) to unchanged reader3" $two $three $data
}
$plain=Join-Path $run 'controlled';$events=Join-Path $plain 'events';$null=New-Item -ItemType Directory -Path $events
$script:mockFormat=[pscustomobject]@{dataSchema=1;minimumReaderGeneration=1}
foreach($generation in @(5,'4',$null)){
    $bad=Clone $four;$bad.capabilities.dataReaderGeneration=$generation
    Check 'unknown, text or absent reader not inferred' $four $bad $plain $(if($null -eq $generation){'Reader generation missing'}else{'Unsupported or inconsistent'})
}
$script:mockFormat=$null
[IO.File]::WriteAllText((Join-Path $events '00000000.reader5.json'),'{}')
Check 'future guard refuses legacy shortcut' $one $one $plain 'Unknown reader guard'
[IO.File]::WriteAllText((Join-Path $events '.format-migration-currency.pending'),'partial')
Check 'partial promotion refuses update' $four $four $plain 'Data migration is incomplete'
$result=@{status='passed';checks=$checks;actualCoreProbes=$probes.Count;priorFixtureFilesChecked=$before.Count;runtimeSha256=(Get-FileHash -LiteralPath (Join-Path $runtime 'cerulean-runtime.exe')).Hash.ToLowerInvariant();programReplacementTested=$false;installed=$false;modelCalls=0;limits='Production gate functions with real read-only status probes and controlled malformed replies; not full installer or update transaction acceptance'}
[IO.File]::WriteAllText((Join-Path $run 'result.json'),($result|ConvertTo-Json -Depth 12),[Text.UTF8Encoding]::new($false))
@{status='passed';checks=$checks.Count;actualCoreProbes=$probes.Count;run=$run}|ConvertTo-Json -Compress
