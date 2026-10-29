$ErrorActionPreference='Stop'
$ceruleanRoot=Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $ceruleanRoot
$env:PATH="$env:USERPROFILE\.cargo\bin;D:\qqagent\node;$env:PATH"
$env:CERULEAN_TEST_GIT=(Get-Command git.exe).Source
$env:CERULEAN_OMP_TEST_PARENT='D:\test'
$env:CERULEAN_OMP_FIXTURE_EXE=Join-Path $ceruleanRoot 'target/debug/examples/omp_offline_worker_fixture.exe'
cargo test -p cerulean-core --lib dependency_worktree_omp_dag_restart_and_cancel --locked -- --ignored --nocapture --test-threads=1 *> reports/p5-dependency-host-omp-20261012.log
if ($LASTEXITCODE) { Get-Content reports/p5-dependency-host-omp-20261012.log -Tail 25; exit $LASTEXITCODE }
cargo test -p cerulean-core --lib actual_omp_project_dag_and_cancellation --locked -- --ignored --nocapture --test-threads=1 *> reports/p5-dependency-host-omp-legacy-20261012.log
if ($LASTEXITCODE) { Get-Content reports/p5-dependency-host-omp-legacy-20261012.log -Tail 25; exit $LASTEXITCODE }
$env:CERULEAN_DAG_EXE=Join-Path $ceruleanRoot 'target/debug/examples/codex_dag_fixture.exe'
foreach ($name in @('CERULEAN_DAG_OBSERVATIONS','CERULEAN_DAG_ADVICE','CERULEAN_DAG_DELIVERY','CERULEAN_DAG_SUPERVISION','CERULEAN_DAG_OBSERVATIONS_CLEAN')) {
    Remove-Item -LiteralPath "Env:$name" -ErrorAction SilentlyContinue
}
foreach ($mode in @('success','recover-a','legacy')) {
    $ceruleanDir=Join-Path $ceruleanRoot "reports/p5-dependency-host-codex-$mode-20261012"
    if (Test-Path -LiteralPath $ceruleanDir) { throw 'Use a new run identifier; never overwrite previous evidence' }
    $null=New-Item -ItemType Directory -Path $ceruleanDir
    $env:CERULEAN_DAG_DIR=$ceruleanDir
    $env:CERULEAN_DAG_MODE=if ($mode -eq 'legacy') {'success'} else {$mode}
    $ceruleanFilter=if ($mode -eq 'legacy') {'two_task_production_pipeline_with_multichecks_and_restarts'} else {'dependency_worktree_codex_dag_with_restarts'}
    $ceruleanLog="reports/p5-dependency-host-codex-$mode-20261012.log"
    cargo test -p cerulean-core --lib $ceruleanFilter --locked -- --ignored --nocapture --test-threads=1 *> $ceruleanLog
    if ($LASTEXITCODE) { Get-Content -LiteralPath $ceruleanLog -Tail 25; exit $LASTEXITCODE }
}
cargo fmt --all -- --check *> reports/p5-dependency-host-fmt-20261012.log
if ($LASTEXITCODE) { exit $LASTEXITCODE }
cargo test -p cerulean-core --lib --locked *> reports/p5-dependency-host-core-regression-20261012.log
if ($LASTEXITCODE) { Get-Content reports/p5-dependency-host-core-regression-20261012.log -Tail 25; exit $LASTEXITCODE }
cargo clippy -p cerulean-core --all-targets --locked -- -D warnings *> reports/p5-dependency-host-clippy-20261012.log
if ($LASTEXITCODE) { Get-Content reports/p5-dependency-host-clippy-20261012.log -Tail 25; exit $LASTEXITCODE }
Write-Output 'Resident dependency DAG and changed legacy fixture checks passed. No provider calls.'
