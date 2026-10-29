$ErrorActionPreference='Stop'
$OutputEncoding=[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
$repo=Split-Path -Parent $PSScriptRoot
$out=Join-Path $repo 'reports\experiment-reader-257\controller'
if(Test-Path -LiteralPath $out){throw 'Preserve original controller check'}
$null=New-Item -ItemType Directory -Path $out
function Definition([string]$file,[string]$name){
 $tokens=$null;$errors=$null;$ast=[Management.Automation.Language.Parser]::ParseFile($file,[ref]$tokens,[ref]$errors)
 if($errors.Count){throw "PowerShell syntax failure: $file"}
 $node=$ast.Find({param($n) $n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq $name},$true)
 if(-not $node){throw "Missing function: $name"}
 return [scriptblock]::Create($node.Extent.Text)
}
. (Definition (Join-Path $repo 'scripts\manage-windows-update.ps1') 'ReaderGeneration')
. (Definition (Join-Path $repo 'scripts\manage-windows-update.ps1') 'CheckDataFormat')
$checks=[Collections.Generic.List[object]]::new();$script:probes=0
function Package($generation){return [pscustomobject]@{packageDirectory='synthetic-unused';capabilities=[pscustomobject]@{version='0.1.15';dataReaderGeneration=$generation;dataFormatGuardSchema=1;binaryUpdateSchema=2;dataRecoverySchema=2;readableDataSchemas=@(1,2)}}}
function Core([string]$binary,[string[]]$arguments){$script:probes++;return [pscustomobject]@{result=$script:format}}
function Check([string]$name,$source,$target,[string]$data,[string]$expected=''){
 $script:plan=[pscustomobject]@{data=$data};$before=$script:probes;$errorText=''
 try{CheckDataFormat $source $target}catch{$errorText=$_.Exception.Message}
 if($expected){if(-not $errorText.Contains($expected)){throw "$name unexpected result: $errorText"}}elseif($errorText){throw "$name failed: $errorText"}
 $checks.Add(@{name=$name;passed=$true;rejection=$errorText;controlledFormatReplies=$script:probes-$before})
}
$current=Package 14;$prior=Package 13
foreach($g in 3..14){if((ReaderGeneration (Package $g)) -ne $g){throw 'Numeric reader rejected'};$checks.Add(@{name="reader $g compatible declaration";passed=$true})}
& {
 . (Definition (Join-Path $repo 'reports\experiment-reader-257\source-before\scripts\manage-windows-update.ps1') 'ReaderGeneration')
 $failed='';try{$null=ReaderGeneration $current}catch{$failed=$_.Exception.Message}
 if(-not $failed.Contains('Unsupported or inconsistent')){throw 'Old controller did not reject new declaration'}
 $checks.Add(@{name='old controller refuses reader14 declaration';passed=$true})
}
$data=Join-Path $out 'protected';$events=Join-Path $data 'events';$null=New-Item -ItemType Directory -Path $events
[IO.File]::WriteAllText((Join-Path $events '00000000.reader14.json'),'{}')
$script:format=[pscustomobject]@{dataSchema=1;minimumReaderGeneration=14;readerGuardDigest=('a'*64)}
Check 'reader14 source and target accept controlled reader14 status' $current $current $data
Check 'reader13 source refuses protected experiment data' $prior $current $data 'cannot both read experiment generation 14'
Check 'reader13 target refuses protected experiment data' $current $prior $data 'cannot both read experiment generation 14'
foreach($g in @(15,'14',14.5,$null)){Check 'future or untyped declarations refused' $current (Package $g) $data $(if($null -eq $g){'Reader generation missing'}else{'Unsupported or inconsistent'})}
foreach($reply in @(@{dataSchema=1;minimumReaderGeneration=13;readerGuardDigest=('a'*64)},@{dataSchema=1;minimumReaderGeneration='14';readerGuardDigest=('a'*64)},@{dataSchema=1;minimumReaderGeneration=14;readerGuardDigest='bad'})){
 $script:format=[pscustomobject]$reply;Check 'inconsistent controlled format refused' $current $current $data 'declaration is missing or inconsistent'
}
$script:format=[pscustomobject]@{dataSchema=1;minimumReaderGeneration=14;readerGuardDigest=('a'*64)}
[IO.File]::WriteAllText((Join-Path $events '00000000.reader15.json'),'{}');Check 'future marker refused before probe' $current $current $data 'Unknown reader guard'
[IO.File]::WriteAllText((Join-Path $events '.format-migration-experiments-partial.pending'),'partial');Check 'partial promotion refused before update' $current $current $data 'Data migration is incomplete'
$plain=Join-Path $out 'plain';$null=New-Item -ItemType Directory -Path (Join-Path $plain 'events');$script:format=[pscustomobject]@{dataSchema=1;minimumReaderGeneration=1;readerGuardDigest=$null}
Check 'reader13 and 14 still accept plain data' $prior $current $plain
$result=@{passed=$checks.Count;checks=$checks;controlledFormatReplies=$script:probes;realCoreProbes=0;modelCalls=0;installations=0;programReplacementTested=$false}
[IO.File]::WriteAllText((Join-Path $out 'result.json'),($result|ConvertTo-Json -Depth 10),[Text.UTF8Encoding]::new($false))
$result.passed
