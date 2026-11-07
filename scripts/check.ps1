param([switch]$CacheSession)
$ErrorActionPreference='Stop'
$ceruleanRoot=Split-Path -Parent $PSScriptRoot
$env:PATH="$(Join-Path $env:USERPROFILE '.cargo\bin');$env:PATH"
Set-Location -LiteralPath $ceruleanRoot
if (-not $CacheSession) {
    node (Join-Path $PSScriptRoot 'cache-task.mjs') check
    exit $LASTEXITCODE
}
if (-not $env:CERULEAN_CACHE_SESSION -or -not $env:CARGO_TARGET_DIR) { throw 'Managed cache session required' }
cargo fmt --all -- --check
if ($LASTEXITCODE) { exit $LASTEXITCODE }
if ($env:CERULEAN_RUN_NATIVE_ACCOUNT_TESTS -eq '1') {
    & powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File tests/provision-native-preflight.ps1
    if ($LASTEXITCODE) { exit $LASTEXITCODE }
}
& powershell.exe -NoProfile -ExecutionPolicy Bypass -File tests/provision-long-path.ps1
if ($LASTEXITCODE) { exit $LASTEXITCODE }
node tests/desktop-bridge-build.mjs
if ($LASTEXITCODE) { exit $LASTEXITCODE }
node tests/desktop-draft-wait.mjs
if ($LASTEXITCODE) { exit $LASTEXITCODE }
node tests/qoder-reply-boundary.mjs
if ($LASTEXITCODE) { exit $LASTEXITCODE }
node tests/desktop-send-routing.mjs
if ($LASTEXITCODE) { exit $LASTEXITCODE }
node tests/update-reader-policy.mjs
if ($LASTEXITCODE) { exit $LASTEXITCODE }
cargo build -p cerulean-domain --bin cerulean-verifier --locked
if ($LASTEXITCODE) { exit $LASTEXITCODE }
cargo build -p cerulean-checker --locked
if ($LASTEXITCODE) { exit $LASTEXITCODE }
# Process fixtures enforce wall-clock and process-tree boundaries. Serialize
# them so concurrent hashing and native launches do not consume each other's budgets.
if ($env:GITHUB_ACTIONS -eq 'true' -and $env:CERULEAN_CI_CORE_TEST_MEMORY_GIB -eq '4') {
    & (Join-Path $ceruleanRoot '.github/scripts/precompile-core-tests.ps1')
    if ($LASTEXITCODE) { exit $LASTEXITCODE }
}
if ($env:CERULEAN_RUN_NATIVE_ACCOUNT_TESTS -eq '1') {
    & (Join-Path $PSScriptRoot 'test-recheck-accounts.ps1')
    if ($LASTEXITCODE) { exit $LASTEXITCODE }
}
cargo test --workspace --exclude cerulean-desktop --locked -- --test-threads=1
if ($LASTEXITCODE) { exit $LASTEXITCODE }
node tests/independent-checks-native.mjs (Join-Path $env:CARGO_TARGET_DIR 'debug/cerulean-verifier.exe')
if ($LASTEXITCODE) { exit $LASTEXITCODE }
cargo build -p cerulean-core --locked
if ($LASTEXITCODE) { exit $LASTEXITCODE }
node tests/codex-review-executable.mjs
if ($LASTEXITCODE) { exit $LASTEXITCODE }
node tests/qoder-upgrade-policy.mjs
if ($LASTEXITCODE) { exit $LASTEXITCODE }
New-Item -ItemType Directory -Path (Join-Path $ceruleanRoot 'target/debug') -Force | Out-Null
foreach ($program in @('cerulean-core.exe','cerulean-verifier.exe','cerulean-checker.exe')) {
    Copy-Item -LiteralPath (Join-Path $env:CARGO_TARGET_DIR ('debug/'+$program)) -Destination (Join-Path $ceruleanRoot ('target/debug/'+$program)) -Force
}
Copy-Item -LiteralPath (Join-Path $ceruleanRoot 'target/debug/cerulean-core.exe') -Destination (Join-Path $ceruleanRoot 'target/debug/cerulean-runtime.exe') -Force
$ceruleanRustInfo = rustc -vV
if ($LASTEXITCODE) { exit $LASTEXITCODE }
$ceruleanTarget = ($ceruleanRustInfo | Select-String '^host: (.+)$').Matches.Groups[1].Value
if (-not $ceruleanTarget) { throw 'Cannot determine native Rust target' }
$ceruleanSidecars = Join-Path $ceruleanRoot 'apps/desktop/src-tauri/binaries'
New-Item -ItemType Directory -Path $ceruleanSidecars -Force | Out-Null
Copy-Item -LiteralPath (Join-Path $ceruleanRoot 'target/debug/cerulean-core.exe') -Destination (Join-Path $ceruleanSidecars "cerulean-runtime-$ceruleanTarget.exe") -Force
Copy-Item -LiteralPath (Join-Path $ceruleanRoot 'target/debug/cerulean-verifier.exe') -Destination (Join-Path $ceruleanSidecars "cerulean-verifier-$ceruleanTarget.exe") -Force
Copy-Item -LiteralPath (Join-Path $ceruleanRoot 'target/debug/cerulean-checker.exe') -Destination (Join-Path $ceruleanSidecars "cerulean-checker-$ceruleanTarget.exe") -Force
cargo run -p cerulean-domain --bin export-contracts --locked
if ($LASTEXITCODE) { exit $LASTEXITCODE }
npm.cmd run build
if ($LASTEXITCODE) { exit $LASTEXITCODE }
npm.cmd run test:ipc
if ($LASTEXITCODE) { exit $LASTEXITCODE }
npm.cmd run test:service
if ($LASTEXITCODE) { exit $LASTEXITCODE }
node tests/cache-runner.mjs
exit $LASTEXITCODE
