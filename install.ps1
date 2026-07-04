#!/usr/bin/env pwsh
# rsbox Windows installer script
# Usage: iwr -useb https://raw.githubusercontent.com/luuuunet/rsbox/main/install.ps1 | iex

$ErrorActionPreference = "Stop"

# Configuration
$REPO = "luuuunet/rsbox"
$BINARY_NAME = "rsbox.exe"
$INSTALL_DIR = "$env:LOCALAPPDATA\rsbox"
$MIN_RSQ_VERSION = "0.1.5"

function Write-ColorOutput($ForegroundColor) {
    $fc = $host.UI.RawUI.ForegroundColor
    $host.UI.RawUI.ForegroundColor = $ForegroundColor
    if ($args) {
        Write-Output $args
    }
    $host.UI.RawUI.ForegroundColor = $fc
}

function Get-LatestVersion {
    Write-ColorOutput Yellow "Fetching latest version..."

    try {
        $response = Invoke-RestMethod -Uri "https://api.github.com/repos/$REPO/releases/latest"
        $version = $response.tag_name
        Write-ColorOutput Green "Latest version: $version"
        return $version
    }
    catch {
        $fallback = "v$MIN_RSQ_VERSION"
        Write-ColorOutput Yellow "Could not fetch latest release; falling back to $fallback"
        return $fallback
    }
}

function Get-Architecture {
    $arch = $env:PROCESSOR_ARCHITECTURE

    switch ($arch) {
        "AMD64" { return "x86_64" }
        "ARM64" { return "aarch64" }
        default {
            Write-ColorOutput Red "Unsupported architecture: $arch"
            exit 1
        }
    }
}

function Test-RsqSupport($binaryPath) {
    $probe = '{"inbounds":[{"type":"rsq","tag":"probe","listen":"127.0.0.1","listen_port":65503}]}'
    $probeFile = Join-Path $env:TEMP "rsbox-rsq-probe.json"
    Set-Content -Path $probeFile -Value $probe -Encoding utf8NoBOM
    try {
        & $binaryPath check -c $probeFile 2>$null
        return $LASTEXITCODE -eq 0
    }
    catch {
        return $false
    }
    finally {
        Remove-Item -Path $probeFile -ErrorAction SilentlyContinue
    }
}

function Download-Binary($version, $arch) {
    $downloadUrl = "https://github.com/$REPO/releases/download/$version/rsbox-windows-$arch.exe"
    $tempFile = "$env:TEMP\$BINARY_NAME"

    Write-ColorOutput Yellow "Downloading from: $downloadUrl"

    try {
        Invoke-WebRequest -Uri $downloadUrl -OutFile $tempFile -UseBasicParsing
    }
    catch {
        Write-ColorOutput Red "Download failed: $_"
        Write-ColorOutput Red "RSQ support requires rsbox >= $MIN_RSQ_VERSION"
        exit 1
    }

    if (-not (Test-RsqSupport $tempFile)) {
        Write-ColorOutput Red "Downloaded binary does not support RSQ inbound (need >= $MIN_RSQ_VERSION)."
        exit 1
    }

    Write-ColorOutput Green "✓ Downloaded successfully"
    return $tempFile
}

function Install-Binary($tempFile) {
    Write-ColorOutput Yellow "Installing to $INSTALL_DIR..."

    if (-not (Test-Path $INSTALL_DIR)) {
        New-Item -ItemType Directory -Path $INSTALL_DIR -Force | Out-Null
    }

    $targetPath = "$INSTALL_DIR\$BINARY_NAME"
    Move-Item -Path $tempFile -Destination $targetPath -Force

    Write-ColorOutput Green "✓ Installed successfully"
}

function Add-ToPath {
    $currentPath = [Environment]::GetEnvironmentVariable("Path", "User")

    if ($currentPath -notlike "*$INSTALL_DIR*") {
        Write-ColorOutput Yellow "Adding to PATH..."

        $newPath = "$currentPath;$INSTALL_DIR"
        [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
        $env:Path = "$env:Path;$INSTALL_DIR"

        Write-ColorOutput Green "✓ Added to PATH"
        Write-ColorOutput Yellow "Note: Restart your terminal to use rsbox command globally"
    }
}

function Test-Installation {
    $binaryPath = "$INSTALL_DIR\$BINARY_NAME"

    if (Test-Path $binaryPath) {
        Write-ColorOutput Green "Installation verified!"
        Write-Output ""

        try {
            & $binaryPath version
        }
        catch {
            Write-ColorOutput Yellow "Binary installed but version check failed"
        }
    }
    else {
        Write-ColorOutput Red "Installation verification failed"
        exit 1
    }
}

function Show-NextSteps {
    Write-Output ""
    Write-Output "Next steps:"
    Write-Output "  1. Generate QUIC TLS certs: rsbox rsq-gen-cert --output-dir .\certs --name your.domain"
    Write-Output "  2. Create a config file: config.json (inbound type: rsq)"
    Write-Output "  3. Run: rsbox run -c config.json"
    Write-Output ""
    Write-Output "For more information, visit:"
    Write-Output "  https://github.com/$REPO"
    Write-Output ""
}

try {
    Write-ColorOutput Green "rsbox Installation Script (Windows)"
    Write-Output ""

    $version = Get-LatestVersion
    $arch = Get-Architecture

    Write-ColorOutput Green "Detected architecture: $arch"

    $tempFile = Download-Binary $version $arch
    Install-Binary $tempFile
    Add-ToPath
    Test-Installation
    Show-NextSteps

    Write-ColorOutput Green "Installation complete! 🎉"
}
catch {
    Write-ColorOutput Red "Installation failed: $_"
    exit 1
}
