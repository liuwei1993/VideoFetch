# Run VideoFetch in development mode (Tauri + Vite) on Windows.
# Usage: powershell -NoProfile -ExecutionPolicy Bypass -File scripts\run-dev.ps1
#        scripts\run-dev.cmd
$ErrorActionPreference = "Stop"
. "$PSScriptRoot\windows-common.ps1"

Set-Location $script:VideoFetchRoot
Import-VideoFetchToolchain
Use-VideoFetchProxy
Invoke-VideoFetchSidecars
Install-VideoFetchNpmDeps

Write-Host "Starting VideoFetch (tauri dev)..."
npm run tauri -- dev @args
exit $LASTEXITCODE
