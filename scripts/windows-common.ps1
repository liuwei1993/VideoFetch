# Shared setup for the Windows VideoFetch scripts.
# Dot-source only. Mirrors the proxy sniff and toolchain checks in scripts/run-dev.sh.
$ErrorActionPreference = "Stop"

$script:VideoFetchRoot = Split-Path -Parent $PSScriptRoot

function Write-VideoFetchFail([string]$Message) {
    [Console]::Error.WriteLine($Message)
    exit 1
}

function Add-VideoFetchPathPrefix([string]$Dir) {
    if (-not (Test-Path $Dir)) {
        return
    }
    $prefix = $Dir.TrimEnd('\')
    $pattern = [regex]::Escape($prefix) + '(?:\\|;)'
    if ($env:Path -notmatch $pattern) {
        $env:Path = "$prefix;$env:Path"
    }
}

function Import-VideoFetchToolchain {
    Add-VideoFetchPathPrefix (Join-Path $env:USERPROFILE ".cargo\bin")
    Add-VideoFetchPathPrefix (Join-Path $env:ProgramFiles "nodejs")
    Add-VideoFetchPathPrefix (Join-Path $env:LOCALAPPDATA "Programs\nodejs")

    if (-not (Get-Command rustc -ErrorAction SilentlyContinue)) {
        Write-VideoFetchFail "rustc not found; install Rust (https://rustup.rs/) first"
    }
    if (-not (Get-Command node -ErrorAction SilentlyContinue)) {
        Write-VideoFetchFail "node not found; install Node.js 18+ first"
    }
    if (-not (Get-Command npm -ErrorAction SilentlyContinue)) {
        Write-VideoFetchFail "npm not found; install Node.js 18+ first"
    }

    $vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
    if (-not (Test-Path $vswhere)) {
        Write-VideoFetchFail "Visual Studio Build Tools not found (vswhere.exe missing). Install the C++ workload."
    }
    $installPath = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if (-not $installPath) {
        Write-VideoFetchFail "MSVC C++ tools not found. Install Visual Studio Build Tools with the Desktop development with C++ workload."
    }
    $bat = Join-Path $installPath "VC\Auxiliary\Build\vcvars64.bat"
    if (-not (Test-Path $bat)) {
        Write-VideoFetchFail "vcvars64.bat not found at $bat"
    }
    Write-Host "Using MSVC at $installPath"
    $vcvars = cmd /c "call `"$bat`" >nul && set"
    if ($LASTEXITCODE -ne 0) {
        Write-VideoFetchFail "vcvars64.bat failed with exit code $LASTEXITCODE"
    }
    $vcvars | ForEach-Object {
        if ($_ -match '^(.*?)=(.*)$') {
            Set-Item -Path "Env:$($matches[1])" -Value $matches[2]
        }
    }
}

function Use-VideoFetchProxy {
    if ($env:https_proxy -or $env:HTTPS_PROXY -or $env:http_proxy -or $env:HTTP_PROXY) {
        return
    }
    & curl.exe -fsS --max-time 3 -x http://127.0.0.1:57890 https://github.com -o NUL
    if ($LASTEXITCODE -eq 0) {
        $env:https_proxy = "http://127.0.0.1:57890"
        $env:http_proxy = "http://127.0.0.1:57890"
        $env:HTTPS_PROXY = $env:https_proxy
        $env:HTTP_PROXY = $env:http_proxy
        Write-Host "Using local HTTP proxy $($env:https_proxy)"
    }
}

function Install-VideoFetchNpmDeps {
    if (-not (Test-Path (Join-Path $script:VideoFetchRoot "node_modules"))) {
        Write-Host "Installing npm dependencies..."
        npm install
        if ($LASTEXITCODE -ne 0) {
            exit $LASTEXITCODE
        }
    }
}

function Invoke-VideoFetchSidecars {
    Write-Host "Fetching Windows sidecars (yt-dlp / ffmpeg / ffprobe)..."
    & "$PSScriptRoot\fetch-windows-sidecars.ps1"
    if ($LASTEXITCODE -ne 0) {
        exit $LASTEXITCODE
    }
}
