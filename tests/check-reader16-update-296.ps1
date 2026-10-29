$ErrorActionPreference='Stop'
$OutputEncoding=[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
$repo=Split-Path -Parent $PSScriptRoot
$out=Join-Path $repo 'reports\installation-preflight-296\update-gate'
if(Test-Path -LiteralPath $out){throw 'Preserve original update gate check'}
$null=New-Item -ItemType Directory -Path $out
function Definition([string]$file,[string]$name){
    $tokens=$null;$errors=$null
    $ast=[Management.Automation.Language.Parser]::ParseFile($file,[ref]$tokens,[ref]$errors)
    if($errors.Count){throw "PowerShell syntax failure: $file"}
    $node=$ast.Find({param($n) $n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq $name},$true)
    if(-not $node){throw "Missing function: $name"}
    return [scriptblock]::Create($node.Extent.Text)
}
. (Definition (Join-Path $repo 'scripts\manage-windows-update.ps1') 'ReaderGeneration')
. (Definition (Join-Path $repo 'scripts\manage-windows-update.ps1') 'CheckDataFormat')
$source=Get-Content -LiteralPath (Join-Path $repo 'reports\p6-windows-package-0116-20261023\manifest.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$candidate=Get-Content -LiteralPath (Join-Path $repo 'reports\p6-windows-package-0119-20261023\manifest.json') -Raw -Encoding UTF8 | ConvertFrom-Json
function Package($manifest){return [pscustomobject]@{packageDirectory='frozen-package-reviewed-separately';capabilities=$manifest.coreBuildInfo}}
$reader15=Package $source;$reader16=Package $candidate
if((ReaderGeneration $reader15) -ne 15 -or (ReaderGeneration $reader16) -ne 16){throw 'Frozen package reader declarations rejected'}
$checks=[Collections.Generic.List[object]]::new()
$checks.Add(@{name='actual manifest reader15 and reader16 declarations';passed=$true})
& {
    . (Definition (Join-Path $repo 'reports\installation-preflight-296\before\manage-windows-update.ps1') 'ReaderGeneration')
    foreach($package in @($reader15,$reader16)){
        $errorText='';try{$null=ReaderGeneration $package}catch{$errorText=$_.Exception.Message}
        if(-not $errorText.Contains('Unsupported or inconsistent')){throw 'Old controller accepted future reader'}
    }
}
$checks.Add(@{name='old controller still rejects reader15 and reader16';passed=$true})
$script:format=$null;$script:probes=0
function Core([string]$binary,[string[]]$arguments){$script:probes++;return [pscustomobject]@{result=$script:format}}
function Case([string]$name,$left,$right,[string[]]$markers,[int]$minimum,[string]$expected='',[string]$guard=('a'*64)){
    $data=Join-Path $out ('case-'+$checks.Count)
    $events=Join-Path $data 'events'
    $null=New-Item -ItemType Directory -Path $events -Force
    foreach($marker in $markers){[IO.File]::WriteAllText((Join-Path $events ('00000000.reader'+$marker+'.json')),'{}')}
    $script:plan=[pscustomobject]@{data=$data}
    $script:format=[pscustomobject]@{dataSchema=1;minimumReaderGeneration=$minimum;readerGuardDigest=$guard}
    $before=$script:probes;$errorText=''
    try{CheckDataFormat $left $right}catch{$errorText=$_.Exception.Message}
    if($expected){if(-not $errorText.Contains($expected)){throw "$name unexpected result: $errorText"}}
    elseif($errorText){throw "$name failed: $errorText"}
    $checks.Add(@{name=$name;passed=$true;rejection=$errorText;formatProbes=$script:probes-$before})
}
Case 'reader15 data supports 0.1.16 to 0.1.19 binary update' $reader15 $reader16 @('15') 15
Case 'reader15 source rejected after reader16 promotion' $reader15 $reader16 @('16') 16 'cannot both read relay generation 16'
Case 'reader16 source and target accept protected relay data' $reader16 $reader16 @('16') 16
Case 'reader14 source rejected for reader15 team-import guard' ([pscustomobject]@{capabilities=([pscustomobject]@{dataReaderGeneration=14;dataFormatGuardSchema=1;binaryUpdateSchema=2;dataRecoverySchema=2;readableDataSchemas=@(1,2)})}) $reader16 @('15') 15 'cannot both read team import generation 15'
Case 'reader16 recognizes complete 14-15-16 marker chain' $reader16 $reader16 @('14','15','16') 16
Case 'reader16 rejects mismatched minimum status' $reader16 $reader16 @('16') 15 'declaration is missing or inconsistent'
Case 'reader16 rejects malformed guard digest' $reader16 $reader16 @('16') 16 'declaration is missing or inconsistent' 'bad'
Case 'reader16 refuses an unknown future marker' $reader16 $reader16 @('17') 17 'Unknown reader guard'
$plain=Join-Path $out 'plain';$null=New-Item -ItemType Directory -Path (Join-Path $plain 'events')
$script:plan=[pscustomobject]@{data=$plain}
$script:format=[pscustomobject]@{dataSchema=1;minimumReaderGeneration=1;readerGuardDigest=$null}
CheckDataFormat $reader15 $reader16
$checks.Add(@{name='source and candidate accept plain data';passed=$true})
$result=@{at=[DateTimeOffset]::Now.ToString('o');passed=$checks.Count;checks=$checks;controlledFormatReplies=$script:probes;actualInstallerLaunches=0;actualModelCalls=0;releaseAccepted=$false}
[IO.File]::WriteAllText((Join-Path $out 'result.json'),($result|ConvertTo-Json -Depth 12),[Text.UTF8Encoding]::new($false))
Write-Output "Passed $($checks.Count) reader15/16 update gate checks"
