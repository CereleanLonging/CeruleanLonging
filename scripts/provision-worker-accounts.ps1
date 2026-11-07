param([Parameter(Mandatory=$true)][string]$PlanPath)
# Product-owned one-shot provisioning. No provider credentials are copied.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
Add-Type -AssemblyName System.Security
# Windows PowerShell 5.1 providers need the extended path form beyond MAX_PATH.
# Preserve plain canonical paths in account identities and serialized records.
function Filesystem-Path([string]$Path) {
    if ($Path.StartsWith('\\?\')) { return $Path }
    return '\\?\'+$Path
}
if ($PlanPath.StartsWith('\\?\')) { $PlanPath=$PlanPath.Substring(4) }
$planFile = [IO.Path]::GetFullPath($PlanPath)
$reportFile = [IO.Path]::ChangeExtension($planFile, 'result.json')
$intentFile = [IO.Path]::ChangeExtension($planFile, 'intent')
$intent = [IO.File]::Open((Filesystem-Path $intentFile), 'CreateNew', 'Write', 'None')
$intent.Dispose()
$created = @()
$ok = $false
$failure = $null
function Plain-Path([string]$Path) {
    if ($Path.StartsWith('\\?\')) { $Path=$Path.Substring(4) }
    if (-not [IO.Path]::IsPathRooted($Path)) { throw 'Absolute path required' }
    $full = [IO.Path]::GetFullPath($Path).TrimEnd('\')
    if ($full -notmatch '^[A-Za-z]:\\') { throw 'Local drive required for independent accounts' }
    if ($full -eq [IO.Path]::GetPathRoot($full).TrimEnd('\')) { throw 'Volume root forbidden' }
    $cursor = $full
    while ($cursor.Length -gt 3) {
        if (Test-Path -LiteralPath (Filesystem-Path $cursor)) {
            if ((Get-Item -Force -LiteralPath (Filesystem-Path $cursor)).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Reparse path forbidden' }
        }
        $cursor = [IO.Path]::GetDirectoryName($cursor)
    }
    return $full
}
function Protect-Path([string]$Path, [string]$WorkerSid, [bool]$Traverse) {
    $item = Get-Item -Force -LiteralPath (Filesystem-Path $Path)
    if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Reparse point in protected tree' }
    $isDir = $item.PSIsContainer
    if ($isDir) { $acl = New-Object Security.AccessControl.DirectorySecurity }
    else { $acl = New-Object Security.AccessControl.FileSecurity }
    $owner = New-Object Security.Principal.SecurityIdentifier($script:plan.ownerSid)
    $acl.SetOwner($owner)
    $acl.SetAccessRuleProtection($true, $false)
    $inherit = if ($isDir) { [Security.AccessControl.InheritanceFlags]'ContainerInherit,ObjectInherit' } else { [Security.AccessControl.InheritanceFlags]::None }
    foreach ($sidText in @($script:plan.ownerSid, 'S-1-5-18', 'S-1-5-32-544')) {
        $sid = New-Object Security.Principal.SecurityIdentifier($sidText)
        $rule = New-Object Security.AccessControl.FileSystemAccessRule($sid, 'FullControl', $inherit, 'None', 'Allow')
        $acl.AddAccessRule($rule)
    }
    if ($WorkerSid) {
        $sid = New-Object Security.Principal.SecurityIdentifier($WorkerSid)
        $rule = New-Object Security.AccessControl.FileSystemAccessRule($sid, 'Modify', $inherit, 'None', 'Allow')
        $acl.AddAccessRule($rule)
    }
    if ($Traverse) {
        foreach ($entry in $script:accounts) {
            $sid = New-Object Security.Principal.SecurityIdentifier($entry.sid)
            # Node/Electron resolves each parent with lstat before opening its
            # own files. Folder attributes allow that lookup without granting
            # directory listing or inherited access to shared source files.
            $rule = New-Object Security.AccessControl.FileSystemAccessRule($sid, 'Traverse,ReadAttributes', 'None', 'None', 'Allow')
            $acl.AddAccessRule($rule)
        }
    }
    Set-Acl -LiteralPath (Filesystem-Path $Path) -AclObject $acl
}
function Protect-Tree([string]$Path, [string]$WorkerSid) {
    # Enumerate before changing ACLs and refuse every reparse point.
    $entries = @()
    $queue = New-Object 'Collections.Generic.Queue[string]'
    $queue.Enqueue($Path)
    while ($queue.Count) {
        foreach ($item in @(Get-ChildItem -LiteralPath (Filesystem-Path $queue.Dequeue()) -Force -ErrorAction Stop)) {
            if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Reparse point in protected tree' }
            $entries += $item
            if ($entries.Count -gt 50000) { throw 'Protected tree exceeds setup limit' }
            if ($item.PSIsContainer) { $queue.Enqueue($item.FullName) }
        }
    }
    if ($entries.Count -gt 50000) { throw 'Protected tree exceeds setup limit' }
    foreach ($entry in $entries) { if ($entry.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Reparse point in protected tree' } }
    Protect-Path $Path $WorkerSid $false
    foreach ($entry in $entries) { Protect-Path $entry.FullName $WorkerSid $false }
}
try {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = New-Object Security.Principal.WindowsPrincipal($identity)
    if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw 'Administrator consent required' }
    # A PowerShell 7 parent can leave its module directories in PSModulePath.
    # Load the Windows PowerShell ACL module from this process's own PSHOME.
    Import-Module (Join-Path $PSHOME 'Modules/Microsoft.PowerShell.Security/Microsoft.PowerShell.Security.psd1') -ErrorAction Stop
    $script:plan = [IO.File]::ReadAllText((Filesystem-Path $planFile), [Text.Encoding]::UTF8) | ConvertFrom-Json
    if ($plan.schemaVersion -ne 1 -or $plan.ownerSid -ne $identity.User.Value) { throw 'Setup owner mismatch' }
    if (@($plan.slots).Count -lt 1 -or @($plan.slots).Count -gt 5) { throw 'One to five slots required' }
    if ($plan.projectId -notmatch '^[a-zA-Z0-9_.:-]{1,128}$') { throw 'Invalid project ID' }
    $data = Plain-Path $plan.dataRoot
    $shared = Plain-Path $plan.sharedRoot
    $chief = Plain-Path $plan.chiefRoot
    foreach ($a in @($data,$shared,$chief)) {
        if (-not (Test-Path -LiteralPath (Filesystem-Path $a) -PathType Container)) { throw 'Managed root missing' }
        foreach ($b in @($data,$shared,$chief)) {
            if ($a -ne $b -and $a.StartsWith($b+'\',[StringComparison]::OrdinalIgnoreCase)) { throw 'Managed roots overlap' }
        }
    }
    if (@($data,$shared,$chief | Select-Object -Unique).Count -ne 3) { throw 'Managed roots must differ' }
    $script:accounts = @()
    $seen = @{}
    foreach ($slot in $plan.slots) {
        if ($slot.identity -notmatch '^[a-f0-9]{64}$' -or $slot.username -notmatch '^CLW[a-f0-9]{16}$' -or $seen.ContainsKey($slot.identity)) { throw 'Invalid or duplicate identity' }
        $seen[$slot.identity] = $true
        $workspace = Plain-Path $slot.workspace
        if ($workspace -ne (Join-Path (Join-Path $shared 'executors') $slot.identity)) { throw 'Workspace mismatch' }
        $null = Get-Item -LiteralPath (Filesystem-Path $workspace) -Force
        $accountDir = Join-Path (Join-Path (Join-Path $data 'worker-accounts') $plan.projectId) $slot.identity
        $null = [IO.Directory]::CreateDirectory((Filesystem-Path $accountDir))
        Protect-Tree $accountDir ''
        $recordPath = Join-Path $accountDir 'account.json'
        $passwordPath = Join-Path $accountDir 'password.dpapi'
        $description = 'CeruleanLonging '+$slot.identity.Substring(0,32)
        $existing = Get-LocalUser -Name $slot.username -ErrorAction SilentlyContinue
        if ($existing) {
            if (-not (Test-Path -LiteralPath (Filesystem-Path $recordPath)) -or -not (Test-Path -LiteralPath (Filesystem-Path $passwordPath))) { throw 'Existing account without complete receipt; manual reconciliation required' }
            $record = [IO.File]::ReadAllText((Filesystem-Path $recordPath),[Text.Encoding]::UTF8) | ConvertFrom-Json
            if ($existing.Description -ne $description -or $record.sid -ne $existing.SID.Value -or $record.identity -ne $slot.identity -or $record.workspace -ne $slot.workspace -or $record.ownerSid -ne $plan.ownerSid -or $record.dataRoot -ne $plan.dataRoot -or $record.chiefRoot -ne $plan.chiefRoot -or $record.projectId -ne $plan.projectId) { throw 'Existing account binding mismatch' }
        } else {
            if ((Test-Path -LiteralPath (Filesystem-Path $recordPath)) -or (Test-Path -LiteralPath (Filesystem-Path $passwordPath))) { throw 'Account disappeared; refusing identity reuse' }
            $random = New-Object byte[] 48
            $rng = [Security.Cryptography.RandomNumberGenerator]::Create()
            $rng.GetBytes($random); $rng.Dispose()
            $textPassword = 'Aa1!'+[Convert]::ToBase64String($random)
            $passwordBytes = [Text.Encoding]::Unicode.GetBytes($textPassword)
            $protected = [Security.Cryptography.ProtectedData]::Protect($passwordBytes,$null,[Security.Cryptography.DataProtectionScope]::CurrentUser)
            $secure = ConvertTo-SecureString -String $textPassword -AsPlainText -Force
            [Array]::Clear($random,0,$random.Length); [Array]::Clear($passwordBytes,0,$passwordBytes.Length); $textPassword=$null
            $existing = New-LocalUser -Name $slot.username -Password $secure -Description $description -Disabled -PasswordNeverExpires -UserMayNotChangePassword
            $created += $slot.username
            $secure.Dispose()
            Add-LocalGroupMember -SID 'S-1-5-32-545' -Member $existing
            [IO.File]::WriteAllBytes((Filesystem-Path $passwordPath),$protected)
            $record = [ordered]@{schemaVersion=1;ownerSid=[string]$plan.ownerSid;projectId=[string]$plan.projectId;identity=[string]$slot.identity;sharedRoot=[string]$plan.sharedRoot;chiefRoot=[string]$plan.chiefRoot;dataRoot=[string]$plan.dataRoot;workspace=[string]$slot.workspace;username=[string]$slot.username;sid=[string]$existing.SID.Value}
            [IO.File]::WriteAllText((Filesystem-Path $recordPath),($record|ConvertTo-Json -Depth 4),[Text.UTF8Encoding]::new($false))
        }
        $script:accounts += $record
    }
    Protect-Tree $chief ''
    Protect-Tree $data ''
    # Shared deputy files stay host-only. Executors receive only their own tree.
    Protect-Tree $shared ''
    Protect-Path $shared '' $true
    Protect-Path (Join-Path $shared 'executors') '' $true
    foreach ($record in $accounts) {
        Protect-Tree (Plain-Path $record.workspace) $record.sid
        Enable-LocalUser -Name $record.username
    }
    $ok=$true
} catch {
    $failure=[string]$_.Exception.Message
    foreach ($name in $created) { Disable-LocalUser -Name $name -ErrorAction SilentlyContinue }
}
$receipt=[ordered]@{schemaVersion=1;success=$ok;created=@($created);error=$failure;finishedAt=[DateTime]::UtcNow.ToString('o');modelRequests=0}
$bytes=[Text.Encoding]::UTF8.GetBytes(($receipt|ConvertTo-Json -Depth 4))
$stream=[IO.File]::Open((Filesystem-Path $reportFile),'CreateNew','Write','None')
try {$stream.Write($bytes,0,$bytes.Length);$stream.Flush($true)} finally {$stream.Dispose()}
if (-not $ok) {exit 1}
