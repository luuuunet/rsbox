# Run WireGuard protocol test (requires elevation for TUN).
param(
    [string]$Rsbox = "$PSScriptRoot\..\target\release\rsbox.exe"
)

$ErrorActionPreference = "Stop"
$cfg = Join-Path $PSScriptRoot "..\examples\generated\protocol-tests\wireguard.json"
$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole(
    [Security.Principal.WindowsBuiltInRole]::Administrator
)
if (-not $isAdmin) {
    Write-Error "Not running as administrator. Re-launch this script from an elevated PowerShell."
    exit 1
}
if (-not (Test-Path $Rsbox)) { Write-Error "rsbox not found: $Rsbox"; exit 1 }

taskkill /F /IM rsbox.exe 2>$null | Out-Null
& $Rsbox check -c $cfg
if ($LASTEXITCODE -ne 0) { Write-Error "config check failed"; exit 1 }

$logErr = Join-Path $env:TEMP "rsbox-test-wireguard-err.log"
$p = Start-Process -FilePath $Rsbox -ArgumentList @("run", "-c", $cfg) -PassThru -WindowStyle Hidden `
    -RedirectStandardError $logErr
$iface = "wg-rsbox"
$serverIp = "10.66.66.1"
$ready = $false
for ($i = 0; $i -lt 25; $i++) {
    Start-Sleep -Seconds 1
    if ($p.HasExited) { break }
    if (Get-NetAdapter -Name $iface -ErrorAction SilentlyContinue) { $ready = $true; break }
}
if (-not $ready) {
    Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
    Write-Host "NO_TUN"
    Get-Content $logErr -Tail 10
    exit 1
}
$ping = ping.exe -n 3 -w 4000 $serverIp 2>&1 | Out-String
Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
if ($ping -match "Reply from|TTL=") {
    Write-Host "PING_OK"
    exit 0
}
Write-Host "FAIL(ping)"
Write-Host $ping
Get-Content $logErr -Tail 10
exit 1
