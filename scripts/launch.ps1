$ErrorActionPreference='Stop'
$ceruleanRoot=Split-Path -Parent $PSScriptRoot
$ceruleanExe=Join-Path $ceruleanRoot 'target\release\CeruleanLonging.exe'
if (!(Test-Path -LiteralPath $ceruleanExe)) {
    throw 'Desktop executable is not built. Run scripts/build.ps1 first.'
}
# This is the interactive desktop application the user is launching.
Start-Process -FilePath $ceruleanExe -WorkingDirectory $ceruleanRoot
