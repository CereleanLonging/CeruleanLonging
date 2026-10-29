param([switch]$AfterDetailFix)
$ErrorActionPreference='Stop'
$ceruleanRoot=Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $ceruleanRoot
$env:PATH="$env:USERPROFILE\.cargo\bin;D:\qqagent\node;$env:PATH"
$env:CERULEAN_TEST_GIT=(Get-Command git.exe -ErrorAction Stop).Source
$env:CERULEAN_TEST_NODE=(Get-Command node.exe -ErrorAction Stop).Source
$env:CERULEAN_OMP_TEST_PARENT='D:\test'
$env:CERULEAN_OMP_FIXTURE_EXE=Join-Path $ceruleanRoot 'target/debug/examples/omp_offline_worker_fixture.exe'
$env:CERULEAN_DAG_EXE=Join-Path $ceruleanRoot 'target/debug/examples/codex_dag_fixture.exe'
$env:CERULEAN_MIXED_DIR=Join-Path $ceruleanRoot 'reports/p5-shared-verification-cases-20261012'
if ($AfterDetailFix) {
    if (-not (Test-Path -LiteralPath $env:CERULEAN_MIXED_DIR) -or (Get-ChildItem -LiteralPath $env:CERULEAN_MIXED_DIR).Count) { throw 'Resume needs the unused mixed case directory.' }
    $priorRunner=Get-Content -Raw reports/p5-shared-verification-runner-20261012.log
    if ($priorRunner -notmatch '3 passed; 0 failed') { throw 'Original runner regression must have passed.' }
} elseif (Test-Path -LiteralPath $env:CERULEAN_MIXED_DIR) { throw 'Preserve existing case evidence.' }
$remainingStages=if ($AfterDetailFix) { @('full2','lifecycle3','worktree','budget','mixed','clippy','desktop') } else { @('full','runner','lifecycle','worktree','budget','mixed','clippy','desktop') }
foreach ($stage in $remainingStages) {
    if (Test-Path -LiteralPath "reports/p5-shared-verification-$stage-20261012.log") { throw 'Preserve existing log.' }
}
if (-not $AfterDetailFix) {New-Item -ItemType Directory -Path $env:CERULEAN_MIXED_DIR | Out-Null}
# Native stderr warnings are not failures. Every process exit is checked.
$ErrorActionPreference='Continue'
$fullStage=if ($AfterDetailFix) {'full2'} else {'full'}
$lifecycleStage=if ($AfterDetailFix) {'lifecycle3'} else {'lifecycle'}
powershell.exe -NoProfile -ExecutionPolicy RemoteSigned -File scripts/check.ps1 *> "reports/p5-shared-verification-$fullStage-20261012.log"
if ($LASTEXITCODE) { Get-Content "reports/p5-shared-verification-$fullStage-20261012.log" -Tail 35; exit $LASTEXITCODE }
if (-not $AfterDetailFix) {
cargo test -p cerulean-core --lib host::integration_tests::parallel:: --locked -- --ignored --nocapture --test-threads=1 *> reports/p5-shared-verification-runner-20261012.log
if ($LASTEXITCODE) { Get-Content reports/p5-shared-verification-runner-20261012.log -Tail 35; exit $LASTEXITCODE }
}
cargo test -p cerulean-core --lib shared_verification_preserves_owner --locked -- --ignored --nocapture --test-threads=1 *> "reports/p5-shared-verification-$lifecycleStage-20261012.log"
if ($LASTEXITCODE) { Get-Content "reports/p5-shared-verification-$lifecycleStage-20261012.log" -Tail 35; exit $LASTEXITCODE }
cargo test -p cerulean-core --lib host::api_host_tests::worktree:: --locked -- --ignored --nocapture --test-threads=1 *> reports/p5-shared-verification-worktree-20261012.log
if ($LASTEXITCODE) { Get-Content reports/p5-shared-verification-worktree-20261012.log -Tail 35; exit $LASTEXITCODE }
cargo test -p cerulean-core --lib api_execution::tests::coordinator_budget:: --locked -- --ignored --nocapture --test-threads=1 *> reports/p5-shared-verification-budget-20261012.log
if ($LASTEXITCODE) { Get-Content reports/p5-shared-verification-budget-20261012.log -Tail 35; exit $LASTEXITCODE }
cargo test -p cerulean-core --lib host::integration_tests::mixed:: --locked -- --ignored --nocapture --test-threads=1 *> reports/p5-shared-verification-mixed-20261012.log
if ($LASTEXITCODE) { Get-Content reports/p5-shared-verification-mixed-20261012.log -Tail 35; exit $LASTEXITCODE }
cargo clippy --workspace --all-targets --locked -- -D warnings *> reports/p5-shared-verification-clippy-20261012.log
if ($LASTEXITCODE) { Get-Content reports/p5-shared-verification-clippy-20261012.log -Tail 30; exit $LASTEXITCODE }
cargo build -p cerulean-desktop --features custom-protocol --locked *> reports/p5-shared-verification-desktop-20261012.log
if ($LASTEXITCODE) { Get-Content reports/p5-shared-verification-desktop-20261012.log -Tail 30; exit $LASTEXITCODE }
Write-Output 'Shared verification, recovery, mixed hosts and desktop checks passed. No real models or user desktop actions.'
