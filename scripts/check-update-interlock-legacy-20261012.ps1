$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$transaction=Join-Path $root 'reports\update-transaction-native-0ea9ec463ba244adb23750f4e1a7c3f2\transaction-after-preserve'
$tool=Join-Path $PSScriptRoot 'manage-windows-update.ps1'
$planPath=Join-Path $transaction 'plan.json'
$digest=(Get-FileHash -LiteralPath $planPath).Hash.ToLowerInvariant()
if((Get-Content -LiteralPath $planPath -Raw | ConvertFrom-Json).maintenanceInterlockSchema){throw 'Fixture is not a legacy transaction'}
function Inventory {
    return ((Get-ChildItem -LiteralPath $transaction -Recurse -File | Sort-Object FullName | ForEach-Object {$_.FullName.Substring($transaction.Length)+':'+(Get-FileHash -LiteralPath $_.FullName).Hash}) -join "`n")
}
$before=Inventory
$review=& $tool -Action Review -TransactionDirectory $transaction -PlanDigest $digest | ConvertFrom-Json
if($review.status -cne 'passed' -or $null -ne $review.maintenance){throw 'Legacy readonly review changed meaning'}
$rejection=''
try{$null=& $tool -Action Rollback -TransactionDirectory $transaction -PlanDigest $digest -AllowUnsignedInternal}catch{$rejection=$_.Exception.Message}
if(-not $rejection.Contains('Legacy transaction requires its original controller')){throw "Unexpected legacy mutation result: $rejection"}
if((Inventory) -cne $before){throw 'Legacy transaction evidence changed'}
[ordered]@{status='passed';transaction=$transaction;planDigest=$digest;legacyReview=$review.state;mutationRejected=$rejection;allTransactionBytesUnchanged=$true;automaticAdoption=$false;modelCalls=0;userDesktopActions=0}|ConvertTo-Json -Depth 8 -Compress
