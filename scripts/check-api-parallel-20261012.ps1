$ErrorActionPreference='Stop'
$ceruleanRoot=Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $ceruleanRoot
$env:PATH="$env:USERPROFILE\.cargo\bin;D:\qqagent\node;$env:PATH"
$env:CERULEAN_TEST_GIT=(Get-Command git.exe -ErrorAction Stop).Source
$env:CERULEAN_OMP_TEST_PARENT='D:\test'
$env:CERULEAN_OMP_FIXTURE_EXE=Join-Path $ceruleanRoot 'target/debug/examples/omp_offline_worker_fixture.exe'
$env:CERULEAN_DAG_EXE=Join-Path $ceruleanRoot 'target/debug/examples/codex_dag_fixture.exe'
$env:CERULEAN_MIXED_DIR=Join-Path $ceruleanRoot 'reports/p5-api-parallel-cases3-20261012'
if (Test-Path -LiteralPath $env:CERULEAN_MIXED_DIR) { throw 'Use a fresh case directory; do not overwrite evidence.' }
foreach ($stage in @('full-check3','public-final','budget','worktree','mixed','clippy','desktop-build')) {
    if (Test-Path -LiteralPath "reports/p5-api-parallel-$stage-20261012.log") { throw 'Existing check log must be preserved.' }
}
New-Item -ItemType Directory -Path $env:CERULEAN_MIXED_DIR | Out-Null
$ErrorActionPreference='Continue'
powershell.exe -NoProfile -ExecutionPolicy RemoteSigned -File scripts/check.ps1 *> reports/p5-api-parallel-full-check3-20261012.log
$checkExit=$LASTEXITCODE
$ErrorActionPreference='Stop'
if ($checkExit) { Get-Content reports/p5-api-parallel-full-check3-20261012.log -Tail 45; exit $checkExit }
cargo test -p cerulean-core --lib host::api_host_tests::registry:: --locked -- --ignored --nocapture --test-threads=1 *> reports/p5-api-parallel-public-final-20261012.log
if ($LASTEXITCODE) { Get-Content reports/p5-api-parallel-public-final-20261012.log -Tail 35; exit $LASTEXITCODE }
cargo test -p cerulean-core --lib api_execution::tests::coordinator_budget:: --locked -- --ignored --nocapture --test-threads=1 *> reports/p5-api-parallel-budget-20261012.log
if ($LASTEXITCODE) { Get-Content reports/p5-api-parallel-budget-20261012.log -Tail 35; exit $LASTEXITCODE }
cargo test -p cerulean-core --lib host::api_host_tests::worktree:: --locked -- --ignored --nocapture --test-threads=1 *> reports/p5-api-parallel-worktree-20261012.log
if ($LASTEXITCODE) { Get-Content reports/p5-api-parallel-worktree-20261012.log -Tail 30; exit $LASTEXITCODE }
cargo test -p cerulean-core --lib host::integration_tests::mixed:: --locked -- --ignored --nocapture --test-threads=1 *> reports/p5-api-parallel-mixed-20261012.log
if ($LASTEXITCODE) { Get-Content reports/p5-api-parallel-mixed-20261012.log -Tail 30; exit $LASTEXITCODE }
cargo clippy --workspace --all-targets --locked -- -D warnings *> reports/p5-api-parallel-clippy-20261012.log
if ($LASTEXITCODE) { Get-Content reports/p5-api-parallel-clippy-20261012.log -Tail 25; exit $LASTEXITCODE }
cargo build -p cerulean-desktop --features custom-protocol --locked *> reports/p5-api-parallel-desktop-build-20261012.log
if ($LASTEXITCODE) { Get-Content reports/p5-api-parallel-desktop-build-20261012.log -Tail 25; exit $LASTEXITCODE }
Write-Output 'Mixed production hosts, strict static checks and desktop build passed; synthetic models only.'
