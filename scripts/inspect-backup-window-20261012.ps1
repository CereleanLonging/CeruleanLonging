$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
$root=Split-Path -Parent $PSScriptRoot
$run=Join-Path $root 'reports\backup-interrupt-window-native-90b8cec5a4574ada8c3c53ceffc69a30'
$stage=Join-Path $run 'maintenance'
$identity=Get-CimInstance Win32_Process -Filter 'ProcessId = 5992'
if(-not $identity -or $identity.Name -ne 'powershell.exe' -or -not $identity.CommandLine.Contains((Join-Path $stage 'maintenance-window.ps1'))){throw 'Original window identity differs; no other window will be inspected'}
$process=Get-Process -Id 5992
$handle=$process.Handle
$ui=[Windows.Automation.AutomationElement]::FromHandle($process.MainWindowHandle)
if($ui.Current.ProcessId -ne $process.Id){throw 'Window owner differs'}
$items=@($ui.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition) | ForEach-Object {
    $value=$null
    if($_.Current.ControlType -eq [Windows.Automation.ControlType]::Edit){$value=($_.GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern)).Current.Value}
    @{type=$_.Current.ControlType.ProgrammaticName;name=$_.Current.Name;enabled=$_.Current.IsEnabled;value=$value}
})
$result=@{at=[DateTimeOffset]::Now.ToString('o');pid=$process.Id;commandLine=$identity.CommandLine;readOnly=$true;items=$items}
[IO.File]::WriteAllText((Join-Path $run 'original-window-failure-inspection.json'),($result|ConvertTo-Json -Depth 6),[Text.UTF8Encoding]::new($false))
$items|Where-Object {$_.type -eq 'ControlType.Text'}|ConvertTo-Json -Depth 3
$process.Dispose()
