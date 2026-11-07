param([Parameter(Mandatory=$true)][string]$PlanPath)
$ErrorActionPreference='Stop'
$planFile=[IO.Path]::GetFullPath($PlanPath)
$root=Split-Path -Parent $planFile
$plan=ConvertFrom-Json ([IO.File]::ReadAllText($planFile))
$identity=[Security.Principal.WindowsIdentity]::GetCurrent()
if($plan.schemaVersion -ne 1 -or $plan.ownerSid -cne $identity.User.Value -or $plan.projectId -cne 'project' -or @($plan.slots).Count -ne 1){throw 'Native fixture identity mismatch'}
foreach($path in @($plan.dataRoot,$plan.sharedRoot,$plan.chiefRoot)){
    if($path.StartsWith('\\?\')){$path=$path.Substring(4)}
    $full=[IO.Path]::GetFullPath($path)
    if(-not $full.StartsWith($root+'\',[StringComparison]::OrdinalIgnoreCase)){throw 'Native fixture escaped its owned root'}
}
$slot=$plan.slots[0]
if($slot.identity -cnotmatch '^[a-f0-9]{64}$' -or $slot.username -cnotmatch '^CLW[a-f0-9]{16}$'){throw 'Invalid native account'}
$resultFile=[IO.Path]::ChangeExtension($planFile,'result.json')
if(-not (Test-Path -LiteralPath $resultFile)){throw 'Provisioning receipt missing; preserve fixture'}
$result=ConvertFrom-Json ([IO.File]::ReadAllText($resultFile))
$user=Get-LocalUser -Name $slot.username -ErrorAction SilentlyContinue
if(-not $user){return}
if($slot.username -cnotin @($result.created) -or $user.Description -cne ('CeruleanLonging '+$slot.identity.Substring(0,32))){throw 'Account was not created by this fixture'}
$recordFile=Join-Path $plan.dataRoot ('worker-accounts\project\'+$slot.identity+'\account.json')
if(Test-Path -LiteralPath $recordFile){
    $record=ConvertFrom-Json ([IO.File]::ReadAllText($recordFile))
    if($record.sid -cne $user.SID.Value -or $record.username -cne $slot.username -or $record.ownerSid -cne $plan.ownerSid -or $record.dataRoot -cne $plan.dataRoot){throw 'Native account record changed'}
}
$profile=Get-CimInstance Win32_UserProfile -Filter ("SID='"+$user.SID.Value+"'")
if($profile){
    if($profile.Loaded){throw 'Native test account profile is still loaded'}
    $profilesDirectory=[Environment]::ExpandEnvironmentVariables((Get-ItemProperty -LiteralPath 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\ProfileList').ProfilesDirectory)
    $expectedProfile=[IO.Path]::GetFullPath((Join-Path $profilesDirectory $user.Name))
    if([IO.Path]::GetFullPath($profile.LocalPath) -ine $expectedProfile){throw 'Native profile path differs from this test account'}
    Remove-CimInstance -InputObject $profile
}
Remove-LocalUser -SID $user.SID
if(Get-LocalUser -Name $slot.username -ErrorAction SilentlyContinue){throw 'Native account removal unresolved'}
