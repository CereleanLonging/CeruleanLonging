param(
    [Parameter(Mandatory=$true)][ValidateSet('Prepare','Review','Apply','Rollback','Commit','Reconcile','AbandonCommit','AbandonPreparation','FinishMaintenance')][string]$Action,
    [Parameter(Mandatory=$true)][string]$TransactionDirectory,
    [string]$PlanDigest,
    [string]$InstalledDirectory,[string]$DataDirectory,[string]$HoldDigest,
    [string]$SourcePackage,[string]$SourceManifestSha256,
    [string]$CandidatePackage,[string]$CandidateManifestSha256,
    [switch]$AllowUnsignedInternal,[string]$PublisherThumbprint,
    [string]$MaintenanceCore,[string]$MaintenanceCoreSha256,
    [string]$IntentDigest,[string]$GuardDigest
)
$ErrorActionPreference='Stop'
$OutputEncoding=[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
$owned=[Collections.Generic.List[IDisposable]]::new()
$packagePins=[Collections.Generic.List[IDisposable]]::new()
$run=[IO.Path]::GetFullPath($TransactionDirectory).TrimEnd('\')
$sid=[Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$uninstallKey='HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\CeruleanLonging'
$productKey='HKCU:\Software\ceruleanlonging\CeruleanLonging'
$planPath=Join-Path $run 'plan.json'
$eventDigest='';$events=@();$plan=$null
$releaseObservation=$null
$maintenanceObservation=$null;$maintenanceBinary=$null;$reply=$null
$guards=[Collections.Generic.Dictionary[string,object]]::new([StringComparer]::OrdinalIgnoreCase)
function ReaderGeneration($package){
    # Explicit declarations support numeric readers 3-18; no coercion or future-generation fallback.
    $cap=$package.capabilities
    if($null -eq $cap){throw 'Package reader capabilities missing'}
    if($null -ne $cap.dataReaderGeneration){
        if(($cap.dataReaderGeneration -isnot [int] -and $cap.dataReaderGeneration -isnot [long]) -or $cap.dataReaderGeneration -notin @(3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18) -or $cap.dataFormatGuardSchema -ne 1 -or $cap.binaryUpdateSchema -ne 2 -or $cap.dataRecoverySchema -ne 2 -or (@($cap.readableDataSchemas) -join ',') -cne '1,2'){throw 'Unsupported or inconsistent explicit reader capabilities'}
        return $cap.dataReaderGeneration
    }
    # Only these inspected legacy versions predate explicit reader declarations.
    $legacyOne=($cap.version -ceq '0.1.1' -and $null -eq $cap.binaryUpdateSchema -and $null -eq $cap.dataSchema) -or ($cap.version -cin @('0.1.2','0.1.3','0.1.4') -and $cap.binaryUpdateSchema -eq 1 -and $cap.dataSchema -eq 1)
    if($legacyOne -and $null -eq $cap.dataFormatGuardSchema -and $null -eq $cap.readableDataSchemas){return 1}
    if($cap.version -cin @('0.1.5','0.1.6','0.1.7','0.1.8') -and $cap.dataFormatGuardSchema -eq 1 -and $cap.binaryUpdateSchema -eq 1 -and (@($cap.readableDataSchemas) -join ',') -ceq '1,2'){return 2}
    throw 'Reader generation missing for an unreviewed version or capability combination'
}
function CheckDataFormat($source,$target){
    $sourceReader=ReaderGeneration $source;$targetReader=ReaderGeneration $target
    $marker=Join-Path $script:plan.data 'events\00000000.json'
    $readerMarker=Join-Path $script:plan.data 'events\00000000.reader3.json'
    $currencyMarker=Join-Path $script:plan.data 'events\00000000.reader4.json'
    $preparationMarker=Join-Path $script:plan.data 'events\00000000.reader5.json'
    $countMarker=Join-Path $script:plan.data 'events\00000000.reader6.json'
    $resolutionMarker=Join-Path $script:plan.data 'events\00000000.reader7.json'
    $scopedPricingMarker=Join-Path $script:plan.data 'events\00000000.reader8.json'
    $scopedCountMarker=Join-Path $script:plan.data 'events\00000000.reader9.json'
    $scopedExecutionMarker=Join-Path $script:plan.data 'events\00000000.reader10.json'
    $apiCurrencyMarker=Join-Path $script:plan.data 'events\00000000.reader11.json'
    $apiFeeResolutionMarker=Join-Path $script:plan.data 'events\00000000.reader12.json'
    $managementSourceMarker=Join-Path $script:plan.data 'events\00000000.reader13.json'
    $experimentMarker=Join-Path $script:plan.data 'events\00000000.reader14.json'
    $teamImportMarker=Join-Path $script:plan.data 'events\00000000.reader15.json'
    $relayMarker=Join-Path $script:plan.data 'events\00000000.reader16.json'
    $rejectedUsageMarker=Join-Path $script:plan.data 'events\00000000.reader17.json'
    $verificationRecheckMarker=Join-Path $script:plan.data 'events\00000000.reader18.json'
    $pending=@(Get-ChildItem -LiteralPath (Join-Path $script:plan.data 'events') -Filter '.format-migration-*.pending' -ErrorAction Stop)
    if($pending.Count){throw 'Data migration is incomplete; no program update is allowed'}
    $unknown=@(Get-ChildItem -LiteralPath (Join-Path $script:plan.data 'events') -Filter '00000000.reader*' | Where-Object {$_.Name -cnotin @('00000000.reader3.json','00000000.reader4.json','00000000.reader5.json','00000000.reader6.json','00000000.reader7.json','00000000.reader8.json','00000000.reader9.json','00000000.reader10.json','00000000.reader11.json','00000000.reader12.json','00000000.reader13.json','00000000.reader14.json','00000000.reader15.json','00000000.reader16.json','00000000.reader17.json','00000000.reader18.json')})
    if($unknown.Count){throw 'Unknown reader guard; no program update is allowed'}
    $readerPresent=Test-Path -LiteralPath $readerMarker
    $currencyPresent=Test-Path -LiteralPath $currencyMarker
    $preparationPresent=Test-Path -LiteralPath $preparationMarker
    $countPresent=Test-Path -LiteralPath $countMarker
    $resolutionPresent=Test-Path -LiteralPath $resolutionMarker
    $scopedPricingPresent=Test-Path -LiteralPath $scopedPricingMarker
    $scopedCountPresent=Test-Path -LiteralPath $scopedCountMarker
    $scopedExecutionPresent=Test-Path -LiteralPath $scopedExecutionMarker
    $apiCurrencyPresent=Test-Path -LiteralPath $apiCurrencyMarker
    $apiFeeResolutionPresent=Test-Path -LiteralPath $apiFeeResolutionMarker
    $managementSourcePresent=Test-Path -LiteralPath $managementSourceMarker
    $experimentPresent=Test-Path -LiteralPath $experimentMarker
    $teamImportPresent=Test-Path -LiteralPath $teamImportMarker
    $relayPresent=Test-Path -LiteralPath $relayMarker
    $rejectedUsagePresent=Test-Path -LiteralPath $rejectedUsageMarker
    $verificationRecheckPresent=Test-Path -LiteralPath $verificationRecheckMarker
    if($verificationRecheckPresent -and ($sourceReader -lt 18 -or $targetReader -lt 18)){throw 'Source/candidate cannot both read corrected verification generation 18'}
    if($rejectedUsagePresent -and ($sourceReader -lt 17 -or $targetReader -lt 17)){throw 'Source/candidate cannot both read rejected usage generation 17'}
    if($relayPresent -and ($sourceReader -lt 16 -or $targetReader -lt 16)){throw 'Source/candidate cannot both read relay generation 16'}
    if($teamImportPresent -and ($sourceReader -lt 15 -or $targetReader -lt 15)){throw 'Source/candidate cannot both read team import generation 15'}
    if($experimentPresent -and ($sourceReader -lt 14 -or $targetReader -lt 14)){throw 'Source/candidate cannot both read experiment generation 14'}
    if($managementSourcePresent -and ($sourceReader -lt 13 -or $targetReader -lt 13)){throw 'Source/candidate cannot both read management source generation 13'}
    if($scopedCountPresent -and (-not $scopedPricingPresent -or $sourceReader -lt 9 -or $targetReader -lt 9)){throw 'Source/candidate cannot both read generation 9 or original reader8 guard is missing'}
    if($scopedExecutionPresent -and (-not $scopedCountPresent -or $sourceReader -lt 10 -or $targetReader -lt 10)){throw 'Source/candidate cannot both read generation 10 or original reader9 guard is missing'}
    if($apiCurrencyPresent -and (-not $scopedExecutionPresent -or $sourceReader -lt 11 -or $targetReader -lt 11)){throw 'Source/candidate cannot both read generation 11 or original reader10 guard is missing'}
    if($apiFeeResolutionPresent -and (-not $apiCurrencyPresent -or $sourceReader -lt 12 -or $targetReader -lt 12)){throw 'Source/candidate cannot both read generation 12 or original reader11 guard is missing'}
    if($scopedPricingPresent -and (-not $resolutionPresent -or $sourceReader -lt 8 -or $targetReader -lt 8)){throw 'Source/candidate cannot both read generation 8 or original reader7 guard is missing'}
    if($resolutionPresent -and (-not $countPresent -or $sourceReader -lt 7 -or $targetReader -lt 7)){throw 'Source/candidate cannot both read generation 7 or original reader6 guard is missing'}
    if($countPresent -and (-not $preparationPresent -or $sourceReader -lt 6 -or $targetReader -lt 6)){throw 'Source/candidate cannot both read generation 6 or original reader5 guard is missing'}
    if($preparationPresent -and (-not $currencyPresent -or $sourceReader -lt 5 -or $targetReader -lt 5)){throw 'Source/candidate cannot both read generation 5 or original reader4 guard is missing'}
    if($currencyPresent -and ($sourceReader -lt 4 -or $targetReader -lt 4)){throw 'Source/candidate cannot both read generation 4; restore the reviewed older backup before using an older reader'}
    if($readerPresent -and ($sourceReader -lt 3 -or $targetReader -lt 3)){throw 'Source/candidate cannot both read generation 3; restore the reviewed older backup before using an older reader'}
    if($targetReader -lt 2){
        if(Test-Path -LiteralPath $marker){throw 'Candidate cannot validate the active data format'}
        return
    }
    $format=(Core (Join-Path $target.packageDirectory 'cerulean-runtime.exe') @('data-format','status')).result
    $sourceSchemas=if($sourceReader -eq 1){@(1)}else{@($source.capabilities.readableDataSchemas)}
    if(($format.dataSchema -isnot [int] -and $format.dataSchema -isnot [long]) -or $format.dataSchema -notin @(1,2) -or $format.dataSchema -notin @($target.capabilities.readableDataSchemas) -or $format.dataSchema -notin $sourceSchemas){throw 'Source/candidate cannot both read the active data format; use the reviewed data migration recovery path'}
    $minimum=$format.minimumReaderGeneration
    if($null -eq $minimum -and $targetReader -eq 2 -and -not $readerPresent -and -not $currencyPresent -and -not $preparationPresent -and -not $countPresent -and -not $resolutionPresent -and -not $scopedPricingPresent -and -not $scopedCountPresent -and -not $scopedExecutionPresent -and -not $apiCurrencyPresent -and -not $apiFeeResolutionPresent -and -not $managementSourcePresent -and -not $experimentPresent -and -not $teamImportPresent -and -not $relayPresent -and -not $rejectedUsagePresent -and -not $verificationRecheckPresent){$minimum=$format.dataSchema}
    $expectedMinimum=if($verificationRecheckPresent){18}elseif($rejectedUsagePresent){17}elseif($relayPresent){16}elseif($teamImportPresent){15}elseif($experimentPresent){14}elseif($managementSourcePresent){13}elseif($apiFeeResolutionPresent){12}elseif($apiCurrencyPresent){11}elseif($scopedExecutionPresent){10}elseif($scopedCountPresent){9}elseif($scopedPricingPresent){8}elseif($resolutionPresent){7}elseif($countPresent){6}elseif($preparationPresent){5}elseif($currencyPresent){4}elseif($readerPresent){3}else{$format.dataSchema}
    if(($minimum -isnot [int] -and $minimum -isnot [long]) -or $minimum -notin @(1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18) -or $minimum -ne $expectedMinimum -or (($readerPresent -or $currencyPresent -or $preparationPresent -or $countPresent -or $resolutionPresent -or $scopedPricingPresent -or $scopedCountPresent -or $scopedExecutionPresent -or $apiCurrencyPresent -or $apiFeeResolutionPresent -or $managementSourcePresent -or $experimentPresent -or $teamImportPresent -or $relayPresent -or $rejectedUsagePresent -or $verificationRecheckPresent) -and $format.readerGuardDigest -cnotmatch '^[a-f0-9]{64}$')){throw 'Active data reader declaration is missing or inconsistent'}
    if($minimum -gt $sourceReader -or $minimum -gt $targetReader){throw 'Source/candidate reader generation is below the active minimum; restore a reviewed compatible backup first'}
}
function HashBytes([byte[]]$bytes){
    $sha=[Security.Cryptography.SHA256]::Create()
    try{return [BitConverter]::ToString($sha.ComputeHash($bytes)).Replace('-','').ToLowerInvariant()}finally{$sha.Dispose()}
}
function HashHandle($stream){
    $stream.Position=0;$sha=[Security.Cryptography.SHA256]::Create()
    try{return [BitConverter]::ToString($sha.ComputeHash($stream)).Replace('-','').ToLowerInvariant()}finally{$sha.Dispose();$stream.Position=0}
}
function DurableJson([string]$path,$value){
    $bytes=[Text.UTF8Encoding]::new($false).GetBytes(($value | ConvertTo-Json -Depth 30))
    if(Test-Path -LiteralPath $path){throw 'Durable record already exists; do not overwrite'}
    $temporary=Join-Path (Split-Path -Parent $path) ('.pending-'+[Guid]::NewGuid().ToString('N'))
    $file=[IO.File]::Open($temporary,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::Read)
    try{$file.Write($bytes,0,$bytes.Length);$file.Flush($true)}finally{$file.Dispose()}
    # No replacement. A killed writer leaves its partial temporary file as evidence;
    # readers only consume complete records published under the final name.
    [IO.File]::Move($temporary,$path)
    return (HashBytes $bytes)
}
function ReadJson([string]$path){
    $file=[CeruleanPackagePin]::Open($path,$false)
    try{
        if($file.Length -gt 4194304){throw 'Transaction metadata exceeds limit'}
        $reader=[IO.StreamReader]::new($file,[Text.UTF8Encoding]::new($false,$true))
        try{return ($reader.ReadToEnd() | ConvertFrom-Json)}finally{$reader.Dispose()}
    }finally{$file.Dispose()}
}
function PinChain([string]$path){
    $chain=[Collections.Generic.List[string]]::new();$cursor=[IO.DirectoryInfo]::new($path)
    while($null -ne $cursor){$chain.Insert(0,$cursor.FullName);$cursor=$cursor.Parent}
    foreach($p in $chain){$owned.Add([CeruleanPackagePin]::Open($p,$true))}
}
function InspectPackage([string]$path,[string]$digest){
    $result=& (Join-Path $PSScriptRoot 'test-windows-package.ps1') -PackageDirectory $path -ManifestSha256 $digest -AllowUnsignedInternal:$AllowUnsignedInternal -PublisherThumbprint $PublisherThumbprint -RetainPins
    foreach($pin in $result.Pins){$packagePins.Add($pin)}
    return $result.Review
}
function CheckResourceLayout($source,$target){
    # Binary update schema 2 retains the original uninstall program. It can
    # replace resource bytes only when the exact file set remains unchanged.
    $left=@($source.resources | ForEach-Object {$_.path} | Sort-Object)
    $right=@($target.resources | ForEach-Object {$_.path} | Sort-Object)
    if(($left -join "`n") -cne ($right -join "`n")){
        throw 'Package resource layout changed. Use the reviewed full-installer upgrade path so the new resources and uninstaller are installed together; no binary update was prepared.'
    }
}
function CheckInstalledFiles($source,$entries){
    foreach($wanted in @($source.artifacts | Where-Object {$_.path -cin @('CeruleanLonging.exe','cerulean-runtime.exe')})+@($source.resources)){
        $actual=@($entries | Where-Object {$_.path -ceq $wanted.path})
        if($actual.Count -ne 1 -or $actual[0].directory -or $actual[0].sha256 -cne $wanted.sha256 -or $actual[0].bytes -ne $wanted.bytes){throw 'Installed program or resource does not match reviewed source'}
    }
}
function StageReviewedFiles($target,[string]$stage){
    $replacements=@($target.artifacts | Where-Object {$_.path -cin @('CeruleanLonging.exe','cerulean-runtime.exe')})+@($target.resources)
    foreach($entry in $replacements){
        $src=[CeruleanPackagePin]::Open((Join-Path $target.packageDirectory $entry.path),$false)
        try{
            if($src.Length -ne $entry.bytes -or (HashHandle $src) -cne $entry.sha256){throw 'Candidate changed during stage preparation'}
            $dst=[IO.File]::Open((Join-Path $stage $entry.path),[IO.FileMode]::Truncate,[IO.FileAccess]::Write,[IO.FileShare]::None)
            try{$src.CopyTo($dst);$dst.Flush($true)}finally{$dst.Dispose()}
        }finally{$src.Dispose()}
    }
}
function CheckStagedFiles($target,$before,$after){
    if((@($before.path | Sort-Object) -join "`n") -cne (@($after.path | Sort-Object) -join "`n")){throw 'Staged file set differs from original installation'}
    $replacements=@($target.artifacts | Where-Object {$_.path -cin @('CeruleanLonging.exe','cerulean-runtime.exe')})+@($target.resources)
    foreach($entry in $after){
        $expected=@($replacements | Where-Object {$_.path -ceq $entry.path})
        if(-not $expected.Count){$expected=@($before | Where-Object {$_.path -ceq $entry.path})}
        if($expected.Count -ne 1 -or $entry.sha256 -cne $expected[0].sha256 -or $entry.bytes -ne $expected[0].bytes -or [bool]$entry.directory -ne [bool]$expected[0].directory){throw 'Staged bytes do not match reviewed candidate and original uninstall files'}
    }
}
function Core([string]$binary,[string[]]$arguments){
    # Arguments come from checked absolute paths and fixed options, not a shell string.
    $oldPreference=$ErrorActionPreference;$ErrorActionPreference='Continue'
    try{$raw=& $binary --workbench --data-dir $script:plan.data @arguments 2>&1;$exitCode=$LASTEXITCODE}finally{$ErrorActionPreference=$oldPreference}
    if($exitCode -ne 0){throw "Core maintenance command rejected ($exitCode): $($arguments[0]) $($arguments[1])"}
    $result=($raw -join "`n") | ConvertFrom-Json
    if(-not $result.ok){throw 'Core maintenance reply is not successful'}
    return $result
}
function OpenMaintenance {
    if($script:plan.maintenanceInterlockSchema -ne 1){throw 'Legacy transaction has no reserved maintenance contract; review with its original controller, do not adopt or rewrite it'}
    $binary=[IO.Path]::GetFullPath($script:plan.maintenanceCore)
    foreach($mutable in @($run,$script:plan.install,$script:plan.data)){
        if(-not (Disjoint $binary $mutable)){throw 'Maintenance core must remain outside all mutable transaction paths'}
    }
    if($script:plan.maintenanceCoreSha256 -notmatch '^[a-f0-9]{64}$'){throw 'Reviewed maintenance core digest required'}
    $cursor=[IO.DirectoryInfo]::new((Split-Path -Parent $binary))
    while($null -ne $cursor){$packagePins.Add([CeruleanPackagePin]::Open($cursor.FullName,$true));$cursor=$cursor.Parent}
    $pin=[CeruleanPackagePin]::Open($binary,$false);$packagePins.Add($pin)
    if((HashHandle $pin) -cne $script:plan.maintenanceCoreSha256){throw 'Reviewed maintenance core bytes changed'}
    $oldPreference=$ErrorActionPreference;$ErrorActionPreference='Continue'
    try{$raw=& $binary --build-info 2>&1;$code=$LASTEXITCODE}finally{$ErrorActionPreference=$oldPreference}
    if($code -ne 0){throw 'Maintenance core build capabilities unavailable'}
    $info=($raw -join "`n") | ConvertFrom-Json
    if($info.maintenanceInterlockSchema -ne 1 -or $info.binaryUpdateSchema -ne 2 -or (ReaderGeneration ([pscustomobject]@{capabilities=$info})) -lt $script:plan.controllerReaderGeneration){throw 'Maintenance core does not support this reader-aware update transaction'}
    $script:maintenanceBinary=$binary
}
function ObserveMaintenance {
    $expected=$GuardDigest
    $receipt=Join-Path $run 'maintenance-reservation.json'
    if(Test-Path -LiteralPath $receipt){
        $saved=ReadJson $receipt
        if($saved.schema -ne 1 -or ($expected -and $expected -cne $saved.guardDigest)){throw 'Maintenance receipt differs from explicit guard digest'}
        $expected=$saved.guardDigest
    }
    $commandArguments=@('maintenance','status')
    if($expected){$commandArguments+=@('--guard-digest',$expected)}
    $observation=(Core $script:maintenanceBinary $commandArguments).result
    $intentHash=HashBytes ([IO.File]::ReadAllBytes((Join-Path $run 'intent.json')))
    $reserved=$observation.reservation
    # Rust returns canonical Windows extended paths; compare normalized paths.
    if($null -eq $reserved -or $reserved.schema -ne $script:plan.schema -or $reserved.intentDigest -cne $intentHash -or $reserved.holdDigest -cne $script:plan.holdDigest -or $reserved.ownerSid -cne $sid -or $reserved.transaction.Replace('\\?\','').TrimEnd('\') -ine $run){throw 'Original maintenance reservation is missing or belongs to another transaction'}
    return $observation
}
function Disjoint([string]$a,[string]$b){
    return -not ($a.Equals($b,[StringComparison]::OrdinalIgnoreCase) -or $a.StartsWith($b+'\',[StringComparison]::OrdinalIgnoreCase) -or $b.StartsWith($a+'\',[StringComparison]::OrdinalIgnoreCase))
}
function RegistryVersion {
    if(-not (Test-Path -LiteralPath $productKey) -or -not (Test-Path -LiteralPath $uninstallKey)){throw 'Original current-user installation registration required'}
    $registered=(Get-Item -LiteralPath $productKey).GetValue('')
    $key=Get-Item -LiteralPath $uninstallKey
    # Locked Tauri NSIS stores InstallLocation quoted, and the product default unquoted.
    if($registered -ine $script:plan.install -or $key.GetValue('InstallLocation') -ine ('"'+$script:plan.install+'"') -or $key.GetValue('UninstallString') -ine ('"'+$script:plan.install+'\uninstall.exe"')){throw 'Installation registration changed ownership'}
    return [string]$key.GetValue('DisplayVersion')
}
function Tree([string]$directory,[string]$copyTo='') {
    $pins=[Collections.Generic.List[IDisposable]]::new();$items=[Collections.Generic.List[object]]::new();$total=0L
    try{
        $pins.Add([CeruleanPackagePin]::Open($directory,$true))
        $pending=[Collections.Generic.Queue[string]]::new();$pending.Enqueue('')
        while($pending.Count){
            $rel=$pending.Dequeue()
            foreach($item in Get-ChildItem -LiteralPath (Join-Path $directory $rel) -Force){
                $name=if($rel){$rel+'\'+$item.Name}else{$item.Name}
                if($item.Name -match '[:\\/]' -or $item.Name.EndsWith('.') -or $item.Name.EndsWith(' ')){throw 'Unsupported program path'}
                $folder=$item.PSIsContainer
                if($directory -ieq $script:plan.data -and $name -ceq 'writer.lock'){
                    $items.Add([pscustomobject][ordered]@{path=$name;directory=$false;bytes=0L;sha256=(HashBytes ([byte[]]@()));handle=$null});continue
                }
                if($script:guards.ContainsKey($item.FullName)){$file=$script:guards[$item.FullName]}else{$file=[CeruleanPackagePin]::Open($item.FullName,$folder);$pins.Add($file)}
                if($folder){$pending.Enqueue($name);$bytes=0L;$hash=''}else{$bytes=$file.Length;$total+=$bytes;$hash=HashHandle $file}
                if($items.Count -ge 4096 -or $total -gt 2147483648){throw 'Program inventory exceeds limit'}
                $items.Add([pscustomobject][ordered]@{path=$name;directory=$folder;bytes=$bytes;sha256=$hash;handle=$file})
            }
        }
        $ordered=@($items | Sort-Object path)
        if($copyTo){
            if(Test-Path -LiteralPath $copyTo){throw 'Stage destination already exists'}
            $null=New-Item -ItemType Directory -Path $copyTo
            foreach($entry in $ordered){
                $destination=Join-Path $copyTo $entry.path
                if($entry.directory){$null=New-Item -ItemType Directory -Path $destination}else{
                    $out=[IO.File]::Open($destination,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::None)
                    try{$entry.handle.CopyTo($out);$out.Flush($true)}finally{$out.Dispose()}
                }
            }
        }
        return @($ordered | ForEach-Object {[pscustomobject][ordered]@{path=$_.path;directory=$_.directory;bytes=$_.bytes;sha256=$_.sha256}})
    }finally{for($i=$pins.Count-1;$i -ge 0;$i--){$pins[$i].Dispose()}}
}
function SameTree([string]$directory,$expected){
    $actual=@(Tree $directory)
    if(($actual | ConvertTo-Json -Depth 8 -Compress) -cne (@($expected) | ConvertTo-Json -Depth 8 -Compress)){throw "Program inventory changed: $directory"}
}
function CheckData([switch]$AllowEmptyReleaseDirectory) {
    if((HashBytes ([IO.File]::ReadAllBytes((Join-Path $script:plan.data 'upgrade-hold.json')))) -cne $script:plan.holdDigest){throw 'Upgrade hold changed; do not continue maintenance'}
    $manifest=ReadJson (Join-Path $run 'data-backup\manifest.json')
    if((HashBytes ([IO.File]::ReadAllBytes((Join-Path $run 'data-backup\manifest.json')))) -cne $script:plan.backupDigest){throw 'Backup manifest changed'}
    # writer.lock is held exclusively by this process; inspect all other original bytes.
    $tree=@(Tree $script:plan.data)
    $actual=@($tree | ForEach-Object {$_.path.Replace('\','/')} | Sort-Object)
    $expected=@($manifest.entries | ForEach-Object {$_.path} | Sort-Object)
    if($AllowEmptyReleaseDirectory -and $expected -cnotcontains 'upgrade-holds'){
        # An interrupted in-process release can leave its newly-created empty directory.
        # Ignore only that directory entry; any added file still fails the exact inventory.
        $extra=@($tree | Where-Object {$_.path -ceq 'upgrade-holds' -and $_.directory})
        if($extra.Count -eq 1){$actual=@($actual | Where-Object {$_ -cne 'upgrade-holds'})}
    }
    if(($actual -join "`n") -cne ($expected -join "`n")){throw 'Data file set changed after backup'}
    foreach($entry in $manifest.entries){
        if($entry.path -ceq 'writer.lock'){continue}
        $p=Join-Path $script:plan.data $entry.path
        $file=[CeruleanPackagePin]::Open($p,$entry.directory)
        try{if(-not $entry.directory -and ($file.Length -ne $entry.bytes -or (HashHandle $file) -cne $entry.sha256)){throw 'Data changed after backup'}}finally{$file.Dispose()}
    }
}
function Event([string]$kind,$detail=$null){
    CheckTransition $(if($script:events.Count){$script:events[-1].kind}else{'initial'}) $kind
    $record=[ordered]@{schema=$script:plan.schema;sequence=$script:events.Count+1;previousDigest=$script:eventDigest;kind=$kind;at=[DateTimeOffset]::Now.ToString('o')}
    if($null -ne $detail){$record.detail=$detail}
    $script:eventDigest=DurableJson (Join-Path $run ('event-{0:D3}.json' -f $record.sequence)) $record
    $script:events+=([pscustomobject]$record)
}
function CheckTransition([string]$from,[string]$to){
    $allowed=@{initial=@('prepared');prepared=@('apply-intent','rollback-intent');'apply-intent'=@('old-preserved','rollback-intent');'old-preserved'=@('new-published','rollback-intent');'new-published'=@('installed-held','rollback-intent');'installed-held'=@('commit-intent','rollback-intent');'rollback-intent'=@('rollback-intent','rolled-back');'commit-intent'=@('committed','commit-abandoned');'commit-abandoned'=@('rollback-intent');'committed'=@();'rolled-back'=@()}
    if(-not $allowed.ContainsKey($from) -or $allowed[$from] -cnotcontains $to -or $script:events.Count -ge 64){throw 'Invalid transaction journal transition'}
}
function ObserveTree([string]$path){
    if(-not (Test-Path -LiteralPath $path)){return 'absent'}
    try{
        $actual=@(Tree $path) | ConvertTo-Json -Depth 8 -Compress
        if($actual -ceq (@($script:plan.oldEntries)|ConvertTo-Json -Depth 8 -Compress)){return 'source'}
        if($actual -ceq (@($script:plan.newEntries)|ConvertTo-Json -Depth 8 -Compress)){return 'candidate'}
        return 'changed'
    }catch{return 'unavailable'}
}
function ProgramGuards([string]$directory){
    $names=@('CeruleanLonging.exe','cerulean-runtime.exe','uninstall.exe')
    if(Test-Path -LiteralPath (Join-Path $directory 'cerulean-verifier.exe')){$names+=@('cerulean-verifier.exe')}
    foreach($name in $names){
        $path=Join-Path $directory $name;$guard=[CeruleanPackagePin]::WriteGuard($path)
        $owned.Add($guard);$script:guards.Add($path,$guard)
    }
}
function MoveProgram([string]$from,[string]$to){
    # Windows rejects directory rename even when an open child shares DELETE.
    # Close only our guards, attempt once, then reacquire. A concurrent launcher
    # can make either step fail; preserve the durable intent, never terminate it.
    foreach($path in @($script:guards.Keys)){
        if($path.StartsWith($from+'\',[StringComparison]::OrdinalIgnoreCase)){
            $script:guards[$path].Dispose();$script:guards.Remove($path) | Out-Null
        }
    }
    [IO.Directory]::Move($from,$to)
    ProgramGuards $to
}
function AssertVersions {
    $v=RegistryVersion
    if($v -cne $script:plan.sourceVersion -and $v -cne $script:plan.targetVersion){throw 'Registered version changed outside transaction'}
}
function ReleaseObservation {
    # Caller owns the transaction lock and original data writer lock. Never infer
    # completion from a missing hold alone, or from a pre-existing archive.
    $backupPath=Join-Path $run 'data-backup\manifest.json'
    if((HashBytes ([IO.File]::ReadAllBytes($backupPath))) -cne $script:plan.backupDigest){throw 'Backup manifest changed'}
    $baseline=ReadJson $backupPath
    $before=@($baseline.entries | ForEach-Object {$_.path.Replace('/','\')})
    $holdPath=Join-Path $script:plan.data 'upgrade-hold.json'
    $holdState='absent';$matchingArchives=@();$conflicts=@()
    if(Test-Path -LiteralPath $holdPath){
        $file=[CeruleanPackagePin]::Open($holdPath,$false)
        try{$holdState=if((HashHandle $file) -ceq $script:plan.holdDigest){'original'}else{'changed'}}finally{$file.Dispose()}
    }
    $history=Join-Path $script:plan.data 'upgrade-holds'
    if(Test-Path -LiteralPath $history){
        $pin=[CeruleanPackagePin]::Open($history,$true)
        try{
            $entries=@(Get-ChildItem -LiteralPath $history -Force)
            if($entries.Count -gt 4096){throw 'Release history exceeds review bound'}
            foreach($entry in $entries){
                $relative='upgrade-holds\'+$entry.Name
                if($before -ccontains $relative -or $entry.Name -notlike ('released-'+$script:plan.holdDigest+'-*')){continue}
                $file=[CeruleanPackagePin]::Open($entry.FullName,$false)
                try{$hash=HashHandle $file}finally{$file.Dispose()}
                if($entry.Name -cmatch ('^released-'+$script:plan.holdDigest+'-[0-9]+\.json$') -and $hash -ceq $script:plan.holdDigest){$matchingArchives+=$relative}else{$conflicts+=$relative}
            }
        }finally{$pin.Dispose()}
    }
    $outcome=if($conflicts.Count -gt 0 -or $matchingArchives.Count -gt 1 -or $holdState -eq 'changed' -or ($holdState -ne 'absent' -and $matchingArchives.Count -gt 0)){'conflict'}elseif($holdState -eq 'absent' -and $matchingArchives.Count -eq 1){'released'}elseif($holdState -eq 'original' -and $matchingArchives.Count -eq 0){'still-held'}else{'unproven'}
    return [ordered]@{outcome=$outcome;hold=$holdState;newArchives=$matchingArchives;conflictingArchives=$conflicts;holdDigest=$script:plan.holdDigest}
}
function ArchiveHold {
    # The reviewed source hold and every original data byte were checked under
    # writer.lock. Archive in this process so a dead updater cannot leave behind
    # a separately running release helper capable of changing a later decision.
    $observation=ReleaseObservation
    if($observation.outcome -cne 'still-held'){throw 'Original hold release is not exclusively pending'}
    $history=Join-Path $script:plan.data 'upgrade-holds'
    if(-not (Test-Path -LiteralPath $history)){$null=New-Item -ItemType Directory -Path $history}
    $historyPin=[CeruleanPackagePin]::Open($history,$true)
    try{
        $destination=Join-Path $history ('released-'+$script:plan.holdDigest+'-'+[DateTime]::UtcNow.Ticks+'.json')
        [IO.File]::Move((Join-Path $script:plan.data 'upgrade-hold.json'),$destination)
    }finally{$historyPin.Dispose()}
    $released=ReleaseObservation
    if($released.outcome -cne 'released'){throw 'Release result is uncertain; reconcile original evidence'}
    return $released
}
try{
  try{
    if($Action -eq 'Prepare'){
        foreach($required in @($InstalledDirectory,$DataDirectory,$HoldDigest,$SourcePackage,$SourceManifestSha256,$CandidatePackage,$CandidateManifestSha256)){if(-not $required){throw 'Prepare requires source/candidate manifests, installation, data and reviewed hold'}}
        if(Test-Path -LiteralPath $run){throw 'Transaction already exists; review it instead of recreating'}
        $install=[IO.Path]::GetFullPath($InstalledDirectory).TrimEnd('\');$data=[IO.Path]::GetFullPath($DataDirectory).TrimEnd('\')
        $source=InspectPackage $SourcePackage $SourceManifestSha256
        $target=InspectPackage $CandidatePackage $CandidateManifestSha256
        CheckResourceLayout $source $target
        if($source.capabilities.upgradeHoldSchema -ne 1 -or $target.capabilities.binaryUpdateSchema -ne 2 -or (ReaderGeneration $target) -notin @(3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18) -or $target.capabilities.dataSchema -ne 1 -or ($source.version -ne '0.1.1' -and $source.capabilities.dataSchema -ne 1) -or [version]$target.version -le [version]$source.version){throw 'Only forward updates to a reader-aware candidate with the same base data schema are supported'}
        foreach($a in @($run,$install,$data)){
            foreach($b in @($source.packageDirectory,$target.packageDirectory)){if(-not (Disjoint $a $b)){throw 'Package and mutable paths overlap'}}
        }
        if(-not (Disjoint $run $install) -or -not (Disjoint $run $data) -or -not (Disjoint $install $data) -or [IO.Path]::GetPathRoot($run) -ine [IO.Path]::GetPathRoot($install)){throw 'Transaction/install/data overlap or volumes differ'}
        PinChain (Split-Path -Parent $run);PinChain (Split-Path -Parent $install);PinChain $data
        $script:plan=[pscustomobject][ordered]@{schema=2;controllerReaderGeneration=(ReaderGeneration $target);ownerSid=$sid;transaction=$run;install=$install;data=$data;holdDigest=$HoldDigest;sourceVersion=$source.version;targetVersion=$target.version;sourceManifest=$source.manifestSha256;targetManifest=$target.manifestSha256;sourcePackage=$source.packageDirectory;candidatePackage=$target.packageDirectory;internal=[bool]$AllowUnsignedInternal;publisher=$PublisherThumbprint;backupDigest='';oldEntries=@();newEntries=@()}
        if((RegistryVersion) -cne $source.version){throw 'Installed version differs from reviewed source package'}
        $oldEntries=@(Tree $install)
        $plan.oldEntries=$oldEntries
        CheckInstalledFiles $source $oldEntries
        $probe=Join-Path $target.packageDirectory 'cerulean-runtime.exe'
        $hold=(Core $probe @('upgrade','status')).result
        $oldCore=@($source.artifacts | Where-Object {$_.path -ceq 'cerulean-runtime.exe'})[0]
        if(-not $hold.validRecord -or $hold.holdDigest -cne $HoldDigest -or $hold.review.coreSha256 -cne $oldCore.sha256){throw 'Hold is not from the reviewed installed core'}
        CheckDataFormat $source $target
        if($MaintenanceCore -or $MaintenanceCoreSha256){
            if(-not $AllowUnsignedInternal -or -not $MaintenanceCore -or -not $MaintenanceCoreSha256){throw 'Explicit maintenance helper is limited to reviewed internal validation and requires path plus digest'}
            $maintenancePath=[IO.Path]::GetFullPath($MaintenanceCore);$maintenanceHash=$MaintenanceCoreSha256
        }else{
            if($target.capabilities.maintenanceInterlockSchema -ne 1){throw 'Candidate lacks maintenance interlock; use a newer reviewed candidate'}
            $maintenancePath=$probe;$maintenanceHash=@($target.artifacts | Where-Object {$_.path -ceq 'cerulean-runtime.exe'})[0].sha256
        }
        $plan | Add-Member -NotePropertyName maintenanceInterlockSchema -NotePropertyValue 1
        $plan | Add-Member -NotePropertyName maintenanceCore -NotePropertyValue $maintenancePath
        $plan | Add-Member -NotePropertyName maintenanceCoreSha256 -NotePropertyValue $maintenanceHash
        OpenMaintenance
        $null=New-Item -ItemType Directory -Path $run
        $lock=[IO.File]::Open((Join-Path $run 'transaction.lock'),[IO.FileMode]::CreateNew,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None);$owned.Add($lock)
        $IntentDigest=DurableJson (Join-Path $run 'intent.json') $plan
        $maintenanceObservation=(Core $maintenanceBinary @('maintenance','reserve','--transaction',$run,'--intent-digest',$IntentDigest,'--hold-digest',$HoldDigest)).result
        $null=DurableJson (Join-Path $run 'maintenance-reservation.json') @{schema=1;guardDigest=$maintenanceObservation.guardDigest}
        $backup=(Core $probe @('upgrade','backup','--hold-digest',$HoldDigest,'--to',(Join-Path $run 'data-backup'))).result
        $plan.backupDigest=$backup.manifestDigest
        $maintenanceObservation=(Core $maintenanceBinary @('maintenance','check','--guard-digest',$maintenanceObservation.guardDigest)).result
        $dataLock=[CeruleanPackagePin]::ExclusiveLock((Join-Path $data 'writer.lock'));$owned.Add($dataLock)
        CheckData
        # Preserve the original uninstall program and file set; resource bytes
        # are replaced from the pinned candidate just like the two executables.
        $plan.oldEntries=@(Tree $install (Join-Path $run 'staged-program'))
        if(($plan.oldEntries | ConvertTo-Json -Depth 8 -Compress) -cne ($oldEntries | ConvertTo-Json -Depth 8 -Compress)){throw 'Source changed during stage preparation'}
        StageReviewedFiles $target (Join-Path $run 'staged-program')
        $plan.newEntries=@(Tree (Join-Path $run 'staged-program'))
        CheckStagedFiles $target $plan.oldEntries $plan.newEntries
        $script:eventDigest=DurableJson $planPath $plan
        $PlanDigest=$eventDigest;Event 'prepared'
    }else{
        if($Action -eq 'AbandonPreparation'){
            if($IntentDigest -notmatch '^[a-f0-9]{64}$'){throw 'An explicitly reviewed original intent digest is required'}
        }elseif($PlanDigest -notmatch '^[a-f0-9]{64}$'){throw 'An explicitly reviewed transaction plan digest is required'}
        # Load the pin helper without executing a candidate before its package review.
        $helper=Get-Content -LiteralPath (Join-Path $PSScriptRoot 'test-windows-package.ps1') -Raw
        $typeSource=[regex]::Match($helper,"(?s)Add-Type -TypeDefinition @'\r?\n(.*?)\r?\n'@").Groups[1].Value
        if(-not ('CeruleanPackagePin' -as [type])){if(-not $typeSource){throw 'Package pin helper unavailable'};Add-Type -TypeDefinition $typeSource}
        PinChain $run
        $owned.Add([CeruleanPackagePin]::ExclusiveLock((Join-Path $run 'transaction.lock')))
        $readPath=if($Action -eq 'AbandonPreparation'){Join-Path $run 'intent.json'}else{$planPath}
        $readDigest=if($Action -eq 'AbandonPreparation'){$IntentDigest}else{$PlanDigest}
        $plan=ReadJson $readPath
        if((HashBytes ([IO.File]::ReadAllBytes($readPath))) -cne $readDigest -or $plan.schema -notin @(1,2) -or ($plan.schema -eq 2 -and (($plan.controllerReaderGeneration -isnot [int] -and $plan.controllerReaderGeneration -isnot [long]) -or $plan.controllerReaderGeneration -notin @(3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18) -or $plan.maintenanceInterlockSchema -ne 1)) -or $plan.ownerSid -cne $sid -or $plan.transaction -ine $run){throw 'Transaction identity, controller generation or plan digest changed'}
        if($plan.schema -eq 1 -and $Action -ne 'Review'){throw 'Legacy transaction requires its original controller; no implicit maintenance adoption'}
        if($plan.schema -eq 2 -and $plan.maintenanceInterlockSchema -eq 1){OpenMaintenance;$maintenanceObservation=ObserveMaintenance}
        elseif($Action -ne 'Review'){throw 'Legacy transaction requires its original controller; no implicit maintenance adoption'}
        if($Action -ne 'Review' -and $Action -notin @('AbandonPreparation','FinishMaintenance') -and $maintenanceObservation.state -cne 'reserved'){throw 'Maintenance reservation is not active; no transaction action repeated'}
        $script:eventDigest=$PlanDigest
        foreach($file in @(Get-ChildItem -LiteralPath $run -Filter 'event-*.json' | Sort-Object Name)){
            $event=ReadJson $file.FullName
            if($event.schema -ne $plan.schema -or $event.sequence -ne $events.Count+1 -or $event.previousDigest -cne $eventDigest -or $file.Name -cne ('event-{0:D3}.json' -f $event.sequence)){throw 'Transaction journal is incomplete or changed'}
            CheckTransition $(if($events.Count){$events[-1].kind}else{'initial'}) $event.kind
            $script:events+=$event;$script:eventDigest=HashBytes ([IO.File]::ReadAllBytes($file.FullName))
        }
        if($Action -eq 'AbandonPreparation'){
            if($events.Count){throw 'Preparation has execution records; use the original plan and rollback/finish path'}
        }elseif($events.Count -eq 0 -or $events[0].kind -cne 'prepared'){throw 'Preparation is incomplete'}
        if($Action -ne 'Review'){
            if([bool]$plan.internal -ne [bool]$AllowUnsignedInternal -or $plan.publisher -cne $PublisherThumbprint){throw 'Transaction trust policy differs'}
            $source=InspectPackage $plan.sourcePackage $plan.sourceManifest
            $target=InspectPackage $plan.candidatePackage $plan.targetManifest
            if($Action -in @('Apply','Commit') -or ($Action -eq 'Rollback' -and $events.kind -cnotcontains 'commit-abandoned')){
                $maintenanceObservation=(Core $maintenanceBinary @('maintenance','check','--guard-digest',$maintenanceObservation.guardDigest)).result
            }
            PinChain (Split-Path -Parent $plan.install);PinChain $plan.data
            $dataLock=[CeruleanPackagePin]::ExclusiveLock((Join-Path $plan.data 'writer.lock'));$owned.Add($dataLock)
            AssertVersions
            CheckDataFormat $source $target
            $recovery=$Action -in @('Reconcile','AbandonCommit','FinishMaintenance','AbandonPreparation')
            $abandonedRollback=$Action -eq 'Rollback' -and $events.kind -contains 'commit-abandoned' -and $events.kind -cnotcontains 'committed'
            if(($events.kind -contains 'commit-intent' -or $events.kind -contains 'committed') -and -not $recovery -and -not $abandonedRollback){throw 'Commit has started; rollback/apply is unavailable. Review release outcome without replaying it.'}
            if($Action -ne 'AbandonPreparation'){
                $backup=(Core (Join-Path $target.packageDirectory 'cerulean-runtime.exe') @('backup','verify','--from',(Join-Path $run 'data-backup'))).result
                if($backup.manifestDigest -cne $plan.backupDigest){throw 'Verified backup digest differs from transaction'}
            }
            if(-not $recovery){CheckData -AllowEmptyReleaseDirectory:$abandonedRollback}
            if($Action -eq 'AbandonPreparation' -and (RegistryVersion) -cne $plan.sourceVersion){throw 'Original registered version changed'}
            if($Action -eq 'FinishMaintenance'){
                if($events[-1].kind -cnotin @('committed','rolled-back')){throw 'Only a terminal transaction can finish maintenance; inspect the original unresolved action'}
                $expectedVersion=if($events[-1].kind -ceq 'committed'){$plan.targetVersion}else{$plan.sourceVersion}
                if((RegistryVersion) -cne $expectedVersion){throw 'Terminal registered version differs'}
            }
        }
        $previous=Join-Path $run 'previous-program';$stage=Join-Path $run 'staged-program';$failed=Join-Path $run 'failed-program'
        if($Action -eq 'Apply'){
            if($events.Count -ne 1 -or $events[-1].kind -cne 'prepared'){throw 'Apply is single-attempt; inspect or explicitly roll back an interrupted attempt'}
            SameTree $plan.install $plan.oldEntries;SameTree $stage $plan.newEntries
            ProgramGuards $plan.install;ProgramGuards $stage
            Event 'apply-intent'
            MoveProgram $plan.install $previous
            Event 'old-preserved'
            MoveProgram $stage $plan.install
            Event 'new-published'
            Set-ItemProperty -LiteralPath $uninstallKey -Name DisplayVersion -Value $plan.targetVersion
            SameTree $plan.install $plan.newEntries
            Event 'installed-held'
        }elseif($Action -eq 'Rollback'){
            if($events.kind -contains 'rolled-back'){throw 'Already rolled back; no action repeated'}
            if(Test-Path -LiteralPath $previous){
                SameTree $previous $plan.oldEntries;ProgramGuards $previous
                if(Test-Path -LiteralPath $plan.install){SameTree $plan.install $plan.newEntries;ProgramGuards $plan.install}
                if(Test-Path -LiteralPath $failed){
                    if(Test-Path -LiteralPath $plan.install){throw 'Both current and failed programs exist; leave ambiguous state untouched'}
                    SameTree $failed $plan.newEntries
                }
                Event 'rollback-intent'
                if(Test-Path -LiteralPath $plan.install){MoveProgram $plan.install $failed}
                MoveProgram $previous $plan.install
            }else{
                SameTree $plan.install $plan.oldEntries;ProgramGuards $plan.install
                Event 'rollback-intent'
            }
            Set-ItemProperty -LiteralPath $uninstallKey -Name DisplayVersion -Value $plan.sourceVersion
            SameTree $plan.install $plan.oldEntries;Event 'rolled-back'
        }elseif($Action -eq 'Commit'){
            if($events[-1].kind -cne 'installed-held'){throw 'Only a verified installed-held transaction can commit'}
            SameTree $plan.install $plan.newEntries
            ProgramGuards $plan.install
            SameTree $plan.install $plan.newEntries
            if((RegistryVersion) -cne $plan.targetVersion){throw 'Registered version does not match candidate'}
            Event 'commit-intent' @{releaseMode='in-process-v1'}
            $releaseObservation=ArchiveHold
            Event 'committed' $releaseObservation
        }elseif($Action -in @('Reconcile','AbandonCommit')){
            if($events[-1].kind -cne 'commit-intent'){throw 'Only an unresolved commit intent can be reconciled or abandoned'}
            SameTree $plan.install $plan.newEntries
            ProgramGuards $plan.install;SameTree $plan.install $plan.newEntries
            if((RegistryVersion) -cne $plan.targetVersion){throw 'Registered version does not match candidate'}
            $releaseObservation=ReleaseObservation
            if($Action -eq 'Reconcile'){
                if($releaseObservation.outcome -cne 'released'){throw ('Release is not proven: '+$releaseObservation.outcome)}
                # Journal-only acknowledgment of original evidence; never release again.
                Event 'committed' $releaseObservation
            }else{
                if($events[-1].detail.releaseMode -cne 'in-process-v1'){throw 'Legacy release helper lifetime is unproven; do not abandon automatically'}
                if($releaseObservation.outcome -cne 'still-held'){throw ('Cannot abandon after a possible release: '+$releaseObservation.outcome)}
                CheckData -AllowEmptyReleaseDirectory
                Event 'commit-abandoned' $releaseObservation
            }
        }
    }
    if($Action -eq 'Review' -and $events[-1].kind -ceq 'commit-intent'){
        try{
            PinChain $plan.data
            $reviewLock=[CeruleanPackagePin]::ExclusiveLock((Join-Path $plan.data 'writer.lock'));$owned.Add($reviewLock)
            $releaseObservation=ReleaseObservation
        }catch{$releaseObservation=@{outcome='unavailable';reason='Data lock or original evidence could not be inspected; no outcome inferred'}}
    }
    $observed=[ordered]@{installed=(ObserveTree $plan.install);previous=(ObserveTree (Join-Path $run 'previous-program'));staged=(ObserveTree (Join-Path $run 'staged-program'));failed=(ObserveTree (Join-Path $run 'failed-program'))}
    $state=if($events.Count){$events[-1].kind}else{'preparation-incomplete'}
    $reply=[ordered]@{status='passed';action=$Action;transaction=$run;planDigest=$PlanDigest;intentDigest=$IntentDigest;state=$state;sourceVersion=$plan.sourceVersion;targetVersion=$plan.targetVersion;holdPresent=(Test-Path -LiteralPath (Join-Path $plan.data 'upgrade-hold.json'));releaseObservation=$releaseObservation;observed=$observed;events=$events;automaticServiceStart=$false;dataMigration=$false}
  }finally{
    for($i=$owned.Count-1;$i -ge 0;$i--){$owned[$i].Dispose()}
  }
    # Rust independently owns both writer and transaction locks. Keep the reviewed
    # helper/package bytes pinned, but dispose controller locks before this call.
    # If it fails, terminal evidence and reservation remain; FinishMaintenance is
    # explicit acknowledgment, never a replay of Commit/Rollback.
    if($Action -eq 'AbandonPreparation'){
        if($maintenanceObservation.state -ceq 'reserved'){$maintenanceObservation=(Core $maintenanceBinary @('maintenance','abandon','--guard-digest',$maintenanceObservation.guardDigest)).result}
    }elseif($Action -in @('Rollback','Commit','Reconcile','FinishMaintenance')){
        if($maintenanceObservation.state -ceq 'reserved'){$maintenanceObservation=(Core $maintenanceBinary @('maintenance','release','--guard-digest',$maintenanceObservation.guardDigest,'--plan-digest',$PlanDigest)).result}
    }
    $reply.maintenance=$maintenanceObservation
    $reply | ConvertTo-Json -Depth 12 -Compress
}finally{
    for($i=$packagePins.Count-1;$i -ge 0;$i--){$packagePins[$i].Dispose()}
}
