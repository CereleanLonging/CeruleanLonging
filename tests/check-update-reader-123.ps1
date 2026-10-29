$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$run=Join-Path $root 'reports\update-reader-123-gates'
if(Test-Path -LiteralPath $run){throw 'Preserve previous gate evidence'}
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
. (Definition (Join-Path $root 'scripts\test-windows-package.ps1') 'CapabilityKey')
. (Definition (Join-Path $root 'scripts\maintenance-window.ps1') 'QuoteArgument')
function Native([string]$file,[string[]]$arguments){
    $start=[Diagnostics.ProcessStartInfo]::new();$start.FileName=$file
    $start.Arguments=($arguments|ForEach-Object {QuoteArgument $_}) -join ' '
    $start.UseShellExecute=$false;$start.CreateNoWindow=$true;$start.RedirectStandardOutput=$true;$start.RedirectStandardError=$true
    $start.StandardOutputEncoding=[Text.UTF8Encoding]::new($false);$start.StandardErrorEncoding=[Text.UTF8Encoding]::new($false)
    $child=[Diagnostics.Process]::new();$child.StartInfo=$start
    if(-not $child.Start()){throw 'Metadata child did not start'}
    $outTask=$child.StandardOutput.ReadToEndAsync();$errTask=$child.StandardError.ReadToEndAsync()
    $exited=$child.WaitForExit(20000);$outDone=$outTask.Wait(1000);$errDone=$errTask.Wait(1000)
    $record=@{pid=$child.Id;binary=$file;arguments=$arguments;exited=$exited;stdoutEOF=$outDone;stderrEOF=$errDone;exitCode=if($exited){$child.ExitCode}else{$null};stdout=if($outDone){$outTask.Result}else{$null};stderr=if($errDone){$errTask.Result}else{$null}}
    $probes.Add($record)
    [IO.File]::WriteAllText((Join-Path $run ('probe-{0:D3}.json' -f $probes.Count)),($record|ConvertTo-Json -Depth 8),[Text.UTF8Encoding]::new($false))
    if(-not $exited -or -not $outDone -or -not $errDone){throw 'Metadata child or output remains unresolved; no automatic retry'}
    $child.Dispose()
    if($record.exitCode -ne 0){throw 'Actual core rejected format probe'}
    return ($record.stdout|ConvertFrom-Json)
}
$script:mockFormat=$null
function Core([string]$file,[string[]]$arguments){
    if($null -ne $script:mockFormat){return [pscustomobject]@{result=$script:mockFormat}}
    return Native $file (@('--workbench','--data-dir',$script:plan.data)+$arguments)
}
function Package([string]$directory){
    $m=Get-Content -LiteralPath (Join-Path $directory 'manifest.json') -Raw -Encoding UTF8|ConvertFrom-Json
    return [pscustomobject]@{packageDirectory=$directory;capabilities=$m.coreBuildInfo;version=$m.version}
}
function Clone($value){return ($value|ConvertTo-Json -Depth 15 -Compress|ConvertFrom-Json)}
function Check([string]$name,$source,$target,[string]$data,[string]$expected=''){
    $script:plan=[pscustomobject]@{data=$data};$before=$probes.Count;$errorText=''
    try{CheckDataFormat $source $target}catch{$errorText=$_.Exception.Message}
    if($expected){if(-not $errorText.Contains($expected)){throw "$name unexpected rejection: $errorText"}}
    elseif($errorText){throw "$name failed: $errorText"}
    $checks.Add(@{name=$name;passed=$true;rejection=$errorText;actualCoreProbes=$probes.Count-$before;controlledReply=($null -ne $script:mockFormat)})
}
$newDir=Join-Path $run 'reader3-runtime';$null=New-Item -ItemType Directory -Path $newDir
Copy-Item -LiteralPath (Join-Path $root 'target\debug\cerulean-core.exe') -Destination (Join-Path $newDir 'cerulean-runtime.exe')
$newInfo=Native (Join-Path $newDir 'cerulean-runtime.exe') @('--build-info')
$new=[pscustomobject]@{packageDirectory=$newDir;capabilities=$newInfo;version=$newInfo.version}
$one=Package (Join-Path $root 'reports\p6-windows-package-014-20261012')
$two=Package (Join-Path $root 'reports\p6-windows-package-018-20261012')
$oldCases=Get-Content -LiteralPath (Join-Path $root 'reports\recovery-holds-122-cli-cases\cases.json') -Raw -Encoding UTF8|ConvertFrom-Json
$data1=($oldCases|Where-Object {$_.schema -eq 1 -and $_.mode -eq 'restore'}).data
$data2=($oldCases|Where-Object {$_.schema -eq 2 -and $_.mode -eq 'restore'}).data
$readerCases=Get-Content -LiteralPath (Join-Path $root 'reports\reader-generation-121-cli-cases\cases.json') -Raw -Encoding UTF8|ConvertFrom-Json
$data3=($readerCases|Where-Object {$_.schema -eq 1}).data
function PlainPath([string]$path){if($path.StartsWith('\\?\')){return $path.Substring(4)};return $path}
$data1=PlainPath $data1;$data2=PlainPath $data2;$data3=PlainPath $data3
# These are existing successful isolated data directories; all probes are read-only.
Check 'legacy schema1 to legacy reader1' $one $one $data1
Check 'schema1 forward to reader3' $one $new $data1
Check 'schema2 forward to reader3' $two $new $data2
Check 'schema2 genuine legacy reader2 reply' $two $two $data2
Check 'schema2 rejects reader1 rollback' $one $new $data2 'cannot both read the active data format'
Check 'schema1 container with reader3 guard is supported by both new readers' $new $new $data3
Check 'reader3 data rejects older source' $two $new $data3 'cannot both read generation 3'
Check 'reader3 data rejects older target' $new $two $data3 'cannot both read generation 3'
foreach($v in @('0.1.0','0.1.9','99.0.0')){
    $bad=Clone $two;$bad.capabilities.version=$v
    Check "missing reader not inferred for version $v" $bad $new $data1 'Reader generation missing'
}
foreach($v in @(4,'3')){
    $bad=Clone $new;$bad.capabilities.dataReaderGeneration=$v
    Check "future or string-valued explicit reader $v" $new $bad $data1 'Unsupported or inconsistent'
}
$bad=Clone $new;$bad.capabilities.dataReaderGeneration=$null
Check 'new update protocol cannot masquerade as legacy missing reader' $new $bad $data1 'Reader generation missing'
$bad=Clone $new;$bad.capabilities.binaryUpdateSchema=1
Check 'reader3 without new transaction protocol rejected' $new $bad $data1 'Unsupported or inconsistent'
$empty=Join-Path $run 'partial-data';$events=Join-Path $empty 'events';$null=New-Item -ItemType Directory -Path $events
[IO.File]::WriteAllText((Join-Path $events '.format-migration-reader.pending'),'partial')
Check 'pending reader promotion blocks update' $new $new $empty 'Data migration is incomplete'
Remove-Item -LiteralPath (Join-Path $events '.format-migration-reader.pending')
[IO.File]::WriteAllText((Join-Path $events '00000000.reader4.json'),'{}')
Check 'unknown future guard blocks legacy shortcut' $one $one $empty 'Unknown reader guard'
foreach($reply in @(@{dataSchema=1},@{dataSchema=1;minimumReaderGeneration='1'},@{dataSchema=1;minimumReaderGeneration=0},@{dataSchema=1;minimumReaderGeneration=3},@{dataSchema='1';minimumReaderGeneration=1})){
    $script:mockFormat=[pscustomobject]$reply
    Check 'malformed or inconsistent current-core status' $new $new $data1 $(if($reply.dataSchema -is [string]){'cannot both read'}else{'declaration is missing or inconsistent'})
}
$script:mockFormat=$null
$key=CapabilityKey $newInfo
foreach($value in @($null,'3',4)){
    $bad=Clone $newInfo;$bad.dataReaderGeneration=$value
    if((CapabilityKey $bad) -ceq $key){throw 'Typed capability comparison lost reader identity'}
    $checks.Add(@{name='typed manifest capability comparison';passed=$true;alteredReader=$value})
}
# Exercise the complete public package reviewer against a real old runtime.
$packageDir=Join-Path $run 'old-package-review';Copy-Item -LiteralPath $two.packageDirectory -Destination $packageDir -Recurse
$manifestPath=Join-Path $packageDir 'manifest.json'
$m=Get-Content -LiteralPath $manifestPath -Raw -Encoding UTF8|ConvertFrom-Json
$m.coreBuildInfo|Add-Member -NotePropertyName dataReaderGeneration -NotePropertyValue 3
[IO.File]::WriteAllText($manifestPath,($m|ConvertTo-Json -Depth 20),[Text.UTF8Encoding]::new($false))
$rejected=''
try{$null=& (Join-Path $root 'scripts\test-windows-package.ps1') -PackageDirectory $packageDir -ManifestSha256 (Get-FileHash -LiteralPath $manifestPath).Hash -AllowUnsignedInternal}catch{$rejected=$_.Exception.Message}
if(-not $rejected.Contains('Actual runtime capabilities differ from manifest')){throw "Full public reviewer missed forged reader: $rejected"}
$checks.Add(@{name='actual public package reviewer rejects forged reader';passed=$true;rejection=$rejected})
$result=@{at=[DateTimeOffset]::Now.ToString('o');status='passed';checks=$checks;actualCoreProbes=$probes.Count;newRuntimeSha256=(Get-FileHash -LiteralPath (Join-Path $newDir 'cerulean-runtime.exe')).Hash.ToLowerInvariant();programReplacementTested=$false;installed=$false;realModelCalls=0;userDesktopActions=0;limits='Product functions plus actual format CLI and full public package rejection; not installed updater Prepare/Apply or native maintenance UI'}
[IO.File]::WriteAllText((Join-Path $run 'result.json'),($result|ConvertTo-Json -Depth 15),[Text.UTF8Encoding]::new($false))
Write-Output "Passed $($checks.Count) gate checks; actual metadata probes $($probes.Count)"
