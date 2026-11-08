param([switch]$CacheSession)
$ErrorActionPreference='Stop'
$ceruleanRoot=Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $ceruleanRoot
if (-not $CacheSession) {
    node (Join-Path $PSScriptRoot 'cache-task.mjs') lint
    exit $LASTEXITCODE
}
if (-not $env:CERULEAN_CACHE_SESSION) { throw 'Managed cache session required' }
cargo build -p cerulean-core --locked
if ($LASTEXITCODE) { exit $LASTEXITCODE }
$core=Join-Path $env:CARGO_TARGET_DIR 'debug/cerulean-core.exe'
function Get-CoreDigest([string]$path) {
    $stream=[IO.File]::OpenRead($path)
    $sha=[Security.Cryptography.SHA256]::Create()
    try { [BitConverter]::ToString($sha.ComputeHash($stream)).Replace('-','') }
    finally { $sha.Dispose();$stream.Dispose() }
}
$before=Get-CoreDigest $core
cargo clippy --workspace --all-targets --keep-going --features custom-protocol --locked -- -D warnings
if ($LASTEXITCODE) { exit $LASTEXITCODE }
if ($env:GITHUB_ACTIONS -eq 'true' -and $env:RUNNER_ENVIRONMENT -eq 'github-hosted') {
    $desktopArtifacts=Join-Path $env:CERULEAN_CACHE_SESSION 'desktop-test-artifacts.jsonl'
    cargo test -p cerulean-desktop --features custom-protocol --locked --no-run --message-format=json-render-diagnostics | Set-Content -LiteralPath $desktopArtifacts -Encoding utf8
    if ($LASTEXITCODE) { exit $LASTEXITCODE }
    $records=@(Get-Content -LiteralPath $desktopArtifacts | ForEach-Object { $_ | ConvertFrom-Json })
    if($records[-1].reason -ne 'build-finished' -or -not $records[-1].success){throw 'Successful desktop test build required'}
    $tests=@($records | Where-Object { $_.reason -eq 'compiler-artifact' -and $_.profile.test -and $_.executable })
    if(@($tests | Where-Object {$_.target.name -eq 'file_import'}).Count -ne 1){throw 'Exactly one native file-import test executable required'}
    $targetRoot=$env:CARGO_TARGET_DIR
    if($targetRoot.StartsWith('\\?\')){$targetRoot=$targetRoot.Substring(4)}
    foreach($test in $tests) {
        $testPath=[IO.Path]::GetFullPath($test.executable)
        if($testPath.StartsWith('\\?\')){$testPath=$testPath.Substring(4)}
        if(-not $testPath.StartsWith([IO.Path]::GetFullPath($targetRoot)+'\',[StringComparison]::OrdinalIgnoreCase)){throw 'Unexpected desktop test artifact directory'}
        if($test.target.name -eq 'file_import'){
            & powershell.exe -NoProfile -ExecutionPolicy RemoteSigned -File (Join-Path $ceruleanRoot '.github/scripts/standard-test-token.ps1') -TestExecutable $testPath
        } else { & $testPath }
        if ($LASTEXITCODE) { exit $LASTEXITCODE }
    }
} else {
    cargo test -p cerulean-desktop --features custom-protocol --locked
}
if ($LASTEXITCODE) { exit $LASTEXITCODE }
if ((Get-CoreDigest $core) -ne $before) { throw 'Desktop build replaced Cargo core binary' }
Copy-Item -LiteralPath $core -Destination (Join-Path $ceruleanRoot 'target/debug/cerulean-core.exe') -Force
npm.cmd run test:ipc
exit $LASTEXITCODE
