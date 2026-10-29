$OutputEncoding=[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$run=Join-Path $root ('reports\reader5-window-170-preview-'+[Guid]::NewGuid().ToString('N'))
$null=New-Item -ItemType Directory -Path $run
$frozen=Join-Path $root 'reports\maintenance-workflow-32550bdb742b41aa87da189fcbc9b5ce\maintenance\session.json'
$results=@()
foreach($mode in @('program','data')){
    $stage=Join-Path $run $mode;$null=New-Item -ItemType Directory -Path $stage
    $config=Get-Content -LiteralPath $frozen -Raw -Encoding UTF8|ConvertFrom-Json
    $config.directory=$stage;$config.version=(Get-Content -LiteralPath (Join-Path $root 'package.json') -Raw -Encoding UTF8|ConvertFrom-Json).version
    foreach($entry in $config.artifacts){
        $target=Join-Path $stage $entry.path
        if($entry.path.EndsWith('.ps1')){[IO.File]::WriteAllText($target,[IO.File]::ReadAllText((Join-Path (Join-Path $root 'scripts') $entry.path),[Text.Encoding]::UTF8),[Text.UTF8Encoding]::new($true))}
        else{Copy-Item -LiteralPath (Join-Path $root 'target\debug\cerulean-core.exe') -Destination $target}
        $entry.sha256=(Get-FileHash -LiteralPath $target -Algorithm SHA256).Hash.ToLowerInvariant();$entry.bytes=(Get-Item -LiteralPath $target).Length
    }
    $session=Join-Path $stage 'session.json'
    [IO.File]::WriteAllText($session,($config|ConvertTo-Json -Depth 10),[Text.UTF8Encoding]::new($false))
    $digest=(Get-FileHash -LiteralPath $session -Algorithm SHA256).Hash.ToLowerInvariant()
    $arguments=@('-NoProfile','-ExecutionPolicy','RemoteSigned','-File',(Join-Path $stage 'maintenance-window.ps1'),'-Session',$session,'-SessionDigest',$digest,'-Describe')
    if($mode -eq 'data'){$arguments+='-DataMaintenance'}
    $oldPreference=$ErrorActionPreference;$ErrorActionPreference='Continue'
    try{$raw=& (Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe') @arguments 2>&1;$code=$LASTEXITCODE}finally{$ErrorActionPreference=$oldPreference}
    [IO.File]::WriteAllText((Join-Path $stage 'describe.log'),($raw -join "`n"),[Text.UTF8Encoding]::new($false))
    if($code -ne 0){throw "Describe failed for $mode; inspect original log"}
    $value=($raw -join "`n")|ConvertFrom-Json
    $expectedActions=if($mode -eq 'data'){26}else{16};$expectedGates=if($mode -eq 'data'){13}else{9}
    if($value.actions.Count -ne $expectedActions -or $value.confirmationGateCases -ne $expectedGates -or $value.automaticActions -ne 0){throw 'Unexpected control or consent count'}
    $results+=@{mode=$mode;description=$value}
}
$result=@{status='passed';run=$run;results=$results;modelCalls=0;userDesktopActions=0}
[IO.File]::WriteAllText((Join-Path $run 'result.json'),($result|ConvertTo-Json -Depth 12),[Text.UTF8Encoding]::new($false))
$result|ConvertTo-Json -Depth 12 -Compress
