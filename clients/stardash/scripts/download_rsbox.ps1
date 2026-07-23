$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
$BinDir = Join-Path $Root "g5_client\binaries\windows"
$AssetDir = Join-Path $Root "g5_client\assets\binaries\windows"
New-Item -ItemType Directory -Path $BinDir, $AssetDir -Force | Out-Null
$RsboxVer = if ($env:RSBOX_VER) { $env:RSBOX_VER } else { "0.1.15" }
$Url = "https://github.com/luuuunet/rsbox/releases/download/v$RsboxVer/rsbox-windows-x86_64.exe"
$Out = Join-Path $AssetDir "rsbox.exe"
Write-Host "Downloading rsbox v$RsboxVer ..."
curl.exe -fL --retry 5 -o $Out $Url
Copy-Item $Out (Join-Path $BinDir "rsbox.exe") -Force
Write-Host "Done: $Out ($([math]::Round((Get-Item $Out).Length/1MB,2)) MB)"
