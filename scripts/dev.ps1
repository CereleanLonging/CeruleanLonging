$ErrorActionPreference='Stop'
$ceruleanRoot=Split-Path -Parent $PSScriptRoot
$ceruleanCargo=Join-Path $env:USERPROFILE '.cargo\bin'
$env:PATH="$ceruleanCargo;$env:PATH"
Set-Location -LiteralPath $ceruleanRoot
npm.cmd run desktop:dev
exit $LASTEXITCODE
