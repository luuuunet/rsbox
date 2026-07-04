param(
    [string]$Rsbox = "",
    [string]$TestUrl = "https://www.cloudflare.com/cdn-cgi/trace",
    [switch]$Quiet
)

$ErrorActionPreference = "Stop"
$Root = (Resolve-Path "$PSScriptRoot\..").Path
. (Join-Path $PSScriptRoot "rsq-test-common.ps1")

if (-not $Rsbox) {
    foreach ($c in @(
        "$Root\target\release\rsbox.exe",
        "$Root\target\debug\rsbox.exe",
        "$env:APPDATA\com.example\g5_client\bin\rsbox.exe"
    )) {
        if (Test-Path $c) { $Rsbox = (Resolve-Path $c).Path; break }
    }
}
if (-not $Rsbox -or -not (Test-Path $Rsbox)) {
    Write-Error "rsbox not found. Build with: cargo build --release -p rsbox"
}

function Test-PortListening {
    param([int]$Port, [ValidateSet("Tcp", "Udp")]$Protocol = "Tcp")
    if ($Protocol -eq "Udp") {
        return [bool](Get-NetUDPEndpoint -LocalPort $Port -ErrorAction SilentlyContinue)
    }
    return [bool](Get-NetTCPConnection -LocalPort $Port -State Listen -ErrorAction SilentlyContinue)
}

function Ensure-RsqCerts {
    $certDir = Join-Path $Root "examples\certs\rsq-local"
    if (-not (Test-Path (Join-Path $certDir "fullchain.pem"))) {
        if (-not $Quiet) { Write-Host "Generating RSQ dev certs..." }
        Push-Location $Root
        & $Rsbox rsq-gen-cert 2>&1 | Out-Null
        Pop-Location
    }
}

if (-not $Quiet) {
    Write-Host "=== RSQ subscription local E2E ==="
    Write-Host "rsbox: $Rsbox"
}

Ensure-RsqCerts
Stop-RsqOwned

$serverCfg = Join-Path $Root "examples\rsq-local-server-multi.json"
$clientCfg = Join-Path $Root "examples\rsq-subscription-local.json"
$listenPort = 17894

Push-Location $Root
try {
    $checkOut = & $Rsbox check -c $clientCfg 2>&1 | Out-String
    if ($LASTEXITCODE -ne 0) {
        Write-Error "subscription client config invalid:`n$checkOut"
    }
    if ($checkOut -notmatch "Subscription: loaded") {
        Write-Error "subscription merge failed during check"
    }
} finally {
    Pop-Location
}

$serverLog = Join-Path $env:TEMP "rsq-sub-server.log"
$serverErrLog = Join-Path $env:TEMP "rsq-sub-server.err.log"
$serverProc = Start-Process -FilePath $Rsbox -ArgumentList @("run", "-c", $serverCfg) -PassThru -WindowStyle Hidden `
    -RedirectStandardError $serverErrLog -RedirectStandardOutput $serverLog -WorkingDirectory $Root
Add-RsqOwnedPid $serverProc.Id

$serverReady = $false
for ($i = 0; $i -lt 30; $i++) {
    Start-Sleep -Milliseconds 500
    if ($serverProc.HasExited) { break }
    if (Test-PortListening -Port 18443 -Protocol Udp) {
        $serverReady = $true
        Start-Sleep -Milliseconds 500
        break
    }
}
if (-not $serverReady) {
    Stop-RsqOwned
    Get-Content $serverLog -Tail 10 -ErrorAction SilentlyContinue
    Get-Content $serverErrLog -Tail 10 -ErrorAction SilentlyContinue
    Write-Error "RSQ server failed to listen on 18443"
}

$clientLog = Join-Path $env:TEMP "rsq-sub-client.log"
$clientErrLog = Join-Path $env:TEMP "rsq-sub-client.err.log"
$clientProc = Start-Process -FilePath $Rsbox -ArgumentList @("run", "-c", $clientCfg) -PassThru -WindowStyle Hidden `
    -RedirectStandardError $clientErrLog -RedirectStandardOutput $clientLog -WorkingDirectory $Root
Add-RsqOwnedPid $clientProc.Id

$clientReady = $false
for ($i = 0; $i -lt 40; $i++) {
    Start-Sleep -Milliseconds 500
    if ($clientProc.HasExited) { break }
    if (Test-PortListening -Port $listenPort -Protocol Tcp) {
        $clientReady = $true
        Start-Sleep -Milliseconds 1000
        break
    }
}
if (-not $clientReady) {
    Stop-RsqOwned
    Get-Content $clientLog -Tail 12 -ErrorAction SilentlyContinue
    Get-Content $clientErrLog -Tail 12 -ErrorAction SilentlyContinue
    Write-Error "subscription client failed to listen on $listenPort"
}

$prevEap = $ErrorActionPreference
$ErrorActionPreference = "Continue"
$raw = (curl.exe -x "http://127.0.0.1:$listenPort" -sS -o NUL -w "%{http_code}" --connect-timeout 25 --max-time 45 $TestUrl 2>$null) | Out-String
$ErrorActionPreference = $prevEap
$code = $raw.Trim()
if ($code -notmatch '^\d{3}$') { $code = "" }

Stop-RsqOwned

$pass = $code -match "^(200|204|301|302)$"
if (-not $Quiet) {
    Write-Host ""
    [pscustomobject]@{
        Case = "subscription-selector"
        Http = if ($code) { $code } else { "FAIL" }
        Pass = $pass
    } | Format-Table -AutoSize
    if ($pass) {
        Write-Host "=== RSQ subscription local: 1 / 1 passed ==="
    } else {
        Write-Host "=== RSQ subscription local: 0 / 1 passed ==="
    }
}

if (-not $pass) { exit 1 }
exit 0
