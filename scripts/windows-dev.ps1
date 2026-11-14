$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')
$env:LOCALFIND_PYTHON = (Resolve-Path .venv\Scripts\python.exe).Path
npm run desktop
