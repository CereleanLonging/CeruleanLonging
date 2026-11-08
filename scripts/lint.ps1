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
    & powershell.exe -NoProfile -ExecutionPolicy RemoteSigned -File (Join-Path $ceruleanRoot '.github/scripts/standard-test-token.ps1')
} else {
    cargo test -p cerulean-desktop --features custom-protocol --locked
}
if ($LASTEXITCODE) { exit $LASTEXITCODE }
if ((Get-CoreDigest $core) -ne $before) { throw 'Desktop build replaced Cargo core binary' }
Copy-Item -LiteralPath $core -Destination (Join-Path $ceruleanRoot 'target/debug/cerulean-core.exe') -Force
npm.cmd run test:ipc
exit $LASTEXITCODE
