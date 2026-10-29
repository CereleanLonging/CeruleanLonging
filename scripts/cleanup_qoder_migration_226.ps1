$ErrorActionPreference = 'Stop'
$reportRoot = 'D:\test\CeruleanLonging\reports\qoder-migration-226'
$ready = Get-Content -LiteralPath "$reportRoot\cleanup-ready.json" -Raw -Encoding UTF8 | ConvertFrom-Json
if ($ready.status -ne 'original-backups-reverified-after-relaunch' -or $ready.roots.Count -ne 5) {
    throw 'Cleanup requires post-launch backup verification'
}
$expected = @(
    @('E:\千问\Qoder', 'D:\Qoder\app'),
    @('E:\qoder-data', 'D:\Qoder\data'),
    @('C:\Users\姚\.qoder', 'D:\Qoder\user-profile'),
    @('C:\Users\姚\AppData\Local\Qoder', 'D:\Qoder\local'),
    @('C:\Users\姚\AppData\Roaming\Qoder', 'D:\Qoder\roaming')
)
foreach ($mapping in $expected) {
    $source = $mapping[0]
    $destination = $mapping[1]
    $backup = "$source.migration-226-backup"
    $record = @($ready.roots | Where-Object { $_.backup -eq $backup -and $_.destination -eq $destination })
    if ($record.Count -ne 1 -or -not $record[0].unchanged) { throw 'Missing explicit verified backup' }
    $backupItem = Get-Item -LiteralPath $backup
    $sourceItem = Get-Item -LiteralPath $source
    $destinationItem = Get-Item -LiteralPath $destination
    if ($backupItem.FullName -ne $backup -or
        ($backupItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -or
        $sourceItem.LinkType -ne 'Junction' -or $sourceItem.Target -ne $destination -or
        ($destinationItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        throw 'Resolved paths do not match explicit migration roots'
    }
}
$removed = @()
foreach ($mapping in $expected) {
    $source = $mapping[0]
    $backup = "$source.migration-226-backup"
    if ($source -eq 'C:\Users\姚\.qoder') {
        foreach ($name in @('file-history','logs','projects','tmp')) {
            $linkPath = "$backup\$name"
            $linkItem = Get-Item -LiteralPath $linkPath
            if ($linkItem.LinkType -ne 'Junction' -or $linkItem.Target -ne "E:\qoder-data\$name") {
                throw 'Unexpected nested link; do not recurse'
            }
            # Non-recursive junction removal: never touch the active target.
            [IO.Directory]::Delete($linkPath)
        }
    }
    # Each absolute backup directory was validated above. All known nested
    # junctions are detached; the live source is a different directory entry.
    Remove-Item -LiteralPath $backup -Recurse -Force
    $removed += $backup
    $removed | ConvertTo-Json | Set-Content -LiteralPath "$reportRoot\cleanup-progress.json" -Encoding UTF8
}
@{ status='verified-original-copies-removed'; removed=$removed; completedAt=(Get-Date -Format o) } |
    ConvertTo-Json -Depth 4 |
    Set-Content -LiteralPath "$reportRoot\cleanup.json" -Encoding UTF8
'Verified redundant source copies removed; live junctions and D drive data retained'
