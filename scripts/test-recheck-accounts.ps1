param([string]$TestExecutable,[string]$CoreExecutable)
$ErrorActionPreference='Stop'
$identity=[Security.Principal.WindowsIdentity]::GetCurrent()
$principal=[Security.Principal.WindowsPrincipal]::new($identity)
if(-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){
    throw 'Explicit administrator launch is required for temporary native test accounts'
}
if(-not $env:CERULEAN_CACHE_SESSION -or $env:CERULEAN_BUILD_SCHEDULE_ACTIVE -ne '1'){
    throw 'Run this native suite through the managed build/test scheduler'
}
$cleanup=Join-Path (Split-Path -Parent $PSScriptRoot) 'crates\core\tests\strict_workflow\recheck-account-cleanup.ps1'
$tokens=$null
$parseErrors=$null
$cleanupAst=[Management.Automation.Language.Parser]::ParseFile($cleanup,[ref]$tokens,[ref]$parseErrors)
if($parseErrors -or -not $cleanupAst.ParamBlock){throw 'Account cleanup script failed preflight before account creation'}
$previousRoot=$env:CERULEAN_NATIVE_RECHECK_ROOT
$evidenceRoot=Join-Path (Split-Path -Parent $PSScriptRoot) ('reports\native-recheck-accounts-'+[Guid]::NewGuid().ToString('N'))
$null=[IO.Directory]::CreateDirectory($evidenceRoot)
$previous=$env:CERULEAN_NATIVE_RECHECK_ACCOUNTS
$previousCore=$env:CERULEAN_RECHECK_TEST_CORE
$filter=@('--ignored','recheck_ledger::','--test-threads=1',
    '--skip','recheck_ledger::runner::corrected_checker',
    '--skip','recheck_ledger::runner::failed_checker',
    '--skip','recheck_ledger::runner::slow_checker',
    '--skip','recheck_ledger::disposition::crash_host')
try {
    $env:CERULEAN_NATIVE_RECHECK_ACCOUNTS='1'
    $env:CERULEAN_NATIVE_RECHECK_ROOT=$evidenceRoot
    if($TestExecutable){
        if(-not $CoreExecutable){throw 'Staged native tests require the matching core executable'}
        $env:CERULEAN_RECHECK_TEST_CORE=(Resolve-Path -LiteralPath $CoreExecutable).Path
        & (Resolve-Path -LiteralPath $TestExecutable).Path @filter
    } else {
        cargo test -p cerulean-core --test strict_workflow --locked -- @filter
    }
    $code=$LASTEXITCODE
} finally {
    $env:CERULEAN_NATIVE_RECHECK_ROOT=$previousRoot
    $env:CERULEAN_NATIVE_RECHECK_ACCOUNTS=$previous
    if(@(Get-ChildItem -LiteralPath $evidenceRoot -Force).Count -eq 0){[IO.Directory]::Delete($evidenceRoot,$false)}
    else {Write-Warning ('Account cleanup evidence retained at '+$evidenceRoot)}
    $env:CERULEAN_RECHECK_TEST_CORE=$previousCore
}
exit $code
