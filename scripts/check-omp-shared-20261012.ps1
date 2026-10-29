$ErrorActionPreference='Stop'
$ceruleanRoot=Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $ceruleanRoot
$env:PATH="$env:USERPROFILE\.cargo\bin;D:\qqagent\node;$env:PATH"
$env:CERULEAN_TEST_GIT=(Get-Command git.exe -ErrorAction Stop).Source
$env:CERULEAN_TEST_NODE=(Get-Command node.exe -ErrorAction Stop).Source
$env:CERULEAN_OMP_TEST_PARENT='D:\test'
$env:CERULEAN_OMP_FIXTURE_EXE=Join-Path $ceruleanRoot 'target/debug/examples/omp_offline_worker_fixture.exe'
$env:CERULEAN_DAG_EXE=Join-Path $ceruleanRoot 'target/debug/examples/codex_dag_fixture.exe'
$env:CERULEAN_MIXED_DIR=Join-Path $ceruleanRoot 'reports/p5-omp-shared-cases-20261012'
$env:CERULEAN_ASSIGNMENT_REPORT=Join-Path $ceruleanRoot 'reports/p5-omp-shared-crash-result-20261012.json'
foreach ($stage in @('full','worktree','dag','api-registry','mixed','crash','clippy','desktop')) {
    if (Test-Path -LiteralPath "reports/p5-omp-shared-$stage-20261012.log") { throw 'Preserve prior log.' }
}
if ((Test-Path -LiteralPath $env:CERULEAN_MIXED_DIR) -or (Test-Path -LiteralPath $env:CERULEAN_ASSIGNMENT_REPORT)) { throw 'Preserve prior case evidence.' }
New-Item -ItemType Directory -Path $env:CERULEAN_MIXED_DIR | Out-Null
$ErrorActionPreference='Continue'
powershell.exe -NoProfile -ExecutionPolicy RemoteSigned -File scripts/check.ps1 *> reports/p5-omp-shared-full-20261012.log
if ($LASTEXITCODE) { Get-Content reports/p5-omp-shared-full-20261012.log -Tail 35; exit $LASTEXITCODE }
cargo test -p cerulean-core --lib host::omp_host::tests::worktree:: --locked -- --ignored --nocapture --test-threads=1 *> reports/p5-omp-shared-worktree-20261012.log
if ($LASTEXITCODE) { Get-Content reports/p5-omp-shared-worktree-20261012.log -Tail 35; exit $LASTEXITCODE }
cargo test -p cerulean-core --lib host::omp_project_tests::dependency_worktree_omp_dag_restart_and_cancel --locked -- --ignored --nocapture --test-threads=1 *> reports/p5-omp-shared-dag-20261012.log
if ($LASTEXITCODE) { Get-Content reports/p5-omp-shared-dag-20261012.log -Tail 35; exit $LASTEXITCODE }
cargo test -p cerulean-core --lib host::api_host_tests::registry:: --locked -- --ignored --nocapture --test-threads=1 *> reports/p5-omp-shared-api-registry-20261012.log
if ($LASTEXITCODE) { Get-Content reports/p5-omp-shared-api-registry-20261012.log -Tail 35; exit $LASTEXITCODE }
cargo test -p cerulean-core --lib host::integration_tests::mixed:: --locked -- --ignored --nocapture --test-threads=1 *> reports/p5-omp-shared-mixed-20261012.log
if ($LASTEXITCODE) { Get-Content reports/p5-omp-shared-mixed-20261012.log -Tail 35; exit $LASTEXITCODE }
cargo test -p cerulean-core --lib five_assignment_owner_crashes_preserve_evidence_and_never_resend --locked -- --ignored --nocapture --test-threads=1 *> reports/p5-omp-shared-crash-20261012.log
if ($LASTEXITCODE) { Get-Content reports/p5-omp-shared-crash-20261012.log -Tail 35; exit $LASTEXITCODE }
cargo clippy --workspace --all-targets --locked -- -D warnings *> reports/p5-omp-shared-clippy-20261012.log
if ($LASTEXITCODE) { Get-Content reports/p5-omp-shared-clippy-20261012.log -Tail 35; exit $LASTEXITCODE }
cargo build -p cerulean-desktop --features custom-protocol --locked *> reports/p5-omp-shared-desktop-20261012.log
if ($LASTEXITCODE) { Get-Content reports/p5-omp-shared-desktop-20261012.log -Tail 35; exit $LASTEXITCODE }
Write-Output 'OMP shared host checks passed; real provider/user desktop actions 0.'
