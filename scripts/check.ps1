$ErrorActionPreference='Stop'
$ceruleanRoot=Split-Path -Parent $PSScriptRoot
$env:PATH="$(Join-Path $env:USERPROFILE '.cargo\bin');$env:PATH"
Set-Location -LiteralPath $ceruleanRoot
cargo fmt --all -- --check
if ($LASTEXITCODE) { exit $LASTEXITCODE }
cargo test --workspace --exclude cerulean-desktop --locked
if ($LASTEXITCODE) { exit $LASTEXITCODE }
cargo build -p cerulean-domain --bin cerulean-verifier --locked
if ($LASTEXITCODE) { exit $LASTEXITCODE }
cargo build -p cerulean-core --locked
if ($LASTEXITCODE) { exit $LASTEXITCODE }
Copy-Item -LiteralPath (Join-Path $ceruleanRoot 'target/debug/cerulean-core.exe') -Destination (Join-Path $ceruleanRoot 'target/debug/cerulean-runtime.exe') -Force
$ceruleanRustInfo = rustc -vV
if ($LASTEXITCODE) { exit $LASTEXITCODE }
$ceruleanTarget = ($ceruleanRustInfo | Select-String '^host: (.+)$').Matches.Groups[1].Value
if (-not $ceruleanTarget) { throw 'Cannot determine native Rust target' }
$ceruleanSidecars = Join-Path $ceruleanRoot 'apps/desktop/src-tauri/binaries'
New-Item -ItemType Directory -Path $ceruleanSidecars -Force | Out-Null
Copy-Item -LiteralPath (Join-Path $ceruleanRoot 'target/debug/cerulean-core.exe') -Destination (Join-Path $ceruleanSidecars "cerulean-runtime-$ceruleanTarget.exe") -Force
Copy-Item -LiteralPath (Join-Path $ceruleanRoot 'target/debug/cerulean-verifier.exe') -Destination (Join-Path $ceruleanSidecars "cerulean-verifier-$ceruleanTarget.exe") -Force
cargo run -p cerulean-domain --bin export-contracts --locked
if ($LASTEXITCODE) { exit $LASTEXITCODE }
npm.cmd run build
if ($LASTEXITCODE) { exit $LASTEXITCODE }
npm.cmd run test:ipc
if ($LASTEXITCODE) { exit $LASTEXITCODE }
npm.cmd run test:service
exit $LASTEXITCODE
