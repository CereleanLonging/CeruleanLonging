param([Parameter(Mandatory=$true)][string]$ReportDirectory)
$ErrorActionPreference='Stop'
$repo=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$out=[IO.Path]::GetFullPath($ReportDirectory)
if (-not $out.StartsWith((Join-Path $repo 'reports')+'\',[StringComparison]::OrdinalIgnoreCase) -or (Test-Path -LiteralPath $out)) {throw 'Expected a new report directory'}
$null=New-Item -ItemType Directory -Path $out
. (Join-Path $repo 'scripts/omp-live-input-checks.ps1')
$cases=[Collections.Generic.List[string]]::new()
function Expect-Rejected([string]$Name,[scriptblock]$Action,[string]$Expected) {
    $caught=$null
    try { & $Action } catch { $caught=$_.Exception.Message }
    if (-not $caught -or -not $caught.Contains($Expected)) {throw "Unexpected result for $Name : $caught"}
    $cases.Add($Name)
}
$source=Join-Path $out 'synthetic-source'
$prepared=Join-Path $out 'prepared'
Expect-Rejected 'missing runtime directory' {Assert-OmpRuntimeParent (Join-Path $out 'absent-runtime')} 'runtime parent directory is missing'
Expect-Rejected 'missing source directory' {Assert-OmpCredentialSource $source $prepared} 'source directory is missing'
$null=New-Item -ItemType Directory -Path (Join-Path $source 'credentials')
Expect-Rejected 'missing credential file' {Assert-OmpCredentialSource $source $prepared} 'no DeepSeek workbench credential'
$file=Join-Path $source 'credentials/deepseek.dpapi'
[IO.File]::WriteAllBytes($file,[byte[]]::new(0))
Expect-Rejected 'empty credential envelope' {Assert-OmpCredentialSource $source $prepared} 'invalid envelope size'
[IO.File]::WriteAllBytes($file,[byte[]]::new(65537))
Expect-Rejected 'oversized credential envelope' {Assert-OmpCredentialSource $source $prepared} 'invalid envelope size'
[IO.File]::WriteAllText($file,'synthetic-not-a-key')
Assert-OmpCredentialSource $source $prepared
if ([IO.File]::ReadAllText($file) -ne 'synthetic-not-a-key') {throw 'Metadata check modified synthetic source'}
$cases.Add('valid metadata does not decrypt or rewrite envelope')
$null=New-Item -ItemType Directory -Path (Join-Path $prepared 'data') -Force
Expect-Rejected 'source equals isolated data root' {Assert-OmpCredentialSource (Join-Path $prepared 'data') $prepared} 'must differ'
Assert-OmpRuntimeParent $source
$cases.Add('existing runtime directory passes metadata gate')
# Whole wrapper must reject before it creates evidence/prepared files or launches
# any test executable; the executable path here is intentionally nonexistent.
$wrapper=Join-Path $repo 'scripts/test-omp-supervision-live.ps1'
$evidence=Join-Path $out 'must-not-be-created'
Expect-Rejected 'whole Live wrapper rejects missing source before launch' { & $wrapper -Live -PreparedDirectory $prepared -EvidenceDirectory $evidence -TestExecutable 'never-launch.exe' -ApprovedDigest ('a'*64) -CredentialSource (Join-Path $out 'absent') } 'source directory is missing'
if (Test-Path -LiteralPath $evidence) {throw 'Preflight created command evidence before rejecting'}
Expect-Rejected 'whole Prepare wrapper rejects missing runtime before launch' { & $wrapper -Prepare -PreparedDirectory (Join-Path $out 'new-prepared') -EvidenceDirectory $evidence -TestExecutable 'never-launch.exe' -OmpExecutable 'never-start.exe' -RuntimeParent (Join-Path $out 'absent') -NodeExecutable 'never-node.exe' } 'runtime parent directory is missing'
if (Test-Path -LiteralPath (Join-Path $out 'new-prepared')) {throw 'Preflight created prepared data before rejecting'}
$result=@{status='passed';cases=$cases.ToArray();testExecutableStarts=0;realModelCalls=0;userCredentialReads=0;syntheticFilesOnly=$true}
[IO.File]::WriteAllText((Join-Path $out 'result.json'),($result|ConvertTo-Json -Depth 8),[Text.UTF8Encoding]::new($false))
$result|ConvertTo-Json -Depth 8
