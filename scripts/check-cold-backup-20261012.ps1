param([string[]]$Stages=@('unit','cli','build','native','clippy','desktop'))
$ErrorActionPreference='Stop'
Set-Location -LiteralPath (Split-Path -Parent $PSScriptRoot)
$env:PATH="$env:USERPROFILE\.cargo\bin;D:\qqagent\node;$env:PATH"
foreach ($stage in $Stages) {
    $log="reports/p6-cold-backup-$stage-20261012.log"
    if (Test-Path -LiteralPath $log) { throw "Preserve log: $log" }
    $ErrorActionPreference='Continue'
    switch -Regex ($stage) {
        '^unit\d*$' { cargo test -p cerulean-core --lib backup:: --locked *> $log; break }
        '^cli\d*$' { cargo test -p cerulean-core --bin cerulean-core --locked *> $log; break }
        '^build\d*$' { cargo build -p cerulean-core --bin cerulean-core --locked *> $log; break }
        '^native\d*$' { node tests/cold-backup.test.mjs *> $log; break }
        '^clippy\d*$' { cargo clippy --workspace --all-targets --locked -- -D warnings *> $log; break }
        '^desktop\d*$' { cargo build -p cerulean-desktop --features custom-protocol --locked *> $log; break }
        default { throw 'Unknown stage' }
    }
    $code=$LASTEXITCODE
    $ErrorActionPreference='Stop'
    if ($code) { Get-Content -LiteralPath $log -Tail 40; exit $code }
    if ($stage -match '^build') {
        Copy-Item -LiteralPath target/debug/cerulean-core.exe -Destination target/debug/cerulean-runtime.exe -Force
        Copy-Item -LiteralPath target/debug/cerulean-core.exe -Destination apps/desktop/src-tauri/binaries/cerulean-runtime-x86_64-pc-windows-msvc.exe -Force
    }
    Write-Output "Passed $stage"
}
