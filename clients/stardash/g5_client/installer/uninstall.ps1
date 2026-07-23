# 卸载 G5 Client（删除安装目录与桌面快捷方式）
$ErrorActionPreference = 'Stop'

$InstallDir = Join-Path $env:LOCALAPPDATA 'Programs\G5Client'
$desktop = [Environment]::GetFolderPath('Desktop')
$shortcutPath = Join-Path $desktop 'G5 VPN.lnk'

if (Get-Process g5_client -ErrorAction SilentlyContinue) {
    Stop-Process -Name g5_client -Force -ErrorAction SilentlyContinue
    Start-Sleep -Seconds 1
}

if (Test-Path $shortcutPath) { Remove-Item $shortcutPath -Force }
if (Test-Path $InstallDir) { Remove-Item $InstallDir -Recurse -Force }

Write-Host '已卸载 G5 VPN 客户端。' -ForegroundColor Green
