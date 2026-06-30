param(
    [string]$Rsbox = "",
    [string]$Config = "$PSScriptRoot\..\examples\generated\g5-panel-test.json",
    [int]$V2RayPort = 10085,
    [int]$SsPort = 18443,
    [string]$UserName = "g5user@test.com"
)

$ErrorActionPreference = "Stop"

if (-not $Rsbox) {
    foreach ($c in @(
        "$PSScriptRoot\..\target\release\rsbox.exe",
        "$PSScriptRoot\..\target\debug\rsbox.exe",
        "$env:APPDATA\com.example\g5_client\bin\rsbox.exe"
    )) {
        if (Test-Path $c) { $Rsbox = (Resolve-Path $c).Path; break }
    }
}

$root = (Resolve-Path "$PSScriptRoot\..").Path
if (-not $Rsbox -or -not (Test-Path $Rsbox)) {
    Write-Host "Building rsbox..."
    Push-Location $root
    cargo build --release -p rsbox
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    Pop-Location
    $Rsbox = Join-Path $root "target\release\rsbox.exe"
}

$Config = (Resolve-Path $Config).Path
Write-Host "=== G5 panel live smoke test ==="
Write-Host "rsbox:  $Rsbox"
Write-Host "config: $Config"

& $Rsbox check -c $Config
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Get-Process rsbox -ErrorAction SilentlyContinue | ForEach-Object {
    Write-Host "Stopping existing rsbox PID $($_.Id)"
    Stop-Process -Id $_.Id -Force
}
Start-Sleep -Seconds 1

$logDir = Join-Path $env:TEMP "rsbox-g5-test"
New-Item -ItemType Directory -Path $logDir -Force | Out-Null
$logFile = Join-Path $logDir "g5-$(Get-Date -Format 'yyyyMMdd-HHmmss').log"

$proc = Start-Process -FilePath $Rsbox -ArgumentList @("run", "-c", $Config) -PassThru -WindowStyle Hidden `
    -RedirectStandardOutput $logFile -RedirectStandardError (Join-Path $logDir "g5-err.log")
Start-Sleep -Seconds 3

if ($proc.HasExited) {
    Write-Host "FAIL: rsbox exited early"
    Get-Content $logFile -Tail 50
    exit 1
}

$v2rayListen = Get-NetTCPConnection -LocalPort $V2RayPort -State Listen -ErrorAction SilentlyContinue
$ssListen = Get-NetTCPConnection -LocalPort $SsPort -State Listen -ErrorAction SilentlyContinue
if (-not $v2rayListen) {
    Write-Host "FAIL: v2ray_api port $V2RayPort not listening"
    Get-Content $logFile -Tail 30
    Stop-Process -Id $proc.Id -Force
    exit 1
}
if (-not $ssListen) {
    Write-Host "FAIL: shadowsocks port $SsPort not listening"
    Get-Content $logFile -Tail 30
    Stop-Process -Id $proc.Id -Force
    exit 1
}
Write-Host "OK: v2ray_api :$V2RayPort and shadowsocks :$SsPort listening"

$statsUrl = "http://127.0.0.1:$V2RayPort/stats?pattern=user>>>$UserName&reset=false"
Write-Host "GET $statsUrl"
$before = Invoke-RestMethod -Uri $statsUrl -Method Get
$beforeUp = ($before.stat | Where-Object { $_.name -like "*uplink*" } | Select-Object -First 1).value
$beforeDown = ($before.stat | Where-Object { $_.name -like "*downlink*" } | Select-Object -First 1).value
Write-Host "Before traffic: uplink=$beforeUp downlink=$beforeDown"

Write-Host ""
Write-Host "Stopping rsbox before isolated integration test (avoid port conflict)..."
Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 2

Write-Host "Running Rust integration test (SS client + stats assert)..."
Push-Location $root
cargo test -p rsb-protocol --test g5_panel g5_v2ray_stats_after_shadowsocks_traffic -- --nocapture
$testExit = $LASTEXITCODE
Pop-Location

if ($testExit -ne 0) {
    Write-Host "FAIL: integration test failed (exit $testExit)"
    exit $testExit
}

Write-Host ""
Write-Host "=== G5 panel test PASSED ==="
exit 0
