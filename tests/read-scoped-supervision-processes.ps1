$ErrorActionPreference='Stop'
$projectRoot=Split-Path -Parent $PSScriptRoot
$reportPrefix=Join-Path $projectRoot 'reports\p5-scoped-supervision-'
$fixtures=@('codex_dag_fixture.exe','omp_offline_worker_fixture.exe','codex_protocol_fixture.exe') | ForEach-Object { Join-Path $projectRoot "target\debug\examples\$_" }
$fixtureMatches=@(Get-CimInstance Win32_Process | Where-Object {
    $path=[string]$_.ExecutablePath
    $command=[string]$_.CommandLine
    ($path -and $path.StartsWith($reportPrefix,[StringComparison]::OrdinalIgnoreCase)) -or
    ($command -and $command.IndexOf($reportPrefix,[StringComparison]::OrdinalIgnoreCase) -ge 0) -or
    ($path -and $path -in $fixtures)
} | Select-Object ProcessId,ParentProcessId,ExecutablePath,CommandLine)
$destination=Join-Path $projectRoot 'reports\p5-scoped-supervision-final-processes-20261013.json'
if(Test-Path -LiteralPath $destination){throw 'Preserve original process observation'}
$record=[ordered]@{schema=1;observedAt=[DateTimeOffset]::Now.ToString('o');mode='read-only CIM';scope=@($reportPrefix)+$fixtures;matches=$fixtureMatches;processesStopped=0;limits='Snapshot of the named report/fixture paths only; not proof of every historical child handle lifetime or every workspace process.'}
[IO.File]::WriteAllText($destination,($record|ConvertTo-Json -Depth 8),(New-Object Text.UTF8Encoding($false)))
Write-Output "Scoped fixture matches: $($fixtureMatches.Count); $destination"
