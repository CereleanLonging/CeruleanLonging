$ErrorActionPreference='Stop'
$root=Split-Path $PSScriptRoot -Parent
$batch=Join-Path $root 'reports/scoped-ledger-177'
if(-not [IO.Directory]::Exists($batch)){throw 'Batch 177 evidence directory is missing'}
$dir=Join-Path $batch ('ui-owned-'+[Guid]::NewGuid().ToString('N'))
if([IO.Directory]::Exists($dir)){throw 'Preserve existing evidence directory'}
[IO.Directory]::CreateDirectory($dir)|Out-Null
$sourcePath=Join-Path $root 'scripts/test-codex-cancel-live.ps1'
$source=[IO.File]::ReadAllText($sourcePath,[Text.Encoding]::UTF8)
$match=[regex]::Match($source,"(?s)Add-Type -TypeDefinition @'\r?\n(.*?)\r?\n'@")
if(-not $match.Success){throw 'Owned runner class missing'}
if(-not ('CeruleanCancelLive.OwnedRunner' -as [type])){Add-Type -TypeDefinition $match.Groups[1].Value}
function Save-Json([string]$Path,$Value){
  $bytes=[Text.UTF8Encoding]::new($false).GetBytes(($Value|ConvertTo-Json -Depth 15))
  $stream=[IO.File]::Open($Path,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::Read)
  try{$stream.Write($bytes,0,$bytes.Length);$stream.Flush($true)}finally{$stream.Dispose()}
}
$savedJob=$env:CERULEAN_NATIVE_OWNED_JOB
$savedRun=$env:CERULEAN_SCOPED_UI_RUN_DIR
$node='D:\qqagent\node\node.exe'
$test='tests/scoped-currency-ui-177.mjs'
Save-Json (Join-Path $dir 'started.json') ([ordered]@{
  startedAt=[DateTime]::UtcNow.ToString('o');node=$node;test=$test;root=$root
  runnerSource=$sourcePath;runnerSha256=(Get-FileHash -LiteralPath $sourcePath -Algorithm SHA256).Hash.ToLowerInvariant()
  testSha256=(Get-FileHash -LiteralPath (Join-Path $root $test) -Algorithm SHA256).Hash.ToLowerInvariant()
  noNativeIpc=$true;noModelCalls=$true;privateJobRequired=$true
})
try{
  $env:CERULEAN_NATIVE_OWNED_JOB='1'
  $env:CERULEAN_SCOPED_UI_RUN_DIR=$dir
  $result=[CeruleanCancelLive.OwnedRunner]::Run($node,@($test),$root,(Join-Path $dir 'stdout.log'),(Join-Path $dir 'stderr.log'),210,$false)
}finally{
  $env:CERULEAN_NATIVE_OWNED_JOB=$savedJob
  $env:CERULEAN_SCOPED_UI_RUN_DIR=$savedRun
}
Save-Json (Join-Path $dir 'cleanup.json') $result
# OwnedRunner returns after process/Job cleanup and closing both inherited log handles.
$streams=@()
foreach($name in @('stdout.log','stderr.log')){
  $path=Join-Path $dir $name
  $pin=[IO.File]::Open($path,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::None)
  try{$streams+=@{path=$path;bytes=$pin.Length;exclusiveReadAfterReturn=$true}}finally{$pin.Dispose()}
}
Save-Json (Join-Path $dir 'completed.json') ([ordered]@{
  completedAt=[DateTime]::UtcNow.ToString('o');outcome=$result;logWritersClosed=$true;streams=$streams
  stdoutSha256=(Get-FileHash -LiteralPath (Join-Path $dir 'stdout.log') -Algorithm SHA256).Hash.ToLowerInvariant()
  stderrSha256=(Get-FileHash -LiteralPath (Join-Path $dir 'stderr.log') -Algorithm SHA256).Hash.ToLowerInvariant()
})
$result|ConvertTo-Json -Depth 10
Write-Output ('artifact='+$dir)
if($result.ExitCode -ne 0 -or $result.Error -or $result.CleanupError -or $result.TimedOut -or -not $result.CleanupConfirmed -or $result.RemainingProcesses -ne 0){exit 1}
