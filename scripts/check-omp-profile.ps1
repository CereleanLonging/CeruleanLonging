param(
    [Parameter(Mandatory=$true)][string]$OmpExecutable,
    [Parameter(Mandatory=$true)][string]$ApprovedSha256,
    [Parameter(Mandatory=$true)][string]$FreshTestRoot
)
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false
$repo = Split-Path -Parent $PSScriptRoot
$probe = Join-Path $repo 'target/debug/examples/omp_owned_readonly_probe.exe'
if (-not (Test-Path -LiteralPath $probe -PathType Leaf)) { throw 'Build the readonly probe first' }
if ($ApprovedSha256 -notmatch '^[a-fA-F0-9]{64}$') { throw 'Expected an approved SHA256' }
if ((Get-FileHash -LiteralPath $OmpExecutable -Algorithm SHA256).Hash -ne $ApprovedSha256) { throw 'OMP executable changed' }
if (-not [IO.Path]::IsPathFullyQualified($FreshTestRoot)) { throw 'Test root must be absolute' }
if (Test-Path -LiteralPath $FreshTestRoot) { throw 'Test root must be new; evidence is never overwritten' }
$testParent = (Resolve-Path -LiteralPath (Split-Path -Parent $FreshTestRoot)).Path
$testRoot = Join-Path $testParent (Split-Path -Leaf $FreshTestRoot)
New-Item -ItemType Directory -Path $testRoot | Out-Null
$cases = @()
foreach ($mode in @('clean', 'agents', 'claude')) {
    $caseRoot = Join-Path $testRoot $mode
    New-Item -ItemType Directory -Path $caseRoot | Out-Null
    $marker = if ($mode -eq 'agents') { 'AGENTS.md' } elseif ($mode -eq 'claude') { 'CLAUDE.md' } else { $null }
    if ($marker) {
        [IO.File]::WriteAllText((Join-Path $caseRoot $marker), 'Synthetic ancestor context; must not be loaded.')
    }
    $profile = Join-Path $caseRoot 'profile'
    $reportPath = Join-Path $caseRoot 'report.json'
    & $probe $OmpExecutable $ApprovedSha256.ToLowerInvariant() $profile $reportPath *> (Join-Path $caseRoot 'probe.log')
    $probeExit = $LASTEXITCODE
    $report = Get-Content -LiteralPath $reportPath -Raw -Encoding UTF8 | ConvertFrom-Json
    if ($report.modelPrompts -ne 0 -or $report.realCredentialsRead -ne $false) { throw 'Unexpected live probe scope' }
    if ($marker) {
        if ($probeExit -eq 0 -or $report.status -ne 'failed' -or $null -ne $report.pid -or
            -not $report.error.Contains("contains $marker") -or (Test-Path -LiteralPath $profile)) {
            throw "Ancestor rejection failed: $mode"
        }
        if ([IO.File]::ReadAllText((Join-Path $caseRoot $marker)) -ne 'Synthetic ancestor context; must not be loaded.') {
            throw 'Ancestor marker changed'
        }
    } elseif ($probeExit -ne 0 -or $report.status -ne 'passed' -or $report.exit.remainingProcesses -ne 0 -or
        $report.exit.exitCode -ne 0 -or $report.exit.forced -ne $false) {
        throw 'Clean profile launch/handshake/cleanup failed'
    }
    $cases += @{ mode=$mode; passed=$true; probeExit=$probeExit; report=$reportPath }
}
$result = @{
    version=1; status='passed'; at=(Get-Date -Format o); cases=$cases
    approvedSha256=$ApprovedSha256.ToLowerInvariant(); realModelPrompts=0; realCredentialsRead=$false
    ordinaryUserDirectoryAccepted=$false; evidenceRetained=$testRoot
}
$result | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $testRoot 'result.json') -Encoding UTF8
$result | ConvertTo-Json -Depth 8
# Expected negative probes return nonzero; do not leak their exit status.
exit 0
