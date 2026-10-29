[CmdletBinding(DefaultParameterSetName='Prepare')]
param(
    [Parameter(Mandatory=$true)][string]$PreparedDirectory,
    [Parameter(Mandatory=$true)][string]$EvidenceDirectory,
    [Parameter(Mandatory=$true)][string]$TestExecutable,
    [Parameter(Mandatory=$true,ParameterSetName='Prepare')][switch]$Prepare,
    [Parameter(Mandatory=$true,ParameterSetName='Prepare')][string]$OmpExecutable,
    [Parameter(Mandatory=$true,ParameterSetName='Prepare')][string]$RuntimeParent,
    [Parameter(Mandatory=$true,ParameterSetName='Prepare')][string]$NodeExecutable,
    [Parameter(Mandatory=$true,ParameterSetName='Live')][switch]$Live,
    [Parameter(Mandatory=$true,ParameterSetName='Live')][string]$ApprovedDigest,
    [Parameter(Mandatory=$true,ParameterSetName='Live')][string]$CredentialSource
)
$ErrorActionPreference='Stop'
$projectRoot=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$reports=Join-Path $projectRoot 'reports'
function Check-ReportPath([string]$Path) {
    $full=[IO.Path]::GetFullPath($Path)
    if (-not $full.StartsWith($reports.TrimEnd('\')+'\',[StringComparison]::OrdinalIgnoreCase)) { throw 'Use an isolated repository reports directory.' }
    $probe=$full
    while ($probe) {
        if (Test-Path -LiteralPath $probe) {
            $item=Get-Item -LiteralPath $probe -Force
            if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw 'Linked paths are not supported.' }
        }
        $probe=[IO.Path]::GetDirectoryName($probe)
    }
    return $full
}
$prepared=Check-ReportPath $PreparedDirectory
$output=Check-ReportPath $EvidenceDirectory
. (Join-Path $PSScriptRoot 'omp-live-input-checks.ps1')
if ($prepared -eq $output -or $output.StartsWith($prepared+'\',[StringComparison]::OrdinalIgnoreCase)) { throw 'Keep command evidence outside the prepared directory.' }
if (Test-Path -LiteralPath $output) { throw 'EvidenceDirectory must be new; never overwrite or retry.' }
if ($Prepare) {
    Assert-OmpRuntimeParent $RuntimeParent
    if (Test-Path -LiteralPath $prepared) { throw 'PreparedDirectory must be new.' }
    $null=New-Item -ItemType Directory -Path $prepared
    $test='prepare_native_supervision'
} else {
    if (Test-Path -LiteralPath (Join-Path $prepared 'live-intent.json')) { throw 'A live attempt already exists; inspect it without retrying.' }
    Assert-OmpCredentialSource $CredentialSource $prepared
    $test='native_supervision_once'
}
$null=New-Item -ItemType Directory -Path $output
$source=[IO.File]::ReadAllText((Join-Path $PSScriptRoot 'test-codex-cancel-live.ps1'))
$match=[regex]::Match($source,"(?s)Add-Type -TypeDefinition @'\r?\n(.*?)\r?\n'@")
if (-not $match.Success) { throw 'Existing owned-process runner unavailable.' }
if (-not ('CeruleanCancelLive.OwnedRunner' -as [type])) { Add-Type -TypeDefinition $match.Groups[1].Value }
$variables=@{
    CERULEAN_OMP_SUPERVISION_DIR=$prepared
    CERULEAN_OMP_SUPERVISION_OWNED='1'
    CERULEAN_OMP_SUPERVISION_APPROVED_DIGEST=$ApprovedDigest
    CERULEAN_OMP_CREDENTIAL_SOURCE=$CredentialSource
    CERULEAN_OMP_EXE=$OmpExecutable
    CERULEAN_OMP_RUNTIME_PARENT=$RuntimeParent
    CERULEAN_OMP_NODE=$NodeExecutable
}
$saved=@{}
$executable=[IO.Path]::GetFullPath($TestExecutable)
$argsForTest=@('--exact',('host::omp_supervision_requests::tests::live::'+$test),'--ignored','--test-threads=1')
$utf8=[Text.UTF8Encoding]::new($false)
function Get-ClosedFileSha([string]$Path) {
    $stream=[IO.File]::Open($Path,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::None)
    $algorithm=[Security.Cryptography.SHA256]::Create()
    try { return ([BitConverter]::ToString($algorithm.ComputeHash($stream))).Replace('-','').ToLowerInvariant() }
    finally { $algorithm.Dispose(); $stream.Dispose() }
}
$launch=@{mode=$PSCmdlet.ParameterSetName;executable=$executable;sha256=(Get-ClosedFileSha $executable);args=$argsForTest;at=[DateTimeOffset]::Now.ToString('o')}
[IO.File]::WriteAllText((Join-Path $output 'intent.json'),($launch|ConvertTo-Json -Depth 8),$utf8)
try {
    foreach ($name in $variables.Keys) { $saved[$name]=[Environment]::GetEnvironmentVariable($name,'Process'); [Environment]::SetEnvironmentVariable($name,$variables[$name],'Process') }
    $result=[CeruleanCancelLive.OwnedRunner]::Run($executable,$argsForTest,$projectRoot,(Join-Path $output 'stdout.log'),(Join-Path $output 'stderr.log'),210,$false)
} finally {
    foreach ($name in $saved.Keys) { [Environment]::SetEnvironmentVariable($name,$saved[$name],'Process') }
}
[IO.File]::WriteAllText((Join-Path $output 'cleanup.json'),($result|ConvertTo-Json -Depth 10),$utf8)
# Sharing.None verifies there are no inherited writers left after original handle cleanup.
$hashes=@{}
foreach ($name in @('stdout.log','stderr.log')) {
    $hashes[$name]=Get-ClosedFileSha (Join-Path $output $name)
}
[IO.File]::WriteAllText((Join-Path $output 'closed-logs.json'),($hashes|ConvertTo-Json),$utf8)
$stdout=[IO.File]::ReadAllText((Join-Path $output 'stdout.log'))
if ($result.ExitCode -ne 0 -or $result.Error -or $result.TimedOut -or -not $result.CleanupConfirmed -or $stdout -notmatch '1 passed; 0 failed; 0 ignored') { throw 'Probe failed; preserve original evidence and do not resend.' }
$result|ConvertTo-Json -Depth 10
