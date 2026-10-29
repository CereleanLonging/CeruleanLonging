param()
$ErrorActionPreference = 'Stop'
$workspacePath = [IO.Path]::GetFullPath((Split-Path $PSScriptRoot -Parent))
$targetPath = [IO.Path]::GetFullPath((Join-Path $workspacePath 'target'))
$debugPath = [IO.Path]::GetFullPath((Join-Path $targetPath 'debug'))
$cachePath = [IO.Path]::GetFullPath((Join-Path $debugPath 'incremental'))
if (-not $cachePath.StartsWith($workspacePath + '\', [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Cache path escaped workspace.'
}
foreach ($path in @($workspacePath, $targetPath, $debugPath, $cachePath)) {
    if (Test-Path -LiteralPath $path) {
        $item = Get-Item -LiteralPath $path -Force
        if (-not $item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
            throw "Expected ordinary directory: $path"
        }
        if ([IO.Path]::GetFullPath($item.FullName) -ne $path) { throw 'Resolved path mismatch.' }
    }
}
$builders = @(Get-Process | Where-Object { $_.ProcessName -match '^(cargo|rustc|clippy-driver)$' })
if ($builders.Count) { throw 'Build processes are active; preserve cache and retry after they finish.' }
if (-not (Test-Path -LiteralPath $cachePath)) { Write-Output 'Incremental cache is already absent.'; exit 0 }
$lockPath = Join-Path $debugPath '.cargo-lock'
$lockItem = Get-Item -LiteralPath $lockPath -Force
if ($lockItem.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Unexpected build lock path.' }
$guard = [IO.File]::Open($lockPath, [IO.FileMode]::Open, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
try {
    $entries = @(Get-ChildItem -LiteralPath $cachePath -Recurse -Force)
    if (@($entries | Where-Object { $_.Attributes -band [IO.FileAttributes]::ReparsePoint }).Count) {
        throw 'Cache contains reparse points; preserve it for inspection.'
    }
    $files = @($entries | Where-Object { -not $_.PSIsContainer })
    $bytes = ($files | Measure-Object -Property Length -Sum).Sum
    Remove-Item -LiteralPath $cachePath -Recurse -Force
    if (Test-Path -LiteralPath $cachePath) { throw 'Cache removal incomplete.' }
    @{ cachePath=$cachePath; removedFiles=$files.Count; removedLogicalBytes=$bytes; cargoLockHeld=$true; releaseTouched=$false } | ConvertTo-Json -Compress
} finally { $guard.Dispose() }
