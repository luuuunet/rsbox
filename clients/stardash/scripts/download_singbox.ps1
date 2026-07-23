$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
$BinDir = Join-Path $Root "g5_client\binaries\windows"
$AssetDir = Join-Path $Root "g5_client\assets\binaries\windows"
New-Item -ItemType Directory -Path $BinDir, $AssetDir -Force | Out-Null

$sbVer = "1.12.12"
$sbZip = Join-Path $env:TEMP "sing-box-$sbVer-windows-amd64.zip"
$sbUrl = "https://github.com/SagerNet/sing-box/releases/download/v$sbVer/sing-box-$sbVer-windows-amd64.zip"
Write-Host "Downloading sing-box $sbVer..."
curl.exe -fL --retry 5 -o $sbZip $sbUrl
Expand-Archive -Path $sbZip -DestinationPath "$env:TEMP\sing-box-extract" -Force
$exe = Get-ChildItem "$env:TEMP\sing-box-extract" -Recurse -Filter "sing-box.exe" | Select-Object -First 1
Copy-Item $exe.FullName (Join-Path $BinDir "sing-box.exe") -Force

$wintunZip = Join-Path $env:TEMP "wintun.zip"
curl.exe -fL --retry 5 -o $wintunZip "https://www.wintun.net/builds/wintun-0.14.1.zip"
Expand-Archive -Path $wintunZip -DestinationPath "$env:TEMP\wintun-extract" -Force
$wintun = Get-ChildItem "$env:TEMP\wintun-extract" -Recurse -Filter "wintun.dll" | Where-Object { $_.FullName -match "amd64" } | Select-Object -First 1
Copy-Item $wintun.FullName (Join-Path $BinDir "wintun.dll") -Force

Copy-Item "$BinDir\*" $AssetDir -Force
# Keep existing rsbox.exe if present
Write-Host "Done: $AssetDir"
Get-ChildItem $AssetDir
