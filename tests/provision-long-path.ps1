$ErrorActionPreference='Stop'
Set-StrictMode -Version Latest
Import-Module (Join-Path $PSHOME 'Modules/Microsoft.PowerShell.Security/Microsoft.PowerShell.Security.psd1') -ErrorAction Stop
if (-not $env:CERULEAN_CACHE_SESSION) { throw 'Managed cache session required' }
$source=Join-Path (Split-Path -Parent $PSScriptRoot) 'scripts/provision-worker-accounts.ps1'
$tokens=$null
$errors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile($source,[ref]$tokens,[ref]$errors)
if ($errors.Count) { throw 'Provisioning script parse failed' }
foreach ($name in @('Filesystem-Path','Plain-Path','Protect-Path','Protect-Tree')) {
    $functions=@($ast.FindAll({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst]},$true) | Where-Object Name -CEQ $name)
    if ($functions.Count -ne 1) { throw 'Production function is ambiguous' }
    . ([ScriptBlock]::Create($functions[0].Extent.Text))
}
$fixture=Join-Path $env:CERULEAN_CACHE_SESSION ('provision-path-'+[Guid]::NewGuid().ToString('N'))
$long=$fixture
while ($long.Length -lt 290) { $long=Join-Path $long 'long-parent-01234567890123456789' }
$long=Join-Path $long 'literal [directory]'
$null=[IO.Directory]::CreateDirectory((Filesystem-Path $long))
if ((Plain-Path $long) -cne $long) { throw 'Plain account identity changed' }
if ((Plain-Path (Filesystem-Path $long)) -cne $long) { throw 'Extended identity canonicalization changed' }
if ((Filesystem-Path (Filesystem-Path $long)) -cne (Filesystem-Path $long)) { throw 'Extended prefix duplicated' }
$file=Join-Path $long 'literal [record].json'
[IO.File]::WriteAllText((Filesystem-Path $file),'{"fixture":true}')
$script:plan=@{ownerSid=[Security.Principal.WindowsIdentity]::GetCurrent().User.Value}
$script:accounts=@()
Protect-Tree $long ''
foreach ($path in @($long,$file)) {
    $item=Get-Item -LiteralPath (Filesystem-Path $path) -Force
    $acl=Get-Acl -LiteralPath (Filesystem-Path $path)
    if (-not $acl.AreAccessRulesProtected) { throw 'Long path ACL was not protected' }
    $rules=$acl.GetAccessRules($true,$false,[Security.Principal.SecurityIdentifier])
    $sids=@($rules | ForEach-Object {$_.IdentityReference.Value} | Sort-Object -Unique)
    $expected=@($script:plan.ownerSid,'S-1-5-18','S-1-5-32-544' | Sort-Object -Unique)
    if (Compare-Object $sids $expected) { throw 'Long path ACL includes unexpected identities' }
}
if ([IO.File]::ReadAllText((Filesystem-Path $file)) -cne '{"fixture":true}') { throw 'Long path content changed' }
foreach ($path in @('relative-path','C:\','\\server\share\path')) {
    $rejected=$false
    try { $null=Plain-Path $path } catch { $rejected=$true }
    if (-not $rejected) { throw 'Unsupported account path was accepted' }
}
@{passed=$true;shell=$PSVersionTable.PSVersion.ToString();longPathCharacters=$long.Length;cases=7;accountsCreated=0;modelCalls=0}|ConvertTo-Json -Compress
