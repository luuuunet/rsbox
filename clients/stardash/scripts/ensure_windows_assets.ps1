# Ensure Windows sing-box asset paths exist so `flutter build` succeeds on all CI runners.
$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$AssetDir = Join-Path $Root "g5_client\assets\binaries\windows"
New-Item -ItemType Directory -Path $AssetDir -Force | Out-Null

$singBox = Join-Path $AssetDir "sing-box.exe"
$wintun = Join-Path $AssetDir "wintun.dll"
$rsbox = Join-Path $AssetDir "rsbox.exe"

$DevBin = Join-Path $Root "g5_client\binaries\windows"
$DevSingBox = Join-Path $DevBin "sing-box.exe"
$DevWintun = Join-Path $DevBin "wintun.dll"

function Test-ValidBinary($Path, $MinBytes) {
    return (Test-Path $Path) -and ((Get-Item $Path).Length -ge $MinBytes)
}

$assetsOk = (Test-ValidBinary $singBox 1MB) -and (Test-ValidBinary $wintun 32KB)
if ($assetsOk) {
    if (-not (Test-ValidBinary $rsbox 1MB)) {
        $downloadRsbox = Join-Path $Root "scripts\download_rsbox.ps1"
        if (Test-Path $downloadRsbox) {
            try { & $downloadRsbox } catch {
                Write-Host "rsbox missing — placeholder. $_" -ForegroundColor Yellow
                [IO.File]::WriteAllBytes($rsbox, @(0))
            }
        } else {
            [IO.File]::WriteAllBytes($rsbox, @(0))
        }
    }
    Write-Host "Windows VPN binaries already present." -ForegroundColor Green
    exit 0
}

if ((Test-ValidBinary $DevSingBox 1MB) -and (Test-ValidBinary $DevWintun 32KB)) {
    Write-Host "Copying Windows VPN binaries from g5_client/binaries/windows..." -ForegroundColor Cyan
    Copy-Item "$DevBin\*" $AssetDir -Force
    if (-not (Test-ValidBinary $rsbox 1MB)) {
        [IO.File]::WriteAllBytes($rsbox, @(0))
    }
    exit 0
}

Write-Host "Windows VPN binaries missing — creating CI placeholders." -ForegroundColor Yellow
Write-Host "For local VPN: run scripts\download_singbox.ps1" -ForegroundColor Yellow
foreach ($path in @($singBox, $wintun, $rsbox)) {
    if (-not (Test-Path $path)) {
        [IO.File]::WriteAllBytes($path, @(0))
    }
}
