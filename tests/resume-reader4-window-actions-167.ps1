$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$run=Join-Path $root 'reports\reader4-window-actions-167'
$stage=Join-Path $run 'maintenance'
if(Test-Path -LiteralPath (Join-Path $run 'started.json')){throw 'Already started'}
$fixture=Get-Content -LiteralPath (Join-Path $run 'fixture.json') -Raw -Encoding UTF8|ConvertFrom-Json
$session=Join-Path $stage 'session.json'
$sessionDigest=(Get-FileHash -LiteralPath $session).Hash.ToLowerInvariant()
$config=Get-Content -LiteralPath $session -Raw -Encoding UTF8|ConvertFrom-Json
foreach($entry in $config.artifacts){if((Get-FileHash -LiteralPath (Join-Path $stage $entry.path)).Hash.ToLowerInvariant() -cne $entry.sha256){throw 'Staged source changed'}}
$source=[IO.File]::ReadAllText((Join-Path $PSScriptRoot 'check-reader4-window-actions-167.ps1'),[Text.Encoding]::UTF8)
$tail=[regex]::Match($source,'(?s)\$hook=@''.*').Value
if(-not $tail){throw 'Resume anchor missing'}
Invoke-Expression $tail
