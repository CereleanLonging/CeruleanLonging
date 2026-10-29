param(
    [ValidateRange(2, 365)] [int]$MinimumAgeDays = 2,
    [switch]$Apply,
    [Parameter(Mandatory = $true)] [string]$ReportPath
)
$ErrorActionPreference = 'Stop'
$workspacePath = [IO.Path]::GetFullPath((Split-Path $PSScriptRoot -Parent))
$targetPath = [IO.Path]::GetFullPath((Join-Path $workspacePath 'target'))
$debugPath = [IO.Path]::GetFullPath((Join-Path $targetPath 'debug'))
$depsPath = [IO.Path]::GetFullPath((Join-Path $debugPath 'deps'))
$reportFile = [IO.Path]::GetFullPath($ReportPath)
if (-not $reportFile.StartsWith($workspacePath + '\reports\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Report must be inside workspace reports.' }
if (Test-Path -LiteralPath $reportFile) { throw 'Preserve existing report.' }
if (Test-Path -LiteralPath ($reportFile + '.plan.json')) { throw 'Preserve existing removal plan.' }
foreach ($directory in @($workspacePath, $targetPath, $debugPath, $depsPath, (Split-Path $reportFile -Parent))) {
    $item = Get-Item -LiteralPath $directory -Force
    if (-not $item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw "Not an ordinary directory: $directory" }
}
$active = @(Get-Process | Where-Object {
    $_.ProcessName -match '^(cargo|rustc|clippy-driver)$' -or
    ($_.Path -and $_.Path.StartsWith($targetPath + '\', [StringComparison]::OrdinalIgnoreCase))
})
if ($active.Count) { throw 'A compiler or target executable is active; retry after it exits.' }
$guards = @()
try {
    foreach ($name in @('.cargo-lock', '.cargo-build-lock', '.cargo-artifact-lock')) {
        $lockPath = Join-Path $debugPath $name
        $item = Get-Item -LiteralPath $lockPath -Force
        if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Unexpected lock link.' }
        $guards += [IO.File]::Open($lockPath, [IO.FileMode]::Open, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
    }
    $cutoff = [DateTime]::UtcNow.AddDays(-$MinimumAgeDays)
    $symbols = @(Get-ChildItem -LiteralPath $depsPath -File -Filter '*.pdb' | Where-Object { $_.Name -match '^(.+)-[0-9a-f]{16}\.pdb$' })
    $recent = @{}
    # Preserve the newest two variants of every executable/library family.
    foreach ($group in ($symbols | Group-Object { $_.Name -replace '-[0-9a-f]{16}\.pdb$', '' })) {
        foreach ($file in ($group.Group | Sort-Object LastWriteTimeUtc -Descending | Select-Object -First 2)) { $recent[$file.Name] = $true }
    }
    $candidates = @($symbols | Where-Object { $_.LastWriteTimeUtc -lt $cutoff -and -not $recent.ContainsKey($_.Name) })
    $references = @{}
    # Protect symbol names explicitly cited in durable JSON/Markdown evidence.
    # Build logs can cite executables; no executable is a deletion candidate.
    foreach ($folder in @('reports', 'docs', 'handoffs')) {
        foreach ($file in (Get-ChildItem -LiteralPath (Join-Path $workspacePath $folder) -Recurse -File | Where-Object { $_.Extension -in @('.json', '.md') })) {
            $contents = [IO.File]::ReadAllText($file.FullName, [Text.Encoding]::UTF8)
            foreach ($match in [regex]::Matches($contents, '[A-Za-z0-9_]+-[0-9a-f]{16}\.pdb')) { $references[$match.Value] = $true }
        }
    }
    $removed = @(); $protected = @(); $chosen = @()
    foreach ($file in $candidates) {
        if ($references.ContainsKey($file.Name)) { $protected += $file.Name; continue }
        $resolved = [IO.Path]::GetFullPath($file.FullName)
        if ((Split-Path $resolved -Parent) -ne $depsPath -or $file.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Candidate escaped ordinary deps directory.' }
        $chosen += @{path=$resolved; bytes=$file.Length; lastWriteUtc=$file.LastWriteTimeUtc.ToString('o')}
    }
    $freeBefore = (Get-PSDrive -Name ([IO.Path]::GetPathRoot($workspacePath).Substring(0,1))).Free
    [IO.File]::WriteAllText(($reportFile + '.plan.json'), (@{at=[DateTimeOffset]::Now.ToString('o'); apply=[bool]$Apply; candidates=$chosen} | ConvertTo-Json -Depth 8), [Text.UTF8Encoding]::new($false))
    foreach ($entry in $chosen) {
        if ($Apply) {
            $current = Get-Item -LiteralPath $entry.path -Force
            if ($current.Length -ne $entry.bytes -or $current.LastWriteTimeUtc.ToString('o') -ne $entry.lastWriteUtc -or ($current.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Candidate changed during audit.' }
            Remove-Item -LiteralPath $entry.path -Force
            $removed += $entry
        }
    }
    $report = @{
        at=[DateTimeOffset]::Now.ToString('o'); apply=[bool]$Apply; minimumAgeDays=$MinimumAgeDays;
        scope=$depsPath; protectedReferences=$protected; candidates=$chosen; removed=$removed;
        candidateBytes=($chosen | Measure-Object bytes -Sum).Sum;
        removedLogicalBytes=($removed | Measure-Object bytes -Sum).Sum;
        freeDiskBefore=$freeBefore; freeDiskAfter=(Get-PSDrive -Name ([IO.Path]::GetPathRoot($workspacePath).Substring(0,1))).Free;
        notes=@('Only old unreferenced PDB symbols; newest two variants per family retained.', 'No exe/dll/rlib/rmeta/fingerprint, root debug symbols, release artifact or evidence removed.', 'Logical sizes may include hard links. Old removed symbols are unavailable for debugging until rebuilt.', 'All existing Cargo locks held exclusively. No key, trust or network operation.')
    }
    [IO.File]::WriteAllText($reportFile, ($report | ConvertTo-Json -Depth 8), [Text.UTF8Encoding]::new($false))
    @{report=$reportFile; apply=[bool]$Apply; candidates=$chosen.Count; removed=$removed.Count; bytes=$report.removedLogicalBytes; protected=$protected.Count} | ConvertTo-Json -Compress
} finally { foreach ($guard in $guards) { $guard.Dispose() } }
