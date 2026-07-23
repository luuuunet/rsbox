# G5 Client 本地安装（无需管理员；TUN 模式请右键 exe 以管理员运行）
$ErrorActionPreference = 'Stop'

$SourceDir = $PSScriptRoot
$InstallDir = Join-Path $env:LOCALAPPDATA 'Programs\G5Client'
$ExePath = Join-Path $InstallDir 'g5_client.exe'

Write-Host '正在安装 G5 VPN 客户端...' -ForegroundColor Cyan
Write-Host "目标目录: $InstallDir"

if (Get-Process g5_client -ErrorAction SilentlyContinue) {
    Write-Host '正在关闭运行中的客户端...'
    Stop-Process -Name g5_client -Force -ErrorAction SilentlyContinue
    Start-Sleep -Seconds 1
}

New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
Copy-Item -Path (Join-Path $SourceDir '*') -Destination $InstallDir -Recurse -Force

$desktop = [Environment]::GetFolderPath('Desktop')
$shortcutPath = Join-Path $desktop 'G5 VPN.lnk'
$wsh = New-Object -ComObject WScript.Shell
$shortcut = $wsh.CreateShortcut($shortcutPath)
$shortcut.TargetPath = $ExePath
$shortcut.WorkingDirectory = $InstallDir
$shortcut.Description = 'G5 VPN Client'
$shortcut.Save()

Write-Host ''
Write-Host '安装完成！' -ForegroundColor Green
Write-Host "程序目录: $InstallDir"
Write-Host "桌面快捷方式: G5 VPN"
Write-Host ''
Write-Host '提示: TUN 全局模式需要以管理员身份运行客户端。'
