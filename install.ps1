#!/usr/bin/env pwsh
# rsbox Windows installer script
# Usage: iwr -useb https://raw.githubusercontent.com/yourusername/rsbox/main/install.ps1 | iex

$ErrorActionPreference = "Stop"

# Configuration
$REPO = "yourusername/rsbox"
$BINARY_NAME = "rsbox.exe"
$INSTALL_DIR = "$env:LOCALAPPDATA\rsbox"

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
        Write-ColorOutput Red "Failed to fetch latest version"
        exit 1
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

function Download-Binary($version, $arch) {
    $downloadUrl = "https://github.com/$REPO/releases/download/$version/rsbox-windows-$arch.exe"
    $tempFile = "$env:TEMP\$BINARY_NAME"

    Write-ColorOutput Yellow "Downloading from: $downloadUrl"

    try {
        Invoke-WebRequest -Uri $downloadUrl -OutFile $tempFile -UseBasicParsing
        Write-ColorOutput Green "✓ Downloaded successfully"
        return $tempFile
    }
    catch {
        Write-ColorOutput Red "Download failed: $_"
        exit 1
    }
}

function Install-Binary($tempFile) {
    Write-ColorOutput Yellow "Installing to $INSTALL_DIR..."

    # Create installation directory
    if (-not (Test-Path $INSTALL_DIR)) {
        New-Item -ItemType Directory -Path $INSTALL_DIR -Force | Out-Null
    }

    # Move binary
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

        # Update current session
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
    Write-Output "  1. Create a config file: config.json"
    Write-Output "  2. Run: rsbox run -c config.json"
    Write-Output ""
    Write-Output "For more information, visit:"
    Write-Output "  https://github.com/$REPO"
    Write-Output ""
}

# Main installation
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
