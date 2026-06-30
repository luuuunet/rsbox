param(
    [Parameter(Mandatory = $true)]
    [string]$ServerHost,
    [string]$Rsbox = "$PSScriptRoot\..\target\release\rsbox.exe",
    [string]$SourceConfigDir = "$PSScriptRoot\..\examples\generated\protocol-tests",
    [int]$Port = 17891,
    [string]$TestUrl = "https://1.1.1.1/cdn-cgi/trace"
)

$ErrorActionPreference = "Stop"
if (-not (Test-Path $Rsbox)) { Write-Error "rsbox not found: $Rsbox"; exit 1 }
if (-not (Test-Path $SourceConfigDir)) { Write-Error "config dir not found: $SourceConfigDir"; exit 1 }

$tmpDir = Join-Path $env:TEMP "rsbox-protocol-tests-$ServerHost"
if (Test-Path $tmpDir) { Remove-Item $tmpDir -Recurse -Force }
New-Item -ItemType Directory -Path $tmpDir | Out-Null

Get-ChildItem $SourceConfigDir -Filter "*.json" | ForEach-Object {
    $raw = Get-Content $_.FullName -Raw -Encoding UTF8
    $raw = $raw -replace 's\.lulunet\.cc', $ServerHost
    $raw = $raw -replace '157\.230\.3\.206', $ServerHost
    # big VPS test ports (nginx/hysteria occupy 443/3365)
    if ($ServerHost -eq '66.94.122.53') {
        $raw = $raw -replace '"server_port": 443', '"server_port": 4443'
        $raw = $raw -replace '"server_port": 3365', '"server_port": 3366'
    }
    $outPath = Join-Path $tmpDir $_.Name
    [System.IO.File]::WriteAllText($outPath, $raw, [System.Text.UTF8Encoding]::new($false))
}

Write-Host "=== protocol test -> $ServerHost ==="
Write-Host "configs: $tmpDir"
Write-Host ""

& "$PSScriptRoot\test-all-protocols.ps1" -Rsbox $Rsbox -ConfigDir $tmpDir -Port $Port -TestUrl $TestUrl
exit $LASTEXITCODE
