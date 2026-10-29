param([Parameter(Mandatory=$true)][string]$Installer)
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
. (Join-Path $PSScriptRoot 'windows-package-image.ps1')
$reports=[IO.Path]::GetFullPath((Join-Path $root 'reports'))
$run=Join-Path $reports ('installer-native-'+[Guid]::NewGuid().ToString('N'))
$install=Join-Path $run 'installed app'
$data=Join-Path $run 'isolated-data'
$uninstallKey='HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\CeruleanLonging'
$productKey='HKCU:\Software\ceruleanlonging\CeruleanLonging'
foreach($key in @($uninstallKey,$productKey)) {
    if(Test-Path -LiteralPath $key){throw 'Existing product installation/settings detected; do not replace them in this test'}
}
$standard=Join-Path $env:APPDATA 'local.ceruleanlonging.workbench\workspace'
function DataFingerprint([string]$folder) {
    if(-not (Test-Path -LiteralPath $folder)){return 'absent'}
    $entries=@(Get-ChildItem -LiteralPath $folder -Recurse -File | Sort-Object FullName | ForEach-Object {
        $_.FullName.Substring($folder.Length)+':'+(Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash
    })
    $sha=[Security.Cryptography.SHA256]::Create()
    try{return [BitConverter]::ToString($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes(($entries -join "`n")))).Replace('-','').ToLowerInvariant()}finally{$sha.Dispose()}
}
$before=DataFingerprint $standard
if(Test-Path -LiteralPath (Join-Path $standard 'writer.lock')) {
    $probe=[IO.File]::Open((Join-Path $standard 'writer.lock'),[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::None)
    $probe.Dispose()
}
$null=New-Item -ItemType Directory -Path $run
$installerPath=(Resolve-Path -LiteralPath $Installer).Path
$calls=[Collections.Generic.List[object]]::new()
function RunInstaller([string]$executable,[string[]]$arguments,[bool]$success) {
    $child=Start-Process -FilePath $executable -ArgumentList $arguments -WindowStyle Hidden -PassThru
    if(-not $child.WaitForExit(60000)) {
        @{status='still-running';pid=$child.Id;executable=$executable;run=$run} | ConvertTo-Json -Compress | Write-Output
        throw 'Installer is still running; do not restart it or remove its files'
    }
    $child.Refresh()
    $code=$child.ExitCode
    $calls.Add(@{executable=[IO.Path]::GetFileName($executable);exit=$code;expectedSuccess=$success})
    if(($code -eq 0) -ne $success){throw "Unexpected installer exit: $code"}
}
function Core([string[]]$arguments) {
    $runtime=Join-Path $install 'cerulean-runtime.exe'
    $raw=& $runtime --workbench --data-dir $data @arguments
    if($LASTEXITCODE -ne 0){throw "Installed core failed: $LASTEXITCODE"}
    return ($raw | ConvertFrom-Json)
}
$coreStarted=$false
try {
    RunInstaller $installerPath @('/S','/NS',"/D=$install") $true
    foreach($file in @('CeruleanLonging.exe','cerulean-runtime.exe','uninstall.exe')) {
        if(-not (Test-Path -LiteralPath (Join-Path $install $file))){throw "Missing installed file: $file"}
    }
    foreach($file in @('CeruleanLonging.exe','cerulean-runtime.exe')) {
        $expected=if($file -eq 'CeruleanLonging.exe'){Get-NsisImageHash (Join-Path $root "target/release/$file")}else{(Get-FileHash -LiteralPath (Join-Path $root "target/release/$file")).Hash}
        if((Get-FileHash -LiteralPath (Join-Path $install $file)).Hash -ne $expected){throw 'Installed binary differs from exact packaged release image'}
    }
    $installedHash=DataFingerprint $install
    $coreStarted=$true
    $started=Core @('service','start')
    $pidBefore=$started.serverPid
    $runner=Join-Path $run 'uninstall-runner.exe'
    Copy-Item -LiteralPath (Join-Path $install 'uninstall.exe') -Destination $runner
    RunInstaller $installerPath @('/S','/NS',"/D=$install") $false
    if((Core @('status')).serverPid -ne $pidBefore){throw 'Reinstall changed the running core'}
    RunInstaller $runner @('/S',"_?=$install") $false
    if((Core @('status')).serverPid -ne $pidBefore){throw 'Uninstall changed the running core'}
    if((DataFingerprint $install) -ne $installedHash){throw 'Refused maintenance altered installed files'}
    $null=Core @('service','stop');$coreStarted=$false
    Start-Sleep -Milliseconds 300
    $dataHash=DataFingerprint $data
    RunInstaller $runner @('/S',"_?=$install") $true
    if(Test-Path -LiteralPath (Join-Path $install 'cerulean-runtime.exe')){throw 'Uninstall left runtime behind'}
    if(Test-Path -LiteralPath (Join-Path $install 'CeruleanLonging.exe')){throw 'Uninstall left UI behind'}
    if(Test-Path -LiteralPath $uninstallKey){throw 'Uninstall registration remains'}
    if((DataFingerprint $data) -ne $dataHash -or (DataFingerprint $standard) -ne $before){throw 'Uninstall modified core data'}
    # This exact product key was absent before our installation. Remove only the
    # newly created test registration, after checking it still points to our path.
    if(Test-Path -LiteralPath $productKey) {
        $registered=(Get-Item -LiteralPath $productKey).GetValue('')
        if($registered -ne $install){throw 'Product registration changed ownership; leave it untouched'}
        Remove-Item -LiteralPath $productKey -Recurse
    }
    $result=@{status='passed';run=$run;installAndUninstall=$true;busyReinstallRejected=$true;busyUninstallRejected=$true;corePidUnchanged=$true;installedHashesMatch=$true;standardDataUnchanged=$true;isolatedDataUnchanged=$true;cleanMachine=$false;realModelCalls=0;userDesktopActions=0;calls=$calls.ToArray()}
    [IO.File]::WriteAllText((Join-Path $run 'result.json'),($result|ConvertTo-Json -Depth 10),[Text.UTF8Encoding]::new($false))
    $result|ConvertTo-Json -Depth 10 -Compress
} finally {
    if($coreStarted){try{$null=Core @('service','stop')}catch{Write-Warning 'Own test core cleanup not confirmed; inspect recorded test directory'}}
}
