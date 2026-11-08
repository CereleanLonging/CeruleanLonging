param([switch]$ProbeOnly,[string]$TestExecutable)
$ErrorActionPreference='Stop'
foreach($module in @('Microsoft.PowerShell.Management','Microsoft.PowerShell.Utility','Microsoft.PowerShell.Security','Microsoft.PowerShell.LocalAccounts','CimCmdlets')) {
    Import-Module (Join-Path $PSHOME ('Modules/'+$module+'/'+$module+'.psd1')) -ErrorAction Stop
}
if($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted' -or $env:CERULEAN_BUILD_SCHEDULE_ACTIVE -ne '1') {
    throw 'This native fixture requires a managed GitHub-hosted build job'
}
if(-not $ProbeOnly -and (-not $env:CERULEAN_CACHE_SESSION -or -not $TestExecutable)) {
    throw 'Managed desktop test artifact required'
}
$root=Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
Add-Type -Path @((Join-Path $PSScriptRoot 'core-test-job.cs'),(Join-Path $PSScriptRoot 'standard-test-process.cs'))
$cursor=Get-CimInstance Win32_Process -Filter ('ProcessId='+$PID)
$guard=$null
for($index=0;$index -lt 12 -and $cursor.ParentProcessId;$index++){
    $cursor=Get-CimInstance Win32_Process -Filter ('ProcessId='+$cursor.ParentProcessId)
    if($cursor.Name -ieq 'powershell.exe' -and $cursor.CommandLine -match 'build-resource-guard\.ps1'){$guard=$cursor;break}
}
if(-not $guard){throw 'Owned scheduler ancestor unavailable'}
[CeruleanCiCoreJob]::AttachOwnedGuard([uint32]$guard.ProcessId)
$identifier=[Guid]::NewGuid().ToString('N')
$username='CLImport'+$identifier.Substring(0,12)
$fixture=Join-Path $root ('reports/standard-import-ci-'+$identifier)
$password=[Guid]::NewGuid().ToString('N')+'aA!7'
$secure=ConvertTo-SecureString $password -AsPlainText -Force
$account=$null
$code=1
$cleaned=$false
try {
    $null=[IO.Directory]::CreateDirectory($fixture)
    $account=New-LocalUser -Name $username -Password $secure -Description ('CeruleanLonging CI '+$identifier)
    Add-LocalGroupMember -SID 'S-1-5-32-545' -Member $account.SID
    $acl=Get-Acl -LiteralPath $fixture
    $acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($account.SID,'Modify','ContainerInherit,ObjectInherit','None','Allow'))
    Set-Acl -LiteralPath $fixture -AclObject $acl
    if($ProbeOnly){
        $program=Join-Path $PSHOME 'powershell.exe'
    } else {
        $source=[IO.Path]::GetFullPath($TestExecutable)
        $targetRoot=$env:CARGO_TARGET_DIR
        if($targetRoot.StartsWith('\\?\')){$targetRoot=$targetRoot.Substring(4)}
        if(-not $source.StartsWith([IO.Path]::GetFullPath($targetRoot)+'\',[StringComparison]::OrdinalIgnoreCase) -or (Split-Path -Leaf $source) -notmatch '^file_import-[a-f0-9]+\.exe$') {throw 'Unexpected file-import test binary'}
        $program=Join-Path $fixture 'file-import-tests.exe'
        Copy-Item -LiteralPath $source -Destination $program
        if((Get-FileHash -LiteralPath $program).Hash -cne (Get-FileHash -LiteralPath $source).Hash){throw 'Test copy differs from compiled artifact'}
    }
    $code=[CeruleanStandardTestProcess]::Run($username,$password,$account.SID.Value,$program,$fixture,[bool]$ProbeOnly)
    if($code -ne 0){throw ('Ordinary-account desktop test failed with exit '+$code)}
} finally {
    $password=$null
    $secure.Dispose()
    [CeruleanCiCoreJob]::Close()
    if($account){
        $current=Get-LocalUser -SID $account.SID
        if($current.Name -cne $username -or $current.Description -cne ('CeruleanLonging CI '+$identifier)){throw 'Native fixture account identity changed'}
        $profile=Get-CimInstance Win32_UserProfile -Filter ("SID='"+$account.SID.Value+"'")
        $deadline=[DateTime]::UtcNow.AddSeconds(10)
        while($profile -and $profile.Loaded -and [DateTime]::UtcNow -lt $deadline){
            Start-Sleep -Milliseconds 200
            $profile=Get-CimInstance Win32_UserProfile -Filter ("SID='"+$account.SID.Value+"'")
        }
        if($profile){
            if($profile.Loaded){throw ('Native test profile remains loaded for '+$username)}
            $profiles=[Environment]::ExpandEnvironmentVariables((Get-ItemProperty -LiteralPath 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\ProfileList').ProfilesDirectory)
            if([IO.Path]::GetFullPath($profile.LocalPath) -ine [IO.Path]::GetFullPath((Join-Path $profiles $username))){throw 'Native profile path changed'}
            Remove-CimInstance -InputObject $profile
        }
        Remove-LocalUser -SID $account.SID
        if(Get-LocalUser -Name $username -ErrorAction SilentlyContinue){throw 'Native test account cleanup unresolved'}
        $cleaned=$true
    }
    $record=@{passed=($code -eq 0);accountRemoved=$cleaned;ordinaryAccount=$true;probeOnly=[bool]$ProbeOnly;testExit=$code}
    $record|ConvertTo-Json -Compress|Write-Host
    if($cleaned){
        $resolved=[IO.Path]::GetFullPath($fixture)
        if(-not $resolved.StartsWith([IO.Path]::GetFullPath((Join-Path $root 'reports'))+'\',[StringComparison]::OrdinalIgnoreCase) -or (Split-Path -Leaf $resolved) -cne ('standard-import-ci-'+$identifier)){throw 'Fixture cleanup escaped its owned directory'}
        Remove-Item -LiteralPath $resolved -Recurse -Force
    }
}
exit $code
