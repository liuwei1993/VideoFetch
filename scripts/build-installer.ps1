# Build a Windows NSIS installer for VideoFetch (yt-dlp / ffmpeg sidecars included).
# Usage: powershell -NoProfile -ExecutionPolicy Bypass -File scripts\build-installer.ps1
# Output: .build\VideoFetch_*_x64-setup.exe
$ErrorActionPreference = "Stop"
. "$PSScriptRoot\windows-common.ps1"

Set-Location $script:VideoFetchRoot
Import-VideoFetchToolchain
Use-VideoFetchProxy
Install-VideoFetchNpmDeps
Invoke-VideoFetchSidecars

$triple = "x86_64-pc-windows-msvc"
$hostLine = & rustc -vV | Select-String -Pattern "^host: "
if ($hostLine) {
    $triple = $hostLine.ToString().Substring(6).Trim()
}
if ($triple -notmatch "^x86_64-pc-windows-") {
    Write-VideoFetchFail "Unsupported Windows target: $triple (x86_64-pc-windows-* only)"
}

Write-Host "Building NSIS installer for $triple..."
npm run tauri -- build --bundles nsis
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}

$bundleDir = Join-Path $script:VideoFetchRoot "src-tauri\target\release\bundle\nsis"
$installers = @(Get-ChildItem -Path $bundleDir -Filter "*setup.exe" -File -ErrorAction SilentlyContinue | Sort-Object Name)
if ($installers.Count -eq 0) {
    Write-VideoFetchFail "Build finished but no NSIS installer found under $bundleDir"
}

$outDir = Join-Path $script:VideoFetchRoot ".build"
New-Item -ItemType Directory -Force -Path $outDir | Out-Null
Get-ChildItem -Path $outDir -Filter "*setup.exe" -File -ErrorAction SilentlyContinue | Remove-Item -Force

$copied = @()
foreach ($file in $installers) {
    $dest = Join-Path $outDir $file.Name
    Copy-Item $file.FullName $dest -Force
    $copied += Get-Item $dest
}

Write-Host ""
Write-Host "Installer ready in .build/:"
$copied | Format-Table FullName, Length -AutoSize
Write-Host "Run: $($copied[-1].FullName)"
