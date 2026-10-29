$ErrorActionPreference = 'Stop'
$reportRoot = 'D:\test\CeruleanLonging\reports\qoder-migration-226'
$prepared = Get-Content -LiteralPath "$reportRoot\prepared.json" -Raw -Encoding UTF8 | ConvertFrom-Json
if ($prepared.status -ne 'copied-verified-not-switched' -or $prepared.roots.Count -ne 5) {
    throw 'Missing verified preparation'
}
if (Get-Process -Name Qoder -ErrorAction SilentlyContinue) { throw 'Qoder must remain closed' }
$mappings = @(
    @('E:\千问\Qoder', 'D:\Qoder\app'),
    @('E:\qoder-data', 'D:\Qoder\data'),
    @('C:\Users\姚\.qoder', 'D:\Qoder\user-profile'),
    @('C:\Users\姚\AppData\Local\Qoder', 'D:\Qoder\local'),
    @('C:\Users\姚\AppData\Roaming\Qoder', 'D:\Qoder\roaming')
)
foreach ($mapping in $mappings) {
    $source = $mapping[0]
    $destination = $mapping[1]
    $receipt = @($prepared.roots | Where-Object { $_.source -eq $source -and $_.destination -eq $destination })
    if ($receipt.Count -ne 1 -or -not $receipt[0].sha256Verified -or -not $receipt[0].sourceUnchanged) {
        throw 'Receipt does not match explicit path'
    }
    $sourceItem = Get-Item -LiteralPath $source
    $destinationItem = Get-Item -LiteralPath $destination
    if ($sourceItem.FullName -ne $source -or $destinationItem.FullName -ne $destination -or
        ($sourceItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -or
        ($destinationItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        throw 'Resolved target is not the explicitly named real directory'
    }
    if (Test-Path -LiteralPath "$source.migration-226-backup") { throw 'Backup already exists' }
}
$switched = @()
foreach ($mapping in $mappings) {
    if (Get-Process -Name Qoder -ErrorAction SilentlyContinue) { throw 'Qoder reopened; stop before next switch' }
    $source = $mapping[0]
    $destination = $mapping[1]
    $backup = "$source.migration-226-backup"
    # Targets were checked above against the five explicitly named source roots.
    Move-Item -LiteralPath $source -Destination $backup
    try {
        New-Item -ItemType Junction -Path $source -Target $destination | Out-Null
    } catch {
        if (-not (Test-Path -LiteralPath $source)) {
            Move-Item -LiteralPath $backup -Destination $source
        }
        throw
    }
    $switched += [pscustomobject]@{ source=$source; destination=$destination; backup=$backup }
    $switched | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath "$reportRoot\switch-progress.json" -Encoding UTF8
}
@{
    status='switched-backups-preserved'
    completedAt=(Get-Date -Format o)
    roots=$switched
    realProviderCalls=0
} | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath "$reportRoot\switched.json" -Encoding UTF8
'All five roots redirected; original copies preserved for rollback'
