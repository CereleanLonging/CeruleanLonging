param([ValidateSet('workspace','fixtures','mixed','legacy-shared','legacy-advice','legacy-delivery','legacy-closure','closure-forgery','clippy')][string]$Stage, [string]$Suffix='1')
$ErrorActionPreference='Stop'
$projectRoot=Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $projectRoot
$env:PATH="$env:USERPROFILE\.cargo\bin;D:\qqagent\node;$env:PATH"
$env:CERULEAN_TEST_GIT=(Get-Command git.exe).Source
$env:CERULEAN_TEST_NODE=(Get-Command node.exe).Source
$env:CERULEAN_OMP_TEST_PARENT='D:\test'
$env:CERULEAN_OMP_FIXTURE_EXE=Join-Path $projectRoot 'target/debug/examples/omp_offline_worker_fixture.exe'
$env:CERULEAN_DAG_EXE=Join-Path $projectRoot 'target/debug/examples/codex_dag_fixture.exe'
$env:CERULEAN_CODEX_FIXTURE_EXE=Join-Path $projectRoot 'target/debug/examples/codex_protocol_fixture.exe'
$log=Join-Path $projectRoot "reports/p5-scoped-supervision-$Stage-$Suffix-20261013.log"
if (Test-Path -LiteralPath $log) { throw "Preserve log: $log" }
if ($Stage -eq 'mixed') {
    $env:CERULEAN_SCOPED_SUPERVISION_DIR=Join-Path $projectRoot "reports/p5-scoped-supervision-mixed-$Suffix-cases-20261013"
    if (Test-Path -LiteralPath $env:CERULEAN_SCOPED_SUPERVISION_DIR) { throw 'Preserve original evidence' }
    New-Item -ItemType Directory -Path $env:CERULEAN_SCOPED_SUPERVISION_DIR | Out-Null
}
if ($Stage -eq 'legacy-delivery') {
    $env:CERULEAN_DAG_DELIVERY='LOST_ACK'
    $env:CERULEAN_DAG_MODE='success'
    $env:CERULEAN_DAG_OBSERVATIONS='1'
    $env:CERULEAN_DAG_SUPERVISION='1'
    $env:CERULEAN_DAG_OBSERVATIONS_CLEAN='1'
    $env:CERULEAN_DAG_DIR=Join-Path $projectRoot "reports/p5-scoped-supervision-legacy-delivery-$Suffix-cases-20261013"
    if (Test-Path -LiteralPath $env:CERULEAN_DAG_DIR) { throw 'Preserve original delivery evidence' }
    New-Item -ItemType Directory -Path $env:CERULEAN_DAG_DIR | Out-Null
}
if ($Stage -in @('legacy-closure','closure-forgery')) {
    $env:CERULEAN_CLOSURE_SOURCE=Join-Path $projectRoot "reports/p5-scoped-supervision-legacy-delivery-$Suffix-cases-20261013"
    if (-not (Test-Path -LiteralPath (Join-Path $env:CERULEAN_CLOSURE_SOURCE 'delivery-result.json'))) { throw 'Missing original isolated delivery result' }
}
$ErrorActionPreference='Continue'
switch ($Stage) {
    'workspace' { cargo test --workspace --locked --offline *> $log }
    'fixtures' { cargo build -p cerulean-core --examples --locked --offline *> $log }
    'mixed' { cargo test -p cerulean-core --lib mixed_scoped_supervision_delivers_only_to_original_codex_and_reopens --locked --offline -- --ignored --nocapture --test-threads=1 *> $log }
    'legacy-shared' { cargo test -p cerulean-core --lib codex_shared_supervision_interleaving_preserves_turn_and_never_resends --locked --offline -- --ignored --nocapture --test-threads=1 *> $log }
    'legacy-advice' { cargo test -p cerulean-core --lib advice --locked --offline -- --nocapture --test-threads=1 *> $log }
    'legacy-delivery' { cargo test -p cerulean-core --lib host::project_dag_tests::two_task_production_pipeline_with_multichecks_and_restarts --locked --offline -- --ignored --nocapture --test-threads=1 *> $log }
    'legacy-closure' { cargo test -p cerulean-core --lib user_closure_preserves_unknown_and_requires_explicit_resume --locked --offline -- --ignored --nocapture --test-threads=1 *> $log }
    'closure-forgery' { cargo test -p cerulean-core --lib closure_rejects_rehashed_cross_ledger_proofs --locked --offline -- --ignored --nocapture --test-threads=1 *> $log }
    'clippy' { cargo clippy --workspace --all-targets --locked --offline -- -D warnings *> $log }
}
$code=$LASTEXITCODE
$ErrorActionPreference='Stop'
if ($code) { Get-Content -LiteralPath $log -Tail 65; exit $code }
if ($Stage -notin @('fixtures','clippy') -and -not (Select-String -LiteralPath $log -Pattern 'test result: ok\. [1-9]' -Quiet)) { throw 'No passing tests recorded' }
Write-Output "Passed $Stage; $log"
