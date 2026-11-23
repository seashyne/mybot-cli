# Install mybot cli on Windows (x86_64).
#
# Usage:
#   irm https://raw.githubusercontent.com/seashyne/mybot-cli/main/install.ps1 | iex
#   & ([scriptblock]::Create((irm https://raw.githubusercontent.com/seashyne/mybot-cli/main/install.ps1))) -Version v0.5.0
param(
    [string]$Version = "latest",
    [string]$Prefix = (Join-Path $env:LOCALAPPDATA "Programs\mybot\bin"),
    [switch]$Uninstall
)

$ErrorActionPreference = "Stop"
$Repo = "seashyne/mybot-cli"
$Target = "x86_64-pc-windows-msvc"
$AssetName = "mybot-$Target.exe"
$ExePath = Join-Path $Prefix "mybot.exe"

if ($Uninstall) {
    if (Test-Path $ExePath) {
        Remove-Item -Force $ExePath
        Write-Host "removed $ExePath"
    } else {
        Write-Host "not installed at $ExePath"
    }
    return
}

if ([System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture -ne "X64") {
    throw "No prebuilt Windows binary for $([System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture). Build from source with: cargo install --git https://github.com/$Repo"
}

$BaseUrl = if ($env:MYBOT_BASE_URL) {
    $env:MYBOT_BASE_URL.TrimEnd("/")
} elseif ($Version -eq "latest") {
    "https://github.com/$Repo/releases/latest/download"
} else {
    "https://github.com/$Repo/releases/download/$Version"
}

$TempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("mybot-install-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Force -Path $TempDir | Out-Null

try {
    $DownloadPath = Join-Path $TempDir $AssetName
    $SumsPath = Join-Path $TempDir "sha256sums.txt"

    Write-Host "downloading mybot for $Target ($Version)..."
    Invoke-WebRequest -Uri "$BaseUrl/$AssetName" -OutFile $DownloadPath

    try {
        Invoke-WebRequest -Uri "$BaseUrl/sha256sums.txt" -OutFile $SumsPath
        $Line = Get-Content $SumsPath | Where-Object { $_.Trim().EndsWith(" $AssetName") } | Select-Object -First 1
        if ($Line) {
            $Expected = ($Line.Trim() -split "\s+")[0].ToLowerInvariant()
            $Actual = (Get-FileHash -Path $DownloadPath -Algorithm SHA256).Hash.ToLowerInvariant()
            if ($Expected -ne $Actual) {
                throw "checksum mismatch for $AssetName (expected $Expected, got $Actual)"
            }
        }
    } catch {
        if ($_.Exception.Message -like "checksum mismatch*") {
            throw
        }
        Write-Warning "skipping checksum verification: $($_.Exception.Message)"
    }

    New-Item -ItemType Directory -Force -Path $Prefix | Out-Null
    Copy-Item -Force $DownloadPath $ExePath

    $UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $Entries = if ($UserPath) { $UserPath -split ";" | Where-Object { $_ -ne "" } } else { @() }
    if ($Entries -notcontains $Prefix) {
        $NewPath = (@($Entries) + $Prefix) -join ";"
        [Environment]::SetEnvironmentVariable("Path", $NewPath, "User")
        $env:Path = "$Prefix;$env:Path"
        Write-Host "added $Prefix to User PATH (restart new terminals to pick up)"
    }

    Write-Host "installed $ExePath"
    & $ExePath --version
} finally {
    Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $TempDir
}
