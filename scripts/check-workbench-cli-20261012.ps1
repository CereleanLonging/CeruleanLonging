param([string[]]$Stages=@('unit2','build','fixture','native','clippy','desktop'))
$ErrorActionPreference='Stop'
Set-Location -LiteralPath (Split-Path -Parent $PSScriptRoot)
$env:PATH="$env:USERPROFILE\.cargo\bin;D:\qqagent\node;$env:PATH"
foreach ($stage in $Stages) {
    $log="reports/p3-workbench-cli-$stage-20261012.log"
    if (Test-Path -LiteralPath $log) { throw "Preserve log: $log" }
    if ($stage -match '^native\d*$') {
        $entries=Get-Content reports/p3-workbench-cli-fixture-20261012.log | Where-Object { $_ -match '^\{.*"reason":"compiler-artifact"' } | ForEach-Object { $_ | ConvertFrom-Json }
        $entry=$entries | Where-Object { $_.profile.test -and $_.target.kind -contains 'lib' -and $_.executable } | Select-Object -Last 1
        if (-not $entry.executable) { throw 'Compiled fixture test executable missing' }
        $env:CERULEAN_CLI_TEST_EXE=$entry.executable
    }
    $ErrorActionPreference='Continue'
    switch -Regex ($stage) {
        '^unit\d*$' { cargo test -p cerulean-core --bin cerulean-core --locked *> $log; break }
        '^build\d*$' { cargo build -p cerulean-core --bin cerulean-core --locked *> $log; break }
        '^fixture$' { cargo test -p cerulean-core --lib --no-run --locked --message-format=json *> $log; break }
        '^native\d*$' { node tests/workbench-cli.test.mjs *> $log; break }
        '^startup\d*$' { node tests/workbench-cli-startup.test.mjs *> $log; break }
        '^clippy\d*$' { cargo clippy --workspace --all-targets --locked -- -D warnings *> $log; break }
        '^core\d*$' { cargo test -p cerulean-core --lib --locked *> $log; break }
        '^ipc\d*$' { node --test tests/core-ipc.test.mjs tests/resident-service.test.mjs *> $log; break }
        '^desktop\d*$' { cargo build -p cerulean-desktop --features custom-protocol --locked *> $log; break }
        default { throw 'Unknown stage' }
    }
    $code=$LASTEXITCODE
    $ErrorActionPreference='Stop'
    if ($code) { Get-Content -LiteralPath $log -Tail 35; exit $code }
    if ($stage -match '^unit' -and -not (Select-String -LiteralPath $log -SimpleMatch '4 passed; 0 failed' -Quiet)) { throw 'Unit tests did not run' }
    if ($stage -match '^build') {
        Copy-Item -LiteralPath target/debug/cerulean-core.exe -Destination target/debug/cerulean-runtime.exe -Force
        Copy-Item -LiteralPath target/debug/cerulean-core.exe -Destination apps/desktop/src-tauri/binaries/cerulean-runtime-x86_64-pc-windows-msvc.exe -Force
    }
    Write-Output "Passed $stage"
}
