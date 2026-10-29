[CmdletBinding(DefaultParameterSetName='Prepare')]
param(
    [Parameter(Mandatory=$true)][string]$PreparedDirectory,
    [Parameter(Mandatory=$true)][string]$EvidenceDirectory,
    [Parameter(Mandatory=$true)][string]$TestExecutable,
    [Parameter(Mandatory=$true)][string]$ReferencePath,
    [string]$ProjectDirectory,
    [Parameter(Mandatory=$true,ParameterSetName='Prepare')][switch]$Prepare,
    [Parameter(Mandatory=$true,ParameterSetName='Live')][switch]$Live,
    [Parameter(Mandatory=$true,ParameterSetName='Live')][string]$ApprovedDigest,
    [switch]$ValidateOnly
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
$executable=Check-ReportPath $TestExecutable
$reference=Check-ReportPath $ReferencePath
if ($prepared -eq $output -or $output.StartsWith($prepared+'\',[StringComparison]::OrdinalIgnoreCase) -or $prepared.StartsWith($output+'\',[StringComparison]::OrdinalIgnoreCase)) { throw 'Keep command evidence separate from the prepared directory.' }
if (Test-Path -LiteralPath $output) { throw 'EvidenceDirectory must be new; never overwrite or retry.' }
foreach ($file in @($executable,$reference)) {
    if (-not (Test-Path -LiteralPath $file -PathType Leaf)) { throw 'Frozen executable/reference must exist.' }
}
if (-not (Test-Path -LiteralPath $prepared -PathType Container)) { throw 'Keep the existing isolated Qoder workspace; do not recreate it.' }
$project=if ($ProjectDirectory) { Check-ReportPath $ProjectDirectory } else { Check-ReportPath (Join-Path $prepared 'project') }
if (-not (Test-Path -LiteralPath $project -PathType Container)) { throw 'Existing project directory required.' }
if (@(Get-ChildItem -LiteralPath $project -Force).Count -ne 0) { throw 'Project must remain empty; never clear user files.' }
if ($Prepare) {
    $entries=@(Get-ChildItem -LiteralPath $prepared -Force)
    if ($project -eq (Join-Path $prepared 'project')) {
        if ($entries.Count -ne 1 -or $entries[0].Name -ne 'project') { throw 'Preparation requires only the original empty project directory.' }
    } elseif ($entries.Count -ne 0 -or $project.StartsWith($prepared+'\',[StringComparison]::OrdinalIgnoreCase) -or $prepared.StartsWith($project+'\',[StringComparison]::OrdinalIgnoreCase) -or $prepared -eq $project) {
        throw 'New state directory must be empty and separate from the existing project.'
    }
    $test='prepare_qoder_product'
} else {
    if ($ApprovedDigest -notmatch '^[0-9a-f]{64}$') { throw 'Explicit frozen review digest required.' }
    if (Test-Path -LiteralPath (Join-Path $prepared 'live-intent.json')) { throw 'A live attempt already exists; inspect without retrying.' }
    foreach ($file in @('review.json','review-digest.json')) {
        $reviewPath=Check-ReportPath (Join-Path $prepared $file)
        if (-not (Test-Path -LiteralPath $reviewPath -PathType Leaf)) { throw 'Read-only preparation must complete first.' }
    }
    $test='qoder_product_once'
}
$argsForTest=@('--exact',('host::desktop_sessions::tests::run_tests::product_live::qoder_probe::live::'+$test),'--ignored','--test-threads=1')
# Validation is a local path/argument check only; it never loads native UI code.
if ($ValidateOnly) {
    @{validated=$true;mode=$PSCmdlet.ParameterSetName;args=$argsForTest;launched=$false}|ConvertTo-Json -Depth 5
    return
}
$source=[IO.File]::ReadAllText((Join-Path $PSScriptRoot 'test-codex-cancel-live.ps1'))
$match=[regex]::Match($source,"(?s)Add-Type -TypeDefinition @'\r?\n(.*?)\r?\n'@")
if (-not $match.Success) { throw 'Existing owned-process runner unavailable.' }
if (-not ('CeruleanCancelLive.OwnedRunner' -as [type])) { Add-Type -TypeDefinition $match.Groups[1].Value }
$null=New-Item -ItemType Directory -Path $output
$variables=@{
    CERULEAN_QODER_PRODUCT_DIR=$prepared
    CERULEAN_QODER_PRODUCT_REFERENCE=$reference
    CERULEAN_QODER_PRODUCT_PROJECT_DIR=$project
    CERULEAN_QODER_PRODUCT_OWNED='1'
    CERULEAN_QODER_PRODUCT_APPROVED_DIGEST=$ApprovedDigest
}
$saved=@{}
$utf8=[Text.UTF8Encoding]::new($false)
function Get-ClosedFileSha([string]$Path) {
    $stream=[IO.File]::Open($Path,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::None)
    $algorithm=[Security.Cryptography.SHA256]::Create()
    try { return ([BitConverter]::ToString($algorithm.ComputeHash($stream))).Replace('-','').ToLowerInvariant() }
    finally { $algorithm.Dispose(); $stream.Dispose() }
}
$launch=@{mode=$PSCmdlet.ParameterSetName;executable=$executable;sha256=(Get-ClosedFileSha $executable);referenceSha256=(Get-ClosedFileSha $reference);args=$argsForTest;at=[DateTimeOffset]::Now.ToString('o')}
[IO.File]::WriteAllText((Join-Path $output 'intent.json'),($launch|ConvertTo-Json -Depth 8),$utf8)
try {
    foreach ($name in $variables.Keys) { $saved[$name]=[Environment]::GetEnvironmentVariable($name,'Process'); [Environment]::SetEnvironmentVariable($name,$variables[$name],'Process') }
    $result=[CeruleanCancelLive.OwnedRunner]::Run($executable,$argsForTest,$projectRoot,(Join-Path $output 'stdout.log'),(Join-Path $output 'stderr.log'),210,$false)
} finally {
    foreach ($name in $saved.Keys) { [Environment]::SetEnvironmentVariable($name,$saved[$name],'Process') }
}
[IO.File]::WriteAllText((Join-Path $output 'cleanup.json'),($result|ConvertTo-Json -Depth 10),$utf8)
$hashes=@{}
foreach ($name in @('stdout.log','stderr.log')) { $hashes[$name]=Get-ClosedFileSha (Join-Path $output $name) }
[IO.File]::WriteAllText((Join-Path $output 'closed-logs.json'),($hashes|ConvertTo-Json),$utf8)
$stdout=[IO.File]::ReadAllText((Join-Path $output 'stdout.log'))
if ($result.ExitCode -ne 0 -or $result.Error -or $result.TimedOut -or -not $result.CleanupConfirmed -or $stdout -notmatch '1 passed; 0 failed; 0 ignored') { throw 'Probe failed; preserve original evidence and do not resend.' }
$result|ConvertTo-Json -Depth 10
