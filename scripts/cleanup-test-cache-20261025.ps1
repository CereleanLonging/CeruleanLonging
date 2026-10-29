$ErrorActionPreference = 'Stop'
$workspacePath = 'D:\test\CeruleanLonging'
$cachePath = 'D:\test\CeruleanLonging\target\debug\deps'
$candidatePath = Join-Path $workspacePath 'reports\target-cleanup-candidates-20261025.json'
$resultPath = Join-Path $workspacePath 'reports\target-cleanup-result-20261025.json'
if (Test-Path -LiteralPath $resultPath) { throw 'Cleanup receipt exists; inspect it, do not rerun.' }
$resolvedCache = (Resolve-Path -LiteralPath $cachePath).ProviderPath
if (-not $resolvedCache.Equals($cachePath, [StringComparison]::OrdinalIgnoreCase)) { throw 'Unexpected resolved target.' }
foreach ($parentPath in @($workspacePath, (Join-Path $workspacePath 'target'), (Join-Path $workspacePath 'target\debug'), $cachePath)) {
    if ((Get-Item -LiteralPath $parentPath).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Reparse parent rejected.' }
}
$activeBuilds = @(Get-Process | Where-Object { $_.ProcessName -match '^(cargo|rustc|rustdoc|link|cl|cmake|ninja)$|^cerulean|baseline_experiment' })
if ($activeBuilds.Count) { throw 'Build or product process active; no deletion.' }
$plan = Get-Content -LiteralPath $candidatePath -Raw -Encoding UTF8 | ConvertFrom-Json
if ($plan.scope -ne $cachePath) { throw 'Cleanup scope mismatch.' }
$cutoff = [DateTimeOffset]::Parse($plan.cutoff).UtcDateTime
$selected = @()
foreach ($candidate in $plan.candidates) {
    $full = [IO.Path]::GetFullPath([string]$candidate.path)
    if (-not ([IO.Path]::GetDirectoryName($full)).Equals($resolvedCache, [StringComparison]::OrdinalIgnoreCase)) { throw 'Candidate outside exact cache directory.' }
    $file = Get-Item -LiteralPath $full
    if ($file.PSIsContainer -or ($file.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Nonordinary candidate.' }
    if ($file.Extension -notin @('.pdb', '.rlib', '.rmeta', '.o')) { throw 'Unexpected file type.' }
    if ($file.Length -ne $candidate.bytes -or $file.LastWriteTimeUtc -ge $cutoff) { throw 'Candidate changed; no deletion.' }
    $selected += $file
}
$removed = [Collections.Generic.List[object]]::new()
$cleanupError = $null
try {
    foreach ($file in $selected) {
        $current = Get-Item -LiteralPath $file.FullName
        if ($current.Length -ne $file.Length -or $current.LastWriteTimeUtc -ne $file.LastWriteTimeUtc) { throw 'Candidate changed during cleanup.' }
        Remove-Item -LiteralPath $file.FullName -ErrorAction Stop
        $removed.Add([pscustomobject]@{path=$file.FullName;bytes=$file.Length})
    }
} catch {
    $cleanupError = $_.Exception.Message
} finally {
    $receipt = [ordered]@{scope=$resolvedCache;cutoff=$plan.cutoff;completed=($null -eq $cleanupError);error=$cleanupError;removedFiles=$removed.Count;removedLogicalBytes=($removed | Measure-Object -Property bytes -Sum).Sum;removed=$removed;sourceAndReportsUntouched=$true;executablesAndDllsUntouched=$true;newerCachePreserved=$true}
    [IO.File]::WriteAllText($resultPath, ($receipt | ConvertTo-Json -Depth 5), [Text.UTF8Encoding]::new($false))
    [pscustomobject]@{completed=$receipt.completed;removedFiles=$receipt.removedFiles;removedLogicalBytes=$receipt.removedLogicalBytes;error=$cleanupError} | ConvertTo-Json -Compress
}
if ($cleanupError) { throw $cleanupError }
