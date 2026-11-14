$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')
# Prerequisites: Rust MSVC, Visual Studio C++ tools, Node.js and Python 3.12.
# This script does not install system components or download OCR models.
python -m venv .venv
if ($LASTEXITCODE -ne 0) { throw 'Python environment failed' }
& .venv\Scripts\python.exe -m pip install -r workers\requirements.txt
if ($LASTEXITCODE -ne 0) { throw 'Python dependencies failed' }
npm ci
if ($LASTEXITCODE -ne 0) { throw 'npm ci failed' }
cargo test -p localfind-core
if ($LASTEXITCODE -ne 0) { throw 'Core tests failed' }
npm run build
if ($LASTEXITCODE -ne 0) { throw 'Frontend build failed' }
$env:LOCALFIND_PYTHON = (Resolve-Path .venv\Scripts\python.exe).Path
npm run bundle
if ($LASTEXITCODE -ne 0) { throw 'Installer build failed' }
Write-Host 'Unsigned development installer built. Windows manual acceptance still required.'
