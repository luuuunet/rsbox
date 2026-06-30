param(
    [string]$EnvFile = "$PSScriptRoot\..\examples\test.env.example",
    [string]$OutDir = "$PSScriptRoot\..\examples\generated"
)

$ErrorActionPreference = "Stop"
if (-not (Test-Path $EnvFile)) { Write-Error "Missing env file: $EnvFile" }

$vars = @{}
Get-Content $EnvFile | ForEach-Object {
    $line = $_.Trim()
    if ($line -and -not $line.StartsWith("#") -and $line -match "^([^=]+)=(.*)$") {
        $vars[$Matches[1].Trim()] = $Matches[2].Trim()
    }
}

New-Item -ItemType Directory -Path $OutDir -Force | Out-Null

$templates = @(
    "config-shadowtls-ss.json",
    "config-shadowtls-ss-v2.json",
    "config-anytls.json",
    "config-test-shadowtls-ss-anytls.json"
)

function Apply-Template($name) {
    $src = Join-Path "$PSScriptRoot\..\examples" $name
    $text = Get-Content $src -Raw
    foreach ($key in $vars.Keys) {
        if ($key -match "_PORT$") { continue }
        $text = $text.Replace($key, $vars[$key])
    }
    if ($name -like "*anytls*" -and $name -notlike "*shadowtls*") {
        $text = $text -replace '"server_port":\s*443', "`"server_port`": $($vars['ANYTLS_PORT'])"
    }
    if ($name -like "*shadowtls*") {
        $text = $text -replace '("tag":\s*"shadowtls-out"[\s\S]*?"server_port":\s*)\d+', "`${1}$($vars['ST_PORT'])"
        $text = $text -replace '("type":\s*"shadowsocks"[\s\S]*?"server_port":\s*)\d+', "`${1}$($vars['SS_PORT'])"
    }
    if ($name -eq "config-test-shadowtls-ss-anytls.json") {
        $text = $text -replace '("tag":\s*"AnyTLS"[\s\S]*?"server_port":\s*)\d+', "`${1}$($vars['ANYTLS_PORT'])"
    }
    $dest = Join-Path $OutDir $name
    Set-Content -Path $dest -Value $text -Encoding UTF8
    Write-Host "Wrote $dest"
}

foreach ($name in $templates) { Apply-Template $name }
Write-Host "Done. Test with:"
Write-Host "  .\scripts\test-protocol-config.ps1 -Config examples\generated\config-shadowtls-ss.json"
