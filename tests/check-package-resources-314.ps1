param([Parameter(Mandatory=$true)][string]$OutputDirectory)
$ErrorActionPreference='Stop'
$repo=Split-Path -Parent $PSScriptRoot
$out=[IO.Path]::GetFullPath($OutputDirectory)
$allowed=[IO.Path]::GetFullPath((Join-Path $repo 'reports'))+'\'
if(-not $out.StartsWith($allowed,[StringComparison]::OrdinalIgnoreCase) -or (Test-Path -LiteralPath $out)){throw 'Use a new reports directory'}
$null=New-Item -ItemType Directory -Path $out
function Definition([string]$file,[string]$name){
    $tokens=$null;$errors=$null
    $ast=[Management.Automation.Language.Parser]::ParseFile($file,[ref]$tokens,[ref]$errors)
    if($errors.Count){throw 'PowerShell syntax failure'}
    $node=$ast.Find({param($n) $n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq $name},$true)
    if(-not $node){throw "Missing function: $name"}
    return [scriptblock]::Create($node.Extent.Text)
}
$review=Join-Path $repo 'scripts\test-windows-package.ps1'
$update=Join-Path $repo 'scripts\manage-windows-update.ps1'
. (Definition $review 'PackageResources')
foreach($name in @('CheckResourceLayout','CheckInstalledFiles','StageReviewedFiles','CheckStagedFiles','HashHandle')){. (Definition $update $name)}
$helper=Get-Content -LiteralPath $review -Raw -Encoding UTF8
$typeSource=[regex]::Match($helper,"(?s)Add-Type -TypeDefinition @'\r?\n(.*?)\r?\n'@").Groups[1].Value
if(-not $typeSource){throw 'Missing pinned file helper'}
Add-Type -TypeDefinition $typeSource
$checks=[Collections.Generic.List[object]]::new()
function Pass([string]$name){$checks.Add(@{name=$name;passed=$true})}
function Reject([string]$name,[scriptblock]$body,[string]$expected){
    $errorText='';try{& $body | Out-Null}catch{$errorText=$_.Exception.Message}
    if(-not $errorText.Contains($expected)){throw "$name not rejected as expected: $errorText"}
    Pass $name
}
function Entry([string]$name,[byte[]]$body){
    $sha=[Security.Cryptography.SHA256]::Create()
    try{$digest=[BitConverter]::ToString($sha.ComputeHash($body)).Replace('-','').ToLowerInvariant()}finally{$sha.Dispose()}
    return [pscustomobject]@{path=$name;bytes=$body.Length;sha256=$digest;directory=$false}
}
function Resource([string]$name){return Entry $name ([Text.Encoding]::UTF8.GetBytes($name))}
$resourceNames=@('SQLITE-NOTICES.txt','THIRD-PARTY-NOTICES.txt','RUST-LIBRARY-COPYRIGHT.html','THIRD-PARTY-SOURCES.zip','THIRD-PARTY-MANIFEST.json')
$resources=@($resourceNames | ForEach-Object {Resource $_})
$legacy=[pscustomobject]@{version='0.1.20';resources=@()}
$sqlite=[pscustomobject]@{version='0.1.21';resources=@($resources[0])}
$full=[pscustomobject]@{version='0.1.22';resources=$resources}
if(@(PackageResources $legacy).Count -ne 0 -or @(PackageResources $sqlite).Count -ne 1 -or @(PackageResources $full).Count -ne 5){throw 'Versioned resource counts differ'}
Pass 'legacy, SQLite-only and complete resource versions supported'
$verifier=Resource 'cerulean-verifier.exe'
$bound=[pscustomobject]@{version='0.1.39';resources=@($resources)+$verifier;coreBuildInfo=[pscustomobject]@{independentVerifierSchema=1;independentVerifierSha256=$verifier.sha256}}
if(@(PackageResources $bound).Count -ne 6){throw 'Bound verifier resource missing'}
Pass 'new package includes the verifier bound by the core digest'
$bound.coreBuildInfo.independentVerifierSha256='0'*64
Reject 'changed verifier rejected' {PackageResources $bound} 'Verifier and core binding differ'
$bound.coreBuildInfo.independentVerifierSha256=$verifier.sha256
Reject 'verifier layout change needs full installer' {CheckResourceLayout $full $bound} 'full-installer'
Reject 'new version requires complete set' {PackageResources ([pscustomobject]@{version='0.1.22';resources=@($resources[0])})} 'missing or unexpected'
$bad=@($resources[0],$resources[1],$resources[2],$resources[3],$resources[0])
Reject 'duplicate resource rejected' {PackageResources ([pscustomobject]@{version='0.1.22';resources=$bad})} 'missing, duplicate'
$bad=@($resources[0],$resources[1],$resources[2],$resources[3],(Resource '../THIRD-PARTY-MANIFEST.json'))
Reject 'external resource path rejected' {PackageResources ([pscustomobject]@{version='0.1.22';resources=$bad})} 'missing, duplicate'
$oversized=Resource 'THIRD-PARTY-MANIFEST.json';$oversized.bytes=1048577
Reject 'oversized resource rejected' {PackageResources ([pscustomobject]@{version='0.1.22';resources=@($resources[0..3])+$oversized})} 'size or digest'
Reject 'resource layout addition routes to full installer' {CheckResourceLayout $sqlite $full} 'full-installer'
Reject 'resource layout deletion routes to full installer' {CheckResourceLayout $full $sqlite} 'full-installer'
CheckResourceLayout $full $full
Pass 'same resource layout supports byte replacement'
$stage=Join-Path $out 'stage';$candidate=Join-Path $out 'candidate'
$null=New-Item -ItemType Directory -Path $stage,$candidate
$oldEntries=@();$newEntries=@()
foreach($name in @('CeruleanLonging.exe','cerulean-runtime.exe')+$resourceNames){
    $old=[Text.Encoding]::UTF8.GetBytes('old:'+ $name);$new=[Text.Encoding]::UTF8.GetBytes('new longer:'+ $name)
    [IO.File]::WriteAllBytes((Join-Path $stage $name),$old)
    [IO.File]::WriteAllBytes((Join-Path $candidate $name),$new)
    $oldEntries+=Entry $name $old;$newEntries+=Entry $name $new
}
$uninstall=[Text.Encoding]::UTF8.GetBytes('original uninstall fixture')
[IO.File]::WriteAllBytes((Join-Path $stage 'uninstall.exe'),$uninstall)
$oldEntries+=Entry 'uninstall.exe' $uninstall
$source=[pscustomobject]@{resources=@($oldEntries | Where-Object {$_.path -cin $resourceNames});artifacts=@($oldEntries | Where-Object {$_.path -cin @('CeruleanLonging.exe','cerulean-runtime.exe')})}
$target=[pscustomobject]@{resources=@($newEntries | Where-Object {$_.path -cin $resourceNames});artifacts=@($newEntries | Where-Object {$_.path -cin @('CeruleanLonging.exe','cerulean-runtime.exe')});packageDirectory=$candidate}
CheckInstalledFiles $source $oldEntries
Reject 'modified installed resource refused' {CheckInstalledFiles $source @($oldEntries | Where-Object {$_.path -cne 'THIRD-PARTY-NOTICES.txt'})} 'does not match'
StageReviewedFiles $target $stage
$after=@(Get-ChildItem -LiteralPath $stage -File | ForEach-Object {Entry $_.Name ([IO.File]::ReadAllBytes($_.FullName))})
CheckStagedFiles $target $oldEntries $after
Pass 'real staging replaces both executables and five resource files, retains old uninstaller'
Reject 'stale resources in stage refused' {CheckStagedFiles $target $oldEntries $oldEntries} 'Staged bytes'
Reject 'unexpected staged file refused' {CheckStagedFiles $target $oldEntries (@($after)+(Resource 'unexpected.txt'))} 'file set'
$firstResource=Join-Path $candidate 'SQLITE-NOTICES.txt'
[IO.File]::WriteAllBytes($firstResource,[Text.Encoding]::UTF8.GetBytes('changed after review'))
Reject 'changed candidate resource refused' {StageReviewedFiles $target $stage} 'Candidate changed'
$result=@{passed=$checks.Count;checks=$checks;installed=$false;registryChanged=$false;modelCalls=0;releaseAccepted=$false}
[IO.File]::WriteAllText((Join-Path $out 'result.json'),($result | ConvertTo-Json -Depth 8),[Text.UTF8Encoding]::new($false))
Write-Output "Passed $($checks.Count) package resource and staging checks"
