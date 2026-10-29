$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$run=Join-Path $root 'reports\backup-interrupt-window-native-627d1a0102964f8fb50dc5fd8e0188db'
$install=Join-Path $run 'installed app';$data=Join-Path $run 'data';$stage=Join-Path $run 'maintenance'
$product='HKCU:\Software\ceruleanlonging\CeruleanLonging'
$uninstall='HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\CeruleanLonging'
$prior=Get-Content -LiteralPath (Join-Path $run 'result.json') -Raw -Encoding UTF8|ConvertFrom-Json
if($prior.status -ne 'failed' -or $prior.calls[-1].action -ne 'coreReview' -or (Get-ChildItem (Join-Path $stage 'operations') -Filter '*.request.json').Count -ne 1){throw 'Expected only one read-only review'}
$pidRecord=Get-CimInstance Win32_Process -Filter ('ProcessId = '+$prior.windowPid)
if(-not $pidRecord -or $pidRecord.Name -ne 'powershell.exe' -or -not $pidRecord.CommandLine.Contains((Join-Path $stage 'maintenance-window.ps1'))){throw 'Original fixture window identity differs'}
$p=Get-Process -Id $prior.windowPid;$null=$p.Handle
if(-not $p.CloseMainWindow() -or -not $p.WaitForExit(10000)){throw 'Original window remains; no new process will be launched'}
$p.Dispose()
$null=& (Join-Path $install 'cerulean-runtime.exe') --workbench --data-dir $data service stop
if($LASTEXITCODE -ne 0){throw 'Original isolated core did not stop'}
if([IO.Path]::GetFullPath($install) -ine (Join-Path $run 'installed app') -or (Get-Item -LiteralPath $product).GetValue('') -ine $install){throw 'Test install ownership differs'}
$runner=Join-Path $run 'test-uninstall.exe';Copy-Item -LiteralPath (Join-Path $install 'uninstall.exe') -Destination $runner
$p=Start-Process -FilePath $runner -ArgumentList @('/S',"_?=$install") -WindowStyle Hidden -PassThru;$null=$p.Handle
if(-not $p.WaitForExit(60000) -or $p.ExitCode -ne 0){throw 'Original uninstaller pending or failed; do not repeat'}
$p.Dispose()
if(Test-Path -LiteralPath $uninstall){throw 'Test uninstall registration remains'}
if(Test-Path -LiteralPath $product){if((Get-Item -LiteralPath $product).GetValue('') -ine $install){throw 'Owner changed'};Remove-Item -LiteralPath $product -Recurse}
$r=@{status='passed';run=$run;cleanup='passed';originalReadOnlyReviewAttempts=1;prepareAttempts=0;noActionReplay=$true;cause='new capture used locale decoder for UTF-8 native JSON; fixed before retry'}
[IO.File]::WriteAllText((Join-Path $run 'cleanup-result.json'),($r|ConvertTo-Json),[Text.UTF8Encoding]::new($false))
$r|ConvertTo-Json -Compress
