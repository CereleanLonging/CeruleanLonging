$ErrorActionPreference='Stop'
$root=Split-Path $PSScriptRoot -Parent
$dir=Join-Path $root ('reports/workspace-api-publication-reviews-165/ui-owned-'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($dir)|Out-Null
$source=[IO.File]::ReadAllText((Join-Path $root 'scripts/test-codex-cancel-live.ps1'))
$match=[regex]::Match($source,"(?s)Add-Type -TypeDefinition @'\r?\n(.*?)\r?\n'@")
if(-not $match.Success){throw 'Owned runner class missing'}
Add-Type -TypeDefinition $match.Groups[1].Value
$saved=$env:CERULEAN_NATIVE_OWNED_JOB
try {
  $env:CERULEAN_NATIVE_OWNED_JOB='1'
  $result=[CeruleanCancelLive.OwnedRunner]::Run((Get-Command node.exe).Source,@('tests/workspace-api-publication-reviews-ui-165.mjs'),$root,(Join-Path $dir 'stdout.log'),(Join-Path $dir 'stderr.log'),210,$false)
} finally {$env:CERULEAN_NATIVE_OWNED_JOB=$saved}
[IO.File]::WriteAllText((Join-Path $dir 'cleanup.json'),($result|ConvertTo-Json -Depth 10),[Text.UTF8Encoding]::new($false))
$result|ConvertTo-Json -Depth 10
Write-Output ('artifact='+$dir)
if($result.ExitCode -ne 0 -or $result.Error -or $result.TimedOut -or -not $result.CleanupConfirmed){exit 1}
