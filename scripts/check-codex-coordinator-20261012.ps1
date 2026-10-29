param([switch]$ResumeAfterFull)
$ErrorActionPreference='Stop'
$ceruleanRoot=Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $ceruleanRoot
$env:PATH="$env:USERPROFILE\.cargo\bin;D:\qqagent\node;$env:PATH"
$env:CERULEAN_TEST_GIT=(Get-Command git.exe).Source
$env:CERULEAN_TEST_NODE=(Get-Command node.exe).Source
$env:CERULEAN_OMP_TEST_PARENT='D:\test'
$env:CERULEAN_OMP_FIXTURE_EXE=Join-Path $ceruleanRoot 'target/debug/examples/omp_offline_worker_fixture.exe'
$env:CERULEAN_DAG_EXE=Join-Path $ceruleanRoot 'target/debug/examples/codex_dag_fixture.exe'
$env:CERULEAN_CODEX_FIXTURE_EXE=Join-Path $ceruleanRoot 'target/debug/examples/codex_protocol_fixture.exe'
function Invoke-Stage([string]$stage,[scriptblock]$action) {
    $log="reports/p5-codex-coordinator-$stage-20261012.log"
    if (Test-Path -LiteralPath $log) { throw "Preserve existing log: $log" }
    $ErrorActionPreference='Continue'
    & $action *> $log
    $code=$LASTEXITCODE
    if ($code) { Get-Content -LiteralPath $log -Tail 45; exit $code }
    if (Select-String -LiteralPath $log -SimpleMatch 'running 0 tests' -Quiet) {
        if ($stage -notin @('full','fixtures','clippy','desktop')) { throw "No tests matched $stage" }
    }
    Write-Output "Passed $stage"
}
if (-not $ResumeAfterFull) {
    Invoke-Stage 'full' { powershell.exe -NoProfile -ExecutionPolicy RemoteSigned -File scripts/check.ps1 }
    Invoke-Stage 'fixtures' { cargo build -p cerulean-core --examples --locked }
}
Invoke-Stage 'budget2' { cargo test -p cerulean-core --lib api_execution::tests::coordinator_budget:: --locked -- --ignored --nocapture --test-threads=1 }
Invoke-Stage 'worktree2' { cargo test -p cerulean-core --lib host::codex_worktree_tests:: --locked -- --ignored --nocapture --test-threads=1 }
foreach ($mode in @('success','recover-a')) {
    $env:CERULEAN_DAG_DIR=Join-Path $ceruleanRoot "reports/p5-codex-coordinator-dag-$mode-20261012"
    if (Test-Path -LiteralPath $env:CERULEAN_DAG_DIR) { throw 'Preserve original DAG evidence' }
    New-Item -ItemType Directory -Path $env:CERULEAN_DAG_DIR | Out-Null
    $env:CERULEAN_DAG_MODE=$mode
    Invoke-Stage "dag-$mode" { cargo test -p cerulean-core --lib host::project_dag_tests::dependency_worktree_codex_dag_with_restarts --locked -- --ignored --nocapture --test-threads=1 }
}
$env:CERULEAN_DAG_MODE='success'
$env:CERULEAN_DAG_OBSERVATIONS='1'
$env:CERULEAN_DAG_SUPERVISION='1'
$env:CERULEAN_DAG_OBSERVATIONS_CLEAN='1'
foreach ($delivery in @('ACK','WRONG_ACK','LOST_ACK','PAUSE','PAUSE_COMMIT','CANCEL')) {
    $env:CERULEAN_DAG_DELIVERY=$delivery
    $env:CERULEAN_DAG_DIR=Join-Path $ceruleanRoot "reports/p5-codex-coordinator-advice-$delivery-20261012"
    if (Test-Path -LiteralPath $env:CERULEAN_DAG_DIR) { throw 'Preserve original advice evidence' }
    New-Item -ItemType Directory -Path $env:CERULEAN_DAG_DIR | Out-Null
    Invoke-Stage "advice-$delivery" { cargo test -p cerulean-core --lib host::project_dag_tests::two_task_production_pipeline_with_multichecks_and_restarts --locked -- --ignored --nocapture --test-threads=1 }
}
foreach ($key in @('CERULEAN_DAG_DELIVERY','CERULEAN_DAG_OBSERVATIONS','CERULEAN_DAG_SUPERVISION','CERULEAN_DAG_OBSERVATIONS_CLEAN')) {
    Remove-Item -LiteralPath "Env:$key"
}
$env:CERULEAN_ADVICE_CRASH_DIR=Join-Path $ceruleanRoot 'reports/p5-codex-coordinator-crash-cases-20261012'
if (Test-Path -LiteralPath $env:CERULEAN_ADVICE_CRASH_DIR) { throw 'Preserve original crash evidence' }
New-Item -ItemType Directory -Path $env:CERULEAN_ADVICE_CRASH_DIR | Out-Null
Invoke-Stage 'crash' { cargo test -p cerulean-core --lib five_advice_boundaries_survive_real_owner_termination_without_resend --locked -- --ignored --nocapture --test-threads=1 }
Invoke-Stage 'clippy' { cargo clippy --workspace --all-targets --locked -- -D warnings }
Invoke-Stage 'desktop' { cargo build -p cerulean-desktop --features custom-protocol --locked }
Write-Output 'Codex coordinator checks passed. Real model/user desktop actions 0.'
