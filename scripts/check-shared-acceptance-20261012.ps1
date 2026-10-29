$ErrorActionPreference='Stop'
$ceruleanRoot=Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $ceruleanRoot
$env:PATH="$env:USERPROFILE\.cargo\bin;D:\qqagent\node;$env:PATH"
$env:CERULEAN_TEST_GIT=(Get-Command git.exe -ErrorAction Stop).Source
$env:CERULEAN_TEST_NODE=(Get-Command node.exe -ErrorAction Stop).Source
$env:CERULEAN_OMP_TEST_PARENT='D:\test'
$env:CERULEAN_OMP_FIXTURE_EXE=Join-Path $ceruleanRoot 'target/debug/examples/omp_offline_worker_fixture.exe'
$env:CERULEAN_DAG_EXE=Join-Path $ceruleanRoot 'target/debug/examples/codex_dag_fixture.exe'
$env:CERULEAN_MIXED_DIR=Join-Path $ceruleanRoot 'reports/p5-shared-acceptance-cases-20261012'
if ((Get-Content -Raw reports/p5-shared-acceptance-lifecycle2-20261012.log) -notmatch '1 passed; 0 failed') { throw 'Chief lifecycle must pass first.' }
if ((Get-Content -Raw reports/p5-shared-acceptance-runner-20261012.log) -notmatch '4 passed; 0 failed') { throw 'Automatic runner must pass first.' }
foreach ($stage in @('lifecycle3','full','verification','mixed','clippy','desktop')) {
    if (Test-Path -LiteralPath "reports/p5-shared-acceptance-$stage-20261012.log") { throw 'Preserve prior log.' }
}
if (Test-Path -LiteralPath $env:CERULEAN_MIXED_DIR) { throw 'Preserve prior mixed evidence.' }
New-Item -ItemType Directory -Path $env:CERULEAN_MIXED_DIR | Out-Null
$ErrorActionPreference='Continue'
cargo fmt --all
if ($LASTEXITCODE) { exit $LASTEXITCODE }
cargo test -p cerulean-core --lib shared_chief_application --locked -- --ignored --nocapture --test-threads=1 *> reports/p5-shared-acceptance-lifecycle3-20261012.log
if ($LASTEXITCODE) { Get-Content reports/p5-shared-acceptance-lifecycle3-20261012.log -Tail 30; exit $LASTEXITCODE }
powershell.exe -NoProfile -ExecutionPolicy RemoteSigned -File scripts/check.ps1 *> reports/p5-shared-acceptance-full-20261012.log
if ($LASTEXITCODE) { Get-Content reports/p5-shared-acceptance-full-20261012.log -Tail 30; exit $LASTEXITCODE }
cargo test -p cerulean-core --lib shared_verification_preserves_owner --locked -- --ignored --nocapture --test-threads=1 *> reports/p5-shared-acceptance-verification-20261012.log
if ($LASTEXITCODE) { Get-Content reports/p5-shared-acceptance-verification-20261012.log -Tail 30; exit $LASTEXITCODE }
cargo test -p cerulean-core --lib host::integration_tests::mixed:: --locked -- --ignored --nocapture --test-threads=1 *> reports/p5-shared-acceptance-mixed-20261012.log
if ($LASTEXITCODE) { Get-Content reports/p5-shared-acceptance-mixed-20261012.log -Tail 30; exit $LASTEXITCODE }
cargo clippy --workspace --all-targets --locked -- -D warnings *> reports/p5-shared-acceptance-clippy-20261012.log
if ($LASTEXITCODE) { Get-Content reports/p5-shared-acceptance-clippy-20261012.log -Tail 30; exit $LASTEXITCODE }
cargo build -p cerulean-desktop --features custom-protocol --locked *> reports/p5-shared-acceptance-desktop-20261012.log
if ($LASTEXITCODE) { Get-Content reports/p5-shared-acceptance-desktop-20261012.log -Tail 30; exit $LASTEXITCODE }
Write-Output 'Shared chief acceptance, verification, mixed routes and desktop checks passed; no real models or user desktop actions.'
