param([switch]$ResumeAfterFull)
$ErrorActionPreference='Stop'
$ceruleanRoot=Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $ceruleanRoot
$env:PATH="$env:USERPROFILE\.cargo\bin;D:\qqagent\node;$env:PATH"
$env:CERULEAN_TEST_GIT=(Get-Command git.exe -ErrorAction Stop).Source
$env:CERULEAN_OMP_TEST_PARENT='D:\test'
$env:CERULEAN_OMP_FIXTURE_EXE=Join-Path $ceruleanRoot 'target/debug/examples/omp_offline_worker_fixture.exe'
$env:CERULEAN_DAG_EXE=Join-Path $ceruleanRoot 'target/debug/examples/codex_dag_fixture.exe'
$env:CERULEAN_MIXED_DIR=Join-Path $ceruleanRoot 'reports/p5-runner-parallel-cases2-20261012'
if ($ResumeAfterFull) {
    if (-not (Test-Path -LiteralPath $env:CERULEAN_MIXED_DIR) -or (Get-ChildItem -LiteralPath $env:CERULEAN_MIXED_DIR).Count) { throw 'Resume requires the unused case directory from the completed full check.' }
    $fullLog=Get-Content -Raw -LiteralPath reports/p5-runner-parallel-full-check2-20261012.log
    if ($fullLog -notmatch '297 passed; 0 failed; 141 ignored' -or $fullLog -notmatch 'pass 2' -or $fullLog -notmatch 'pass 3' -or $fullLog -notmatch 'built in' -or $fullLog -match 'test result: FAILED') { throw 'Full check is not complete.' }
} elseif (Test-Path -LiteralPath $env:CERULEAN_MIXED_DIR) { throw 'Use a fresh case directory; do not overwrite evidence.' }
foreach ($stage in @('runner2','git','worktree','profiles','integration','mixed','clippy','desktop-build')) {
    if (Test-Path -LiteralPath "reports/p5-runner-parallel-$stage-20261012.log") { throw 'Existing check log must be preserved.' }
}
if (-not $ResumeAfterFull) {
if (Test-Path reports/p5-runner-parallel-full-check2-20261012.log) { throw 'Preserve earlier full check.' }
New-Item -ItemType Directory -Path $env:CERULEAN_MIXED_DIR | Out-Null
$ErrorActionPreference='Continue'
powershell.exe -NoProfile -ExecutionPolicy RemoteSigned -File scripts/check.ps1 *> reports/p5-runner-parallel-full-check2-20261012.log
$checkExit=$LASTEXITCODE
$ErrorActionPreference='Stop'
if ($checkExit) { Get-Content reports/p5-runner-parallel-full-check2-20261012.log -Tail 45; exit $checkExit }
}
# Windows PowerShell treats native stderr warnings as ErrorRecords. Explicit
# process exit codes below remain authoritative; warnings alone cannot abort.
$ErrorActionPreference='Continue'
cargo test -p cerulean-core --lib host::integration_tests::parallel:: --locked -- --ignored --nocapture --test-threads=1 *> reports/p5-runner-parallel-runner2-20261012.log
if ($LASTEXITCODE) { Get-Content reports/p5-runner-parallel-runner2-20261012.log -Tail 35; exit $LASTEXITCODE }
cargo test -p cerulean-core --lib git_workspaces::tests:: --locked -- --ignored --skip git_workspaces::tests::approved --nocapture --test-threads=1 *> reports/p5-runner-parallel-git-20261012.log
if ($LASTEXITCODE) { Get-Content reports/p5-runner-parallel-git-20261012.log -Tail 35; exit $LASTEXITCODE }
cargo test -p cerulean-core --lib host::api_host_tests::worktree:: --locked -- --ignored --nocapture --test-threads=1 *> reports/p5-runner-parallel-worktree-20261012.log
if ($LASTEXITCODE) { Get-Content reports/p5-runner-parallel-worktree-20261012.log -Tail 30; exit $LASTEXITCODE }
cargo test -p cerulean-core --lib host::integration_tests::profiles:: --locked -- --ignored --nocapture --test-threads=1 *> reports/p5-runner-parallel-profiles-20261012.log
if ($LASTEXITCODE) { Get-Content reports/p5-runner-parallel-profiles-20261012.log -Tail 35; exit $LASTEXITCODE }
cargo test -p cerulean-core --lib final_integration_success_failure_conflict_and_reopen --locked -- --ignored --nocapture --test-threads=1 *> reports/p5-runner-parallel-integration-20261012.log
if ($LASTEXITCODE) { Get-Content reports/p5-runner-parallel-integration-20261012.log -Tail 35; exit $LASTEXITCODE }
cargo test -p cerulean-core --lib host::integration_tests::mixed:: --locked -- --ignored --nocapture --test-threads=1 *> reports/p5-runner-parallel-mixed-20261012.log
if ($LASTEXITCODE) { Get-Content reports/p5-runner-parallel-mixed-20261012.log -Tail 30; exit $LASTEXITCODE }
cargo clippy --workspace --all-targets --locked -- -D warnings *> reports/p5-runner-parallel-clippy-20261012.log
if ($LASTEXITCODE) { Get-Content reports/p5-runner-parallel-clippy-20261012.log -Tail 25; exit $LASTEXITCODE }
cargo build -p cerulean-desktop --features custom-protocol --locked *> reports/p5-runner-parallel-desktop-build-20261012.log
if ($LASTEXITCODE) { Get-Content reports/p5-runner-parallel-desktop-build-20261012.log -Tail 25; exit $LASTEXITCODE }
Write-Output 'Mixed production hosts, strict static checks and desktop build passed; synthetic models only.'
