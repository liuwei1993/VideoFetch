# Build a portable Windows folder and zip for VideoFetch.
# yt-dlp, ffmpeg, ffprobe, and the MissAV plugin sit next to VideoFetch.exe.
# WebView2, already present on current Windows 10/11, is still required.
# Usage: powershell -NoProfile -ExecutionPolicy Bypass -File scripts\build-portable.ps1
# Output: .build\VideoFetch-windows-x64\ and .build\VideoFetch-windows-x64.zip
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

Write-Host "Building portable app for $triple..."
npm run tauri -- build --bundles none
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}

$exe = Join-Path $script:VideoFetchRoot "src-tauri\target\release\tauri-app.exe"
if (-not (Test-Path $exe)) {
    Write-VideoFetchFail "Build finished but no executable found at $exe"
}

$binDir = Join-Path $script:VideoFetchRoot "src-tauri\binaries"
$sidecars = @("yt-dlp.exe", "ffmpeg.exe", "ffprobe.exe")
foreach ($name in $sidecars) {
    if (-not (Test-Path (Join-Path $binDir $name))) {
        Write-VideoFetchFail "Missing sidecar $name. Run scripts\fetch-windows-sidecars.ps1 first."
    }
}

$outDir = Join-Path $script:VideoFetchRoot ".build"
$portableDir = Join-Path $outDir "VideoFetch-windows-x64"
$zipPath = Join-Path $outDir "VideoFetch-windows-x64.zip"
New-Item -ItemType Directory -Force -Path $outDir | Out-Null
if (Test-Path $portableDir) {
    Remove-Item -Recurse -Force $portableDir
}
if (Test-Path $zipPath) {
    Remove-Item -Force $zipPath
}
New-Item -ItemType Directory -Force -Path $portableDir | Out-Null

Copy-Item $exe (Join-Path $portableDir "VideoFetch.exe") -Force
foreach ($name in $sidecars) {
    Copy-Item (Join-Path $binDir $name) (Join-Path $portableDir $name) -Force
}
Copy-Item (Join-Path $script:VideoFetchRoot "src-tauri\yt-dlp-plugins") (Join-Path $portableDir "yt-dlp-plugins") -Recurse -Force

Compress-Archive -Path $portableDir -DestinationPath $zipPath -CompressionLevel Optimal

Write-Host ""
Write-Host "Portable app ready:"
Get-Item (Join-Path $portableDir "VideoFetch.exe"), $zipPath | Format-Table FullName, Length -AutoSize
Write-Host "Run: $(Join-Path $portableDir 'VideoFetch.exe')"
