param([string[]]$Stages=@('fixtures','parallel'))
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
foreach ($stage in $Stages) {
    $log="reports/p5-codex-shared-$stage-20261012.log"
    if (Test-Path -LiteralPath $log) { throw "Preserve existing log: $log" }
    $ErrorActionPreference='Continue'
    switch -Regex ($stage) {
        '^fixtures\d*$' { cargo build -p cerulean-core --examples --locked *> $log; break }
        '^parallel\d*$' { cargo test -p cerulean-core --lib host::integration_tests::codex_parallel:: --locked -- --ignored --nocapture --test-threads=1 *> $log; break }
        '^automatic\d*$' { cargo test -p cerulean-core --lib automatic_codex_mixed_parallel_completion_and_single_attempt_cancel --locked -- --ignored --nocapture --test-threads=1 *> $log; break }
        '^supervision\d*$' { cargo test -p cerulean-core --lib codex_shared_supervision_interleaving_preserves_turn_and_never_resends --locked -- --ignored --nocapture --test-threads=1 *> $log; break }
        '^full\d*$' { powershell.exe -NoProfile -ExecutionPolicy RemoteSigned -File scripts/check.ps1 *> $log; break }
        '^core\d*$' { cargo test -p cerulean-core --lib --locked *> $log; break }
        '^build\d*$' { cargo build -p cerulean-core --locked *> $log; break }
        '^omp-parallel\d*$' { cargo test -p cerulean-core --lib host::integration_tests::omp_parallel:: --locked -- --ignored --nocapture --test-threads=1 *> $log; break }
        '^worktree\d*$' { cargo test -p cerulean-core --lib host::codex_worktree_tests:: --locked -- --ignored --nocapture --test-threads=1 *> $log; break }
        '^dag-(success|recover-a)$' {
            $env:CERULEAN_DAG_MODE=$Matches[1]
            $env:CERULEAN_DAG_DIR=Join-Path $ceruleanRoot "reports/p5-codex-shared-$stage-cases-20261012"
            if (Test-Path -LiteralPath $env:CERULEAN_DAG_DIR) { throw 'Preserve DAG evidence' }
            New-Item -ItemType Directory -Path $env:CERULEAN_DAG_DIR | Out-Null
            cargo test -p cerulean-core --lib host::project_dag_tests::dependency_worktree_codex_dag_with_restarts --locked -- --ignored --nocapture --test-threads=1 *> $log
            break
        }
        '^advice-(ACK|WRONG_ACK|LOST_ACK|PAUSE|PAUSE_COMMIT|CANCEL)$' {
            $env:CERULEAN_DAG_DELIVERY=$Matches[1]
            $env:CERULEAN_DAG_MODE='success'
            $env:CERULEAN_DAG_OBSERVATIONS='1'
            $env:CERULEAN_DAG_SUPERVISION='1'
            $env:CERULEAN_DAG_OBSERVATIONS_CLEAN='1'
            $env:CERULEAN_DAG_DIR=Join-Path $ceruleanRoot "reports/p5-codex-shared-$stage-cases-20261012"
            if (Test-Path -LiteralPath $env:CERULEAN_DAG_DIR) { throw 'Preserve advice evidence' }
            New-Item -ItemType Directory -Path $env:CERULEAN_DAG_DIR | Out-Null
            cargo test -p cerulean-core --lib host::project_dag_tests::two_task_production_pipeline_with_multichecks_and_restarts --locked -- --ignored --nocapture --test-threads=1 *> $log
            break
        }
        '^crash$' {
            $env:CERULEAN_ADVICE_CRASH_DIR=Join-Path $ceruleanRoot 'reports/p5-codex-shared-crash-cases-20261012'
            if (Test-Path -LiteralPath $env:CERULEAN_ADVICE_CRASH_DIR) { throw 'Preserve crash evidence' }
            New-Item -ItemType Directory -Path $env:CERULEAN_ADVICE_CRASH_DIR | Out-Null
            cargo test -p cerulean-core --lib five_advice_boundaries_survive_real_owner_termination_without_resend --locked -- --ignored --nocapture --test-threads=1 *> $log
            break
        }
        '^clippy\d*$' { cargo clippy --workspace --all-targets --locked -- -D warnings *> $log; break }
        '^desktop\d*$' { cargo build -p cerulean-desktop --features custom-protocol --locked *> $log; break }
        default { throw "Unknown stage $stage" }
    }
    $code=$LASTEXITCODE
    $ErrorActionPreference='Stop'
    foreach ($key in @('CERULEAN_DAG_DELIVERY','CERULEAN_DAG_OBSERVATIONS','CERULEAN_DAG_SUPERVISION','CERULEAN_DAG_OBSERVATIONS_CLEAN')) {
        Remove-Item -LiteralPath "Env:$key" -ErrorAction SilentlyContinue
    }
    if ($code) { Get-Content -LiteralPath $log -Tail 50; exit $code }
    if ($stage -notmatch '^(full|fixtures|clippy|desktop|build)' -and (Select-String -LiteralPath $log -SimpleMatch 'running 0 tests' -Quiet)) { throw "No tests matched $stage" }
    if ($stage -match '^build\d*$') {
        Copy-Item -LiteralPath 'target/debug/cerulean-core.exe' -Destination 'target/debug/cerulean-runtime.exe' -Force
        Copy-Item -LiteralPath 'target/debug/cerulean-core.exe' -Destination 'apps/desktop/src-tauri/binaries/cerulean-runtime-x86_64-pc-windows-msvc.exe' -Force
    }
    Write-Output "Passed $stage"
}
