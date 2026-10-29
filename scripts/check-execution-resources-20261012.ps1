$ErrorActionPreference='Stop'
$ceruleanRoot=Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $ceruleanRoot
$env:PATH="$env:USERPROFILE\.cargo\bin;D:\qqagent\node;$env:PATH"
$env:CERULEAN_TEST_GIT=(Get-Command git.exe -ErrorAction Stop).Source
$env:CERULEAN_OMP_TEST_PARENT='D:\test'
$env:CERULEAN_OMP_FIXTURE_EXE=Join-Path $ceruleanRoot 'target/debug/examples/omp_offline_worker_fixture.exe'
$env:CERULEAN_DAG_EXE=Join-Path $ceruleanRoot 'target/debug/examples/codex_dag_fixture.exe'
$env:CERULEAN_MIXED_DIR=Join-Path $ceruleanRoot 'reports/p5-execution-resources-cases-20261012'
if (Test-Path -LiteralPath $env:CERULEAN_MIXED_DIR) { throw 'Use a fresh case directory; do not overwrite evidence.' }
foreach ($stage in @('mixed','clippy','desktop-build')) {
    if (Test-Path -LiteralPath "reports/p5-execution-resources-$stage-20261012.log") { throw 'Existing check log must be preserved.' }
}
New-Item -ItemType Directory -Path $env:CERULEAN_MIXED_DIR | Out-Null
cargo test -p cerulean-core --lib host::integration_tests::mixed:: --locked -- --ignored --nocapture --test-threads=1 *> reports/p5-execution-resources-mixed-20261012.log
if ($LASTEXITCODE) { Get-Content reports/p5-execution-resources-mixed-20261012.log -Tail 30; exit $LASTEXITCODE }
cargo clippy --workspace --all-targets --locked -- -D warnings *> reports/p5-execution-resources-clippy-20261012.log
if ($LASTEXITCODE) { Get-Content reports/p5-execution-resources-clippy-20261012.log -Tail 25; exit $LASTEXITCODE }
cargo build -p cerulean-desktop --features custom-protocol --locked *> reports/p5-execution-resources-desktop-build-20261012.log
if ($LASTEXITCODE) { Get-Content reports/p5-execution-resources-desktop-build-20261012.log -Tail 25; exit $LASTEXITCODE }
Write-Output 'Mixed production hosts, strict static checks and desktop build passed; synthetic models only.'
