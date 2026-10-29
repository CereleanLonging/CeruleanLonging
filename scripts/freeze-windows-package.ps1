param(
    [Parameter(Mandatory=$true)][string]$Installer,
    [Parameter(Mandatory=$true)][string]$OutputDirectory
)
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
. (Join-Path $PSScriptRoot 'windows-package-image.ps1')
$target=[IO.Path]::GetFullPath($OutputDirectory)
$allowed=[IO.Path]::GetFullPath((Join-Path $root 'reports'))+[IO.Path]::DirectorySeparatorChar
if (-not $target.StartsWith($allowed,[StringComparison]::OrdinalIgnoreCase)) { throw 'Package archive must be inside workspace reports' }
if (Test-Path -LiteralPath $target) { throw 'Preserve existing package archive' }
$installerPath=(Resolve-Path -LiteralPath $Installer).Path
if ([IO.Path]::GetExtension($installerPath) -ne '.exe') { throw 'Expected NSIS executable' }
$config=Get-Content -LiteralPath (Join-Path $root 'apps/desktop/src-tauri/tauri.conf.json') -Raw | ConvertFrom-Json
$package=Get-Content -LiteralPath (Join-Path $root 'package.json') -Raw | ConvertFrom-Json
if ($config.version -ne $package.version) { throw 'Product versions differ' }
$infoRaw=& (Join-Path $root 'target/release/cerulean-core.exe') --build-info
if($LASTEXITCODE -ne 0){throw 'Core build information unavailable; do not infer upgrade support'}
$buildInfo=$infoRaw | ConvertFrom-Json
if($buildInfo.capabilitySchema -ne 1 -or $buildInfo.product -ne $config.productName -or $buildInfo.version -ne $config.version -or $buildInfo.os -ne 'windows' -or $buildInfo.architecture -ne 'x86_64' -or $buildInfo.environment -ne 'msvc' -or $buildInfo.upgradeHoldSchema -ne 1){throw 'Core capabilities or build version do not match package'}
$inputs=@(
    @{name='setup.exe';path=$installerPath},
    @{name='CeruleanLonging.exe';path=(Join-Path $root 'target/release/CeruleanLonging.exe')},
    @{name='cerulean-runtime.exe';path=(Join-Path $root 'target/release/cerulean-runtime.exe')}
)
$coreHash=(Get-FileHash -LiteralPath (Join-Path $root 'target/release/cerulean-core.exe') -Algorithm SHA256).Hash.ToLowerInvariant()
foreach ($runtime in @('target/release/cerulean-runtime.exe','apps/desktop/src-tauri/binaries/cerulean-runtime-x86_64-pc-windows-msvc.exe')) {
    if ((Get-FileHash -LiteralPath (Join-Path $root $runtime) -Algorithm SHA256).Hash.ToLowerInvariant() -ne $coreHash) { throw 'Packaged core and compiled core differ' }
}
$null=New-Item -ItemType Directory -Path $target
$artifacts=@()
foreach ($inputFile in $inputs) {
    $destination=Join-Path $target $inputFile.name
    if($inputFile.name -eq 'CeruleanLonging.exe') {
        [IO.File]::WriteAllBytes($destination,(Get-NsisImageBytes $inputFile.path))
        $expected=Get-NsisImageHash $inputFile.path
    }else{
        Copy-Item -LiteralPath $inputFile.path -Destination $destination
        $expected=(Get-FileHash -LiteralPath $inputFile.path -Algorithm SHA256).Hash.ToLowerInvariant()
    }
    $hash=(Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($hash -ne $expected) { throw 'Archive copy differs' }
    $signature=Get-AuthenticodeSignature -LiteralPath $destination
    $artifacts+=@{path=$inputFile.name;bytes=(Get-Item -LiteralPath $destination).Length;sha256=$hash;authenticodeStatus=$signature.Status.ToString();signerThumbprint=if($signature.SignerCertificate){$signature.SignerCertificate.Thumbprint}else{$null}}
}
$sources=@()
foreach ($name in @('Cargo.lock','Cargo.toml','package-lock.json','package.json','apps/desktop/src-tauri/tauri.conf.json','apps/desktop/src-tauri/installer/hooks.nsh','scripts/desktop.mjs','scripts/windows-package-image.ps1','scripts/manage-windows-update.ps1','scripts/maintenance-window.ps1','scripts/test-windows-package.ps1','scripts/sqlite-notices.py','third-party/SQLITE-NOTICES.txt','third-party/sqlite-notices.json','scripts/distribution-notices.py','scripts/distribution-build-inputs.py','third-party/distribution-build-inputs.json','rust-toolchain.toml','.cargo/config.toml','third-party/distribution-inputs.json','third-party/distribution-inputs.zip')) {
    $sources+=@{path=$name;sha256=(Get-FileHash -LiteralPath (Join-Path $root $name) -Algorithm SHA256).Hash.ToLowerInvariant()}
}
$resources=@()
foreach($name in @('SQLITE-NOTICES.txt','THIRD-PARTY-NOTICES.txt','RUST-LIBRARY-COPYRIGHT.html','THIRD-PARTY-SOURCES.zip','THIRD-PARTY-MANIFEST.json')){
    $notice=Join-Path $root ('third-party/'+$name)
    $noticeDestination=Join-Path $target $name
    Copy-Item -LiteralPath $notice -Destination $noticeDestination
    $noticeHash=(Get-FileHash -LiteralPath $noticeDestination -Algorithm SHA256).Hash.ToLowerInvariant()
    if($noticeHash -cne (Get-FileHash -LiteralPath $notice -Algorithm SHA256).Hash.ToLowerInvariant()){throw 'Notice copy differs'}
    $resources+=@{path=$name;bytes=(Get-Item -LiteralPath $noticeDestination).Length;sha256=$noticeHash}
}
$manifest=[ordered]@{
    schema=1
    product=$config.productName
    identifier=$config.identifier
    version=$config.version
    target='x86_64-pc-windows-msvc'
    createdAt=[DateTimeOffset]::Now.ToString('o')
    channel='internal-validation'
    releaseAccepted=$false
    acceptanceLimitations=@('Full P3/P4/P5/P6 gates remain incomplete','Signature status is measured per artifact; hashes are not publisher signatures','No cross-version or schema migration is implied')
    dataLocation='%APPDATA%\local.ceruleanlonging.workbench\workspace'
    downgradeAllowed=$config.bundle.windows.allowDowngrades
    coreSha256=$coreHash
    coreBuildInfo=$buildInfo
    guiBundleMarker='Tauri unique TAURI_BUNDLE_TYPE_VAR_ marker set to NSS; exact installed payload verified separately'
    artifacts=$artifacts
    resources=$resources
    buildInputs=$sources
}
$json=$manifest | ConvertTo-Json -Depth 12
[IO.File]::WriteAllText((Join-Path $target 'manifest.json'),$json,[Text.UTF8Encoding]::new($false))
$manifestHash=(Get-FileHash -LiteralPath (Join-Path $target 'manifest.json') -Algorithm SHA256).Hash.ToLowerInvariant()
@{archive=$target;manifestSha256=$manifestHash;artifacts=$artifacts;releaseAccepted=$false} | ConvertTo-Json -Depth 8 -Compress
