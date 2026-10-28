$ErrorActionPreference='Stop'
Set-Location -LiteralPath (Split-Path -Parent $PSScriptRoot)
$env:PATH="$(Join-Path $env:USERPROFILE '.cargo\bin');$env:PATH"
npm.cmd run desktop:build
exit $LASTEXITCODE
