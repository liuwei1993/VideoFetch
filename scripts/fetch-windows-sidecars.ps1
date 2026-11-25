# Download standalone yt-dlp + ffmpeg/ffprobe for Windows x64.
# Writes <name>.exe and <name>-<target-triple>.exe under src-tauri/binaries/ (gitignored).
$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
$binDir = Join-Path $root "src-tauri\binaries"
New-Item -ItemType Directory -Force -Path $binDir | Out-Null

if (-not $env:https_proxy -and -not $env:HTTPS_PROXY -and -not $env:http_proxy -and -not $env:HTTP_PROXY) {
    & curl.exe -fsS --max-time 3 -x http://127.0.0.1:57890 https://github.com -o NUL
    if ($LASTEXITCODE -eq 0) {
        $env:https_proxy = "http://127.0.0.1:57890"
        $env:http_proxy = "http://127.0.0.1:57890"
        Write-Host "Using local HTTP proxy $($env:https_proxy)"
    }
}

$triple = $env:VIDEOFETCH_HOST_TRIPLE
if (-not $triple) {
    $rustc = Get-Command rustc -ErrorAction SilentlyContinue
    if ($rustc) {
        $hostLine = & rustc -vV | Select-String -Pattern "^host: "
        if ($hostLine) {
            $triple = $hostLine.ToString().Substring(6).Trim()
        }
    }
}
if (-not $triple) {
    $triple = "x86_64-pc-windows-msvc"
}

if ($triple -notmatch "^x86_64-pc-windows-") {
    Write-Error "Unsupported Windows target: $triple (x86_64-pc-windows-* only)"
}

$yt = Join-Path $binDir "yt-dlp-$triple.exe"
$ff = Join-Path $binDir "ffmpeg-$triple.exe"
$fp = Join-Path $binDir "ffprobe-$triple.exe"
if ((Test-Path $yt) -and (Test-Path $ff) -and (Test-Path $fp) -and (Test-Path (Join-Path $binDir "yt-dlp.exe")) -and (Test-Path (Join-Path $binDir "ffmpeg.exe")) -and (Test-Path (Join-Path $binDir "ffprobe.exe"))) {
    Write-Host "Sidecars already present for $triple"
    Get-ChildItem $binDir | Format-Table Name, Length
    exit 0
}

function Invoke-Curl([string]$Url, [string]$OutFile) {
    $curlArgs = @("-fL", "--retry", "3", "--retry-all-errors", "-o", $OutFile, $Url)
    $proxy = $env:https_proxy
    if (-not $proxy) { $proxy = $env:HTTPS_PROXY }
    if (-not $proxy) { $proxy = $env:http_proxy }
    if (-not $proxy) { $proxy = $env:HTTP_PROXY }
    if ($proxy) {
        $curlArgs = @("-x", $proxy) + $curlArgs
    }
    & curl.exe @curlArgs
    if ($LASTEXITCODE -ne 0) {
        throw "download failed: $Url"
    }
}

$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("videofetch-sidecars-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Force -Path $tmp | Out-Null
try {
    if (-not (Test-Path $yt) -or -not (Test-Path (Join-Path $binDir "yt-dlp.exe"))) {
        Write-Host "Downloading yt-dlp for $triple..."
        $ytTmp = Join-Path $tmp "yt-dlp.exe"
        Invoke-Curl "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe" $ytTmp
        Copy-Item $ytTmp $yt -Force
        Copy-Item $ytTmp (Join-Path $binDir "yt-dlp.exe") -Force
    }
    if (-not (Test-Path $ff) -or -not (Test-Path $fp)) {
        Write-Host "Downloading ffmpeg/ffprobe for $triple..."
        $zip = Join-Path $tmp "ffmpeg.zip"
        Invoke-Curl "https://github.com/yt-dlp/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-gpl.zip" $zip
        $extract = Join-Path $tmp "ff"
        Expand-Archive -Path $zip -DestinationPath $extract -Force
        $ffmpeg = Get-ChildItem -Path $extract -Recurse -Filter ffmpeg.exe | Select-Object -First 1
        $ffprobe = Get-ChildItem -Path $extract -Recurse -Filter ffprobe.exe | Select-Object -First 1
        if (-not $ffmpeg -or -not $ffprobe) {
            throw "ffmpeg archive did not contain ffmpeg.exe and ffprobe.exe"
        }
        Copy-Item $ffmpeg.FullName $ff -Force
        Copy-Item $ffprobe.FullName $fp -Force
        Copy-Item $ffmpeg.FullName (Join-Path $binDir "ffmpeg.exe") -Force
        Copy-Item $ffprobe.FullName (Join-Path $binDir "ffprobe.exe") -Force
    }
}
finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}

Write-Host "Sidecars ready:"
Get-ChildItem $binDir | Format-Table Name, Length
