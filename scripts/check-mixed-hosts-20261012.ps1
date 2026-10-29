$ErrorActionPreference='Stop'
$ceruleanRoot=Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $ceruleanRoot
$env:PATH="$env:USERPROFILE\.cargo\bin;D:\qqagent\node;$env:PATH"
$env:CERULEAN_OMP_TEST_PARENT='D:\test'
$env:CERULEAN_OMP_FIXTURE_EXE=Join-Path $ceruleanRoot 'target/debug/examples/omp_offline_worker_fixture.exe'
$env:CERULEAN_DAG_EXE=Join-Path $ceruleanRoot 'target/debug/examples/codex_dag_fixture.exe'
$env:CERULEAN_DAG_DIR=Join-Path $ceruleanRoot 'reports/p5-mixed-hosts-legacy-codex-20261012'
if (Test-Path -LiteralPath $env:CERULEAN_DAG_DIR) { throw 'Use a new run identifier; never overwrite earlier cases' }
foreach ($stage in @('omp-legacy','codex-legacy','core','clippy','fmt')) {
    if (Test-Path -LiteralPath "reports/p5-mixed-hosts-$stage-20261012.log") { throw 'Never overwrite earlier check logs' }
}
cargo test -p cerulean-core --lib actual_omp_project_dag_and_cancellation --locked -- --ignored --nocapture --test-threads=1 *> reports/p5-mixed-hosts-omp-legacy-20261012.log
if ($LASTEXITCODE) { Get-Content reports/p5-mixed-hosts-omp-legacy-20261012.log -Tail 25; exit $LASTEXITCODE }
foreach ($name in @('CERULEAN_DAG_OBSERVATIONS','CERULEAN_DAG_ADVICE','CERULEAN_DAG_DELIVERY','CERULEAN_DAG_SUPERVISION','CERULEAN_DAG_OBSERVATIONS_CLEAN')) {
    Remove-Item -LiteralPath "Env:$name" -ErrorAction SilentlyContinue
}
New-Item -ItemType Directory -Path $env:CERULEAN_DAG_DIR | Out-Null
$env:CERULEAN_DAG_MODE='success'
cargo test -p cerulean-core --lib two_task_production_pipeline_with_multichecks_and_restarts --locked -- --ignored --nocapture --test-threads=1 *> reports/p5-mixed-hosts-codex-legacy-20261012.log
if ($LASTEXITCODE) { Get-Content reports/p5-mixed-hosts-codex-legacy-20261012.log -Tail 25; exit $LASTEXITCODE }
cargo test -p cerulean-core --lib --locked *> reports/p5-mixed-hosts-core-20261012.log
if ($LASTEXITCODE) { Get-Content reports/p5-mixed-hosts-core-20261012.log -Tail 25; exit $LASTEXITCODE }
cargo clippy -p cerulean-core --all-targets --locked -- -D warnings *> reports/p5-mixed-hosts-clippy-20261012.log
if ($LASTEXITCODE) { Get-Content reports/p5-mixed-hosts-clippy-20261012.log -Tail 25; exit $LASTEXITCODE }
cargo fmt --all -- --check *> reports/p5-mixed-hosts-fmt-20261012.log
if ($LASTEXITCODE) { exit $LASTEXITCODE }
Write-Output 'Changed offline peers, default core regression and static checks passed; no model calls.'
