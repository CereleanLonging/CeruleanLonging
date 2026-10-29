$ErrorActionPreference = 'Stop'
$scope = 'D:\test\CeruleanLonging\target'
$report = 'D:\test\CeruleanLonging\reports\hyperv-acceptance-356'
$receipt = Join-Path $report 'cache-cleanup-result.json'
if (Test-Path -LiteralPath $receipt) { throw 'Receipt exists; do not replay.' }
$plan = Get-Content -LiteralPath (Join-Path $report 'cache-cleanup-plan.json') -Raw -Encoding UTF8 | ConvertFrom-Json
if ($plan.scope -ne $scope) { throw 'Scope mismatch.' }
function Assert-Idle {
    $active = @(Get-Process | Where-Object { $_.ProcessName -match '^(cargo|rustc|rustdoc|link|cl|cmake|ninja)$|^cerulean|baseline_experiment' })
    if ($active.Count) { throw 'Build or product process active.' }
}
function Assert-Candidate($item) {
    $full = [IO.Path]::GetFullPath([string]$item.path)
    if (-not $full.StartsWith($scope + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Outside cache boundary.' }
    $parent = [IO.Path]::GetDirectoryName($full)
    while ($parent) {
        if ((Get-Item -LiteralPath $parent).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Reparse ancestor.' }
        $parent = [IO.Path]::GetDirectoryName($parent)
    }
    $file = Get-Item -LiteralPath $full
    if ($file.PSIsContainer -or ($file.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Nonordinary file.' }
    if ($file.Extension -notin @('.pdb','.rlib','.rmeta','.o','.obj')) { throw 'Unapproved extension.' }
    # The frozen plan field stores Windows FILETIME, not DateTime.Ticks.
    if ($file.Length -ne $item.bytes -or $file.LastWriteTimeUtc.ToFileTimeUtc() -ne $item.modifiedUtcTicks) { throw 'Candidate changed.' }
    return $full
}
Assert-Idle
foreach ($item in $plan.items) { $null = Assert-Candidate $item }
$removed = [Collections.Generic.List[object]]::new()
$failure = $null
try {
    foreach ($item in $plan.items) {
        Assert-Idle
        $path = Assert-Candidate $item
        Remove-Item -LiteralPath $path -ErrorAction Stop
        $removed.Add([pscustomobject]@{path=$path;bytes=$item.bytes})
    }
} catch { $failure = $_.Exception.Message }
finally {
    $result = [ordered]@{at=[DateTimeOffset]::Now.ToString('o');scope=$scope;completed=($null -eq $failure);error=$failure;removedFiles=$removed.Count;removedLogicalBytes=($removed | Measure-Object bytes -Sum).Sum;removed=$removed;sourceReportsExecutablesDllsExcluded=$true}
    $stream = [IO.File]::Open($receipt,[IO.FileMode]::CreateNew)
    try {
        $bytes = [Text.UTF8Encoding]::new($false).GetBytes(($result | ConvertTo-Json -Depth 6))
        $stream.Write($bytes,0,$bytes.Length)
        $stream.Flush($true)
    } finally { $stream.Dispose() }
    [pscustomobject]$result | Select-Object completed,removedFiles,removedLogicalBytes,error | ConvertTo-Json -Compress
}
if ($failure) { throw $failure }
