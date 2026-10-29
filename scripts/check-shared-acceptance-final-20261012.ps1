# Final checks for the missing-Accept guard after writer return/reopen.
$ErrorActionPreference='Stop'
Set-Location -LiteralPath (Split-Path -Parent $PSScriptRoot)
$env:PATH="$env:USERPROFILE\.cargo\bin;D:\qqagent\node;$env:PATH"
$env:CERULEAN_TEST_GIT=(Get-Command git.exe -ErrorAction Stop).Source
$env:CERULEAN_TEST_NODE=(Get-Command node.exe -ErrorAction Stop).Source
foreach ($stage in @('lifecycle4','core-final','clippy-final','core-build-final','desktop-final')) {
    if (Test-Path -LiteralPath "reports/p5-shared-acceptance-$stage-20261012.log") { throw 'Preserve existing final log.' }
}
if ((Get-Content -Raw reports/p5-shared-acceptance-full-20261012.log) -notmatch '297 passed; 0 failed; 144 ignored') { throw 'Initial default regression must pass.' }
if ((Get-Content -Raw reports/p5-shared-acceptance-verification-20261012.log) -notmatch '1 passed; 0 failed') { throw 'Verification regression must pass.' }
if ((Get-Content -Raw reports/p5-shared-acceptance-mixed-20261012.log) -notmatch '2 passed; 0 failed') { throw 'Mixed regression must pass.' }
$ErrorActionPreference='Continue'
cargo fmt --all
if ($LASTEXITCODE) { exit $LASTEXITCODE }
cargo test -p cerulean-core --lib shared_chief_application --locked -- --ignored --nocapture --test-threads=1 *> reports/p5-shared-acceptance-lifecycle4-20261012.log
if ($LASTEXITCODE) { Get-Content reports/p5-shared-acceptance-lifecycle4-20261012.log -Tail 30; exit $LASTEXITCODE }
cargo test -p cerulean-core --lib --locked *> reports/p5-shared-acceptance-core-final-20261012.log
if ($LASTEXITCODE) { Get-Content reports/p5-shared-acceptance-core-final-20261012.log -Tail 30; exit $LASTEXITCODE }
cargo clippy --workspace --all-targets --locked -- -D warnings *> reports/p5-shared-acceptance-clippy-final-20261012.log
if ($LASTEXITCODE) { Get-Content reports/p5-shared-acceptance-clippy-final-20261012.log -Tail 30; exit $LASTEXITCODE }
cargo build -p cerulean-core --locked *> reports/p5-shared-acceptance-core-build-final-20261012.log
if ($LASTEXITCODE) { Get-Content reports/p5-shared-acceptance-core-build-final-20261012.log -Tail 30; exit $LASTEXITCODE }
$ErrorActionPreference='Stop'
Copy-Item -LiteralPath target/debug/cerulean-core.exe -Destination target/debug/cerulean-runtime.exe -Force
Copy-Item -LiteralPath target/debug/cerulean-core.exe -Destination apps/desktop/src-tauri/binaries/cerulean-runtime-x86_64-pc-windows-msvc.exe -Force
$ErrorActionPreference='Continue'
cargo build -p cerulean-desktop --features custom-protocol --locked *> reports/p5-shared-acceptance-desktop-final-20261012.log
if ($LASTEXITCODE) { Get-Content reports/p5-shared-acceptance-desktop-final-20261012.log -Tail 30; exit $LASTEXITCODE }
Write-Output 'Final chief guard on shared, idle and reopened owners passed; no model or desktop calls.'
