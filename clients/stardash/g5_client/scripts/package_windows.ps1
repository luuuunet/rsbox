# 打包 Windows 发布版 ZIP 安装包
$ErrorActionPreference = 'Stop'

$ProjectRoot = Resolve-Path (Join-Path $PSScriptRoot '..')
$ReleaseSrc = Join-Path $ProjectRoot 'build\windows\x64\runner\Release'
$Version = '0.1.0'
$DistRoot = Join-Path $ProjectRoot 'dist'
$StageName = "G5Client-$Version-windows-x64"
$StageDir = Join-Path $DistRoot $StageName
$ZipPath = Join-Path $DistRoot "$StageName.zip"

if (-not (Test-Path (Join-Path $ReleaseSrc 'g5_client.exe'))) {
    Write-Host '未找到 Release 构建，请先执行: flutter build windows --release' -ForegroundColor Red
    exit 1
}

Write-Host "打包 $StageName ..."

if (Test-Path $StageDir) { Remove-Item $StageDir -Recurse -Force }
New-Item -ItemType Directory -Force -Path $StageDir | Out-Null

Copy-Item -Path (Join-Path $ReleaseSrc '*') -Destination $StageDir -Recurse -Force
Copy-Item -Path (Join-Path $ProjectRoot 'installer\install.ps1') -Destination $StageDir -Force
Copy-Item -Path (Join-Path $ProjectRoot 'installer\uninstall.ps1') -Destination $StageDir -Force
Copy-Item -Path (Join-Path $ProjectRoot 'installer\安装.bat') -Destination $StageDir -Force
Copy-Item -Path (Join-Path $ProjectRoot 'installer\卸载.bat') -Destination $StageDir -Force
Copy-Item -Path (Join-Path $ProjectRoot 'installer\README.txt') -Destination $StageDir -Force

if (Test-Path $ZipPath) { Remove-Item $ZipPath -Force }
Compress-Archive -Path $StageDir -DestinationPath $ZipPath -CompressionLevel Optimal

$sizeMb = [math]::Round((Get-Item $ZipPath).Length / 1MB, 2)
Write-Host ''
Write-Host "完成: $ZipPath ($sizeMb MB)" -ForegroundColor Green
Write-Host '解压后运行「安装.bat」即可测试安装。'
