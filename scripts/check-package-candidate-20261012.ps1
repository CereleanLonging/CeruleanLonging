param([Parameter(Mandatory=$true)][string]$PackageDirectory,[string]$ExpectedVersion='0.1.1')
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$run=Join-Path $root ('reports\package-candidate-native-'+[Guid]::NewGuid().ToString('N'))
$null=New-Item -ItemType Directory -Path $run
$copy=Join-Path $run 'candidate'
Copy-Item -LiteralPath $PackageDirectory -Destination $copy -Recurse
$manifestPath=Join-Path $copy 'manifest.json'
$original=[IO.File]::ReadAllBytes($manifestPath)
$checks=[Collections.Generic.List[object]]::new()
function Digest {return (Get-FileHash -LiteralPath $manifestPath -Algorithm SHA256).Hash.ToLowerInvariant()}
function Check([string]$name,[scriptblock]$action,[string]$expected='') {
    $message='';$result=$null
    try{$result=& $action}catch{$message=$_.Exception.Message}
    if($expected){if(-not $message.Contains($expected)){throw "$name expected '$expected', received '$message'"}}
    elseif($message){throw "$name failed: $message"}
    $checks.Add(@{name=$name;passed=$true;rejection=$message})
    return $result
}
function Rewrite($manifest){[IO.File]::WriteAllText($manifestPath,($manifest | ConvertTo-Json -Depth 15),[Text.UTF8Encoding]::new($false))}
function ResetManifest {[IO.File]::WriteAllBytes($manifestPath,$original)}
$script=Join-Path $PSScriptRoot 'test-windows-package.ps1'
$review=Check 'reviewed internal candidate' {& $script -PackageDirectory $copy -ManifestSha256 (Digest) -AllowUnsignedInternal}
$accepted=$review | ConvertFrom-Json
if($accepted.status -ne 'passed' -or $accepted.releaseAccepted -ne $false -or $accepted.installed -ne $false -or $accepted.version -ne $ExpectedVersion){throw 'Unexpected acceptance result'}
$retained=& $script -PackageDirectory $copy -ManifestSha256 (Digest) -AllowUnsignedInternal -RetainPins
try{
    Check 'retained candidate handles exclude writers' {
        $opened=$null;$rejected=$false
        try{$opened=[IO.File]::Open((Join-Path $copy 'cerulean-runtime.exe'),[IO.FileMode]::Open,[IO.FileAccess]::ReadWrite,[IO.FileShare]::ReadWrite)}catch{if($_.Exception.GetBaseException() -is [IO.IOException]){$rejected=$true}else{throw}}
        if($opened){$opened.Dispose()};if(-not $rejected){throw 'Retained package permitted a writer'}
    } | Out-Null
}finally{foreach($pin in $retained.Pins){$pin.Dispose()}}
Check 'explicit pin disposal permits later access' {
    $opened=[IO.File]::Open((Join-Path $copy 'cerulean-runtime.exe'),[IO.FileMode]::Open,[IO.FileAccess]::ReadWrite,[IO.FileShare]::ReadWrite);$opened.Dispose()
} | Out-Null
Check 'unsigned default rejected' {& $script -PackageDirectory $copy -ManifestSha256 (Digest)} 'Public release requires' | Out-Null
Check 'stale manifest rejected' {& $script -PackageDirectory $copy -ManifestSha256 ('0'*64) -AllowUnsignedInternal} 'Reviewed manifest digest changed' | Out-Null
$manifest=Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
$manifest.coreBuildInfo.backupWriterVersions+= '9.9.9';Rewrite $manifest
Check 'runtime capability mismatch rejected' {& $script -PackageDirectory $copy -ManifestSha256 (Digest) -AllowUnsignedInternal} 'Actual runtime capabilities differ' | Out-Null
ResetManifest
if($accepted.capabilities.dataFormatGuardSchema){
    $manifest=Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
    $manifest.coreBuildInfo.dataFormatGuardSchema=99;Rewrite $manifest
    Check 'data format guard capability mismatch rejected' {& $script -PackageDirectory $copy -ManifestSha256 (Digest) -AllowUnsignedInternal} 'Actual runtime capabilities differ' | Out-Null
    ResetManifest
    $manifest=Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
    $manifest.coreBuildInfo.readableDataSchemas+=99;Rewrite $manifest
    Check 'readable data schema mismatch rejected' {& $script -PackageDirectory $copy -ManifestSha256 (Digest) -AllowUnsignedInternal} 'Actual runtime capabilities differ' | Out-Null
    ResetManifest
}
$manifest=Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
$manifest.coreBuildInfo | Add-Member -NotePropertyName maintenanceInterlockSchema -NotePropertyValue 99 -Force
Rewrite $manifest
Check 'maintenance interlock capability mismatch rejected' {& $script -PackageDirectory $copy -ManifestSha256 (Digest) -AllowUnsignedInternal} 'Actual runtime capabilities differ' | Out-Null
    [IO.File]::WriteAllBytes($manifestPath,$original)
    $manifest=Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
    $manifest.coreBuildInfo | Add-Member -NotePropertyName dataRecoverySchema -NotePropertyValue 99 -Force
    [IO.File]::WriteAllText($manifestPath,($manifest|ConvertTo-Json -Depth 30),[Text.UTF8Encoding]::new($false))
    Check 'data recovery capability mismatch rejected' {& $script -PackageDirectory $copy -ManifestSha256 (Digest) -AllowUnsignedInternal} 'Actual runtime capabilities differ' | Out-Null
ResetManifest
$manifest=Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
$manifest.artifacts[0].path='../setup.exe';Rewrite $manifest
Check 'noncanonical artifact rejected' {& $script -PackageDirectory $copy -ManifestSha256 (Digest) -AllowUnsignedInternal} 'Missing or duplicate package artifact' | Out-Null
ResetManifest
$manifest=Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
$manifest.releaseAccepted=$true;Rewrite $manifest
Check 'self-declared release still requires actual signature' {& $script -PackageDirectory $copy -ManifestSha256 (Digest) -PublisherThumbprint ('0'*40)} 'Publisher verification failed' | Out-Null
ResetManifest
$runtime=Join-Path $copy 'cerulean-runtime.exe'
$stream=[IO.File]::Open($runtime,[IO.FileMode]::Append,[IO.FileAccess]::Write,[IO.FileShare]::None)
try{$stream.WriteByte(0)}finally{$stream.Dispose()}
Check 'changed binary rejected before execution' {& $script -PackageDirectory $copy -ManifestSha256 (Digest) -AllowUnsignedInternal} 'Artifact integrity mismatch' | Out-Null
Copy-Item -LiteralPath (Join-Path $PackageDirectory 'cerulean-runtime.exe') -Destination $runtime -Force
$legacy=Join-Path $root 'reports\p6-windows-package-final-20261012'
Check 'legacy package without capability rejected before execution' {& $script -PackageDirectory $legacy -ManifestSha256 ((Get-FileHash -LiteralPath (Join-Path $legacy 'manifest.json')).Hash) -AllowUnsignedInternal} 'Required candidate capabilities missing' | Out-Null
$result=[ordered]@{status='passed';run=$run;candidateReview=$accepted;checks=$checks;passed=$checks.Count;installations=0;realModelCalls=0;userDesktopActions=0;releaseAccepted=$false}
$json=$result | ConvertTo-Json -Depth 16
[IO.File]::WriteAllText((Join-Path $run 'result.json'),$json,[Text.UTF8Encoding]::new($false))
$result | ConvertTo-Json -Depth 16 -Compress
