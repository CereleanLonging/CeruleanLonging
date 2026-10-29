param([string[]]$Stages=@('crash1'))
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
    $log="reports/p5-shared-crash-$stage-20261012.log"
    if (Test-Path -LiteralPath $log) { throw "Preserve log: $log" }
    Remove-Item -LiteralPath 'Env:CERULEAN_SHARED_CRASH_RECOVER' -ErrorAction SilentlyContinue
    if ($stage -match '^supervision\d*$') {
        $env:CERULEAN_SHARED_SUPERVISION_DIR=Join-Path $ceruleanRoot "reports/p5-shared-crash-$stage-cases-20261012"
        if (Test-Path -LiteralPath $env:CERULEAN_SHARED_SUPERVISION_DIR) { throw 'Preserve supervision evidence' }
        New-Item -ItemType Directory -Path $env:CERULEAN_SHARED_SUPERVISION_DIR | Out-Null
    }
    if ($stage -match '^(crash|recover)\d+$') {
        $env:CERULEAN_SHARED_CRASH_DIR=Join-Path $ceruleanRoot "reports/p5-shared-crash-$stage-cases-20261012"
        if (Test-Path -LiteralPath $env:CERULEAN_SHARED_CRASH_DIR) { throw 'Preserve crash evidence' }
        New-Item -ItemType Directory -Path $env:CERULEAN_SHARED_CRASH_DIR | Out-Null
        if ($stage -match '^recover') { $env:CERULEAN_SHARED_CRASH_RECOVER='1' }
    }
    $ErrorActionPreference='Continue'
    switch -Regex ($stage) {
        '^fixtures\d*$' { cargo build -p cerulean-core --examples --locked *> $log; break }
        '^supervision\d*$' { cargo test -p cerulean-core --lib simultaneous_codex_incidents_deliver_once_to_each_original_turn --locked -- --ignored --nocapture --test-threads=1 *> $log; break }
        '^original-advice$' { cargo test -p cerulean-core --lib codex_shared_supervision_interleaving_preserves_turn_and_never_resends --locked -- --ignored --nocapture --test-threads=1 *> $log; break }
        '^advice-(ACK|WRONG_ACK|LOST_ACK|PAUSE|PAUSE_COMMIT|CANCEL)$' {
            $env:CERULEAN_DAG_DELIVERY=$Matches[1]
            $env:CERULEAN_DAG_MODE='success'
            $env:CERULEAN_DAG_OBSERVATIONS='1'
            $env:CERULEAN_DAG_SUPERVISION='1'
            $env:CERULEAN_DAG_OBSERVATIONS_CLEAN='1'
            $env:CERULEAN_DAG_DIR=Join-Path $ceruleanRoot "reports/p5-shared-crash-$stage-cases-20261012"
            if (Test-Path -LiteralPath $env:CERULEAN_DAG_DIR) { throw 'Preserve advice evidence' }
            New-Item -ItemType Directory -Path $env:CERULEAN_DAG_DIR | Out-Null
            cargo test -p cerulean-core --lib host::project_dag_tests::two_task_production_pipeline_with_multichecks_and_restarts --locked -- --ignored --nocapture --test-threads=1 *> $log
            break
        }
        '^(crash|recover)\d+$' { cargo test -p cerulean-core --lib shared_owner_termination_preserves_original_attempts_resources_and_no_resend --locked -- --ignored --nocapture --test-threads=1 *> $log; break }
        '^core\d*$' { cargo test -p cerulean-core --lib --locked *> $log; break }
        '^build\d*$' { cargo build -p cerulean-core --locked *> $log; break }
        '^desktop\d*$' { cargo build -p cerulean-desktop --features custom-protocol --locked *> $log; break }
        '^clippy\d*$' { cargo clippy --workspace --all-targets --locked -- -D warnings *> $log; break }
        default { throw "Unknown stage $stage" }
    }
    $stageCode=$LASTEXITCODE
    $ErrorActionPreference='Stop'
    foreach ($key in @('CERULEAN_DAG_DELIVERY','CERULEAN_DAG_OBSERVATIONS','CERULEAN_DAG_SUPERVISION','CERULEAN_DAG_OBSERVATIONS_CLEAN')) {
        Remove-Item -LiteralPath "Env:$key" -ErrorAction SilentlyContinue
    }
    if ($stageCode) { Get-Content -LiteralPath $log -Tail 50; exit $stageCode }
    if ($stage -notmatch '^(clippy|fixtures|build|desktop)' -and (Select-String -LiteralPath $log -SimpleMatch 'running 0 tests' -Quiet)) { throw "No tests matched $stage" }
    if ($stage -match '^build\d*$') {
        Copy-Item -LiteralPath 'target/debug/cerulean-core.exe' -Destination 'target/debug/cerulean-runtime.exe' -Force
        Copy-Item -LiteralPath 'target/debug/cerulean-core.exe' -Destination 'apps/desktop/src-tauri/binaries/cerulean-runtime-x86_64-pc-windows-msvc.exe' -Force
    }
    Write-Output "Passed $stage"
}
