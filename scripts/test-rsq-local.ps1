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
function Stop-RsqClient {
    param([int]$ServerPid)
    Stop-RsqOwned -Except @($ServerPid)
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

function Get-ListenPortFromConfig {
    param([string]$Config)
    $json = Get-Content $Config -Raw | ConvertFrom-Json
    foreach ($inbound in $json.inbounds) {
        if ($null -ne $inbound.listen_port) {
            return [int]$inbound.listen_port
        }
    }
    return 17891
}

function Test-RsqClient {
    param([string]$Name, [string]$Config, [string]$Expect, [int]$ServerPid)
    Stop-RsqClient -ServerPid $ServerPid
    $listenPort = Get-ListenPortFromConfig $Config
    & $Rsbox check -c $Config 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) {
        return [pscustomobject]@{ Case = $Name; Http = "CHECK_FAIL"; Pass = $false }
    }
    $log = Join-Path $env:TEMP "rsq-test-$Name.log"
    $errLog = Join-Path $env:TEMP "rsq-test-$Name.err.log"
    $proc = Start-Process -FilePath $Rsbox -ArgumentList @("run", "-c", $Config) -PassThru -WindowStyle Hidden `
        -RedirectStandardError $errLog -RedirectStandardOutput $log -WorkingDirectory $Root
    Add-RsqOwnedPid $proc.Id
    $ready = $false
    for ($i = 0; $i -lt 30; $i++) {
        Start-Sleep -Milliseconds 500
        if ($proc.HasExited) { break }
        if (Test-PortListening -Port $listenPort -Protocol Tcp) {
            $ready = $true
            Start-Sleep -Milliseconds 800
            break
        }
    }
    if (-not $ready) {
        Stop-RsqOwned -Except @($ServerPid)
        $tail = Get-Content $log -Tail 8 -ErrorAction SilentlyContinue
        return [pscustomobject]@{ Case = $Name; Http = "NO_LISTEN"; Pass = $false; Note = ($tail -join " ") }
    }
    $prevEap = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    $raw = (curl.exe -x "http://127.0.0.1:$listenPort" -sS -o NUL -w "%{http_code}" --connect-timeout 25 --max-time 45 $TestUrl 2>$null) | Out-String
    $ErrorActionPreference = $prevEap
    $code = $raw.Trim()
    if ($code -notmatch '^\d{3}$') { $code = "" }
    Stop-RsqOwned -Except @($ServerPid)
    Start-Sleep -Seconds 1
    $ok = if ($Expect -eq "fail") { $code -notmatch "^(200|204|301|302)$" -or $code -eq "" } else { $code -match "^(200|204|301|302)$" }
    [pscustomobject]@{
        Case = $Name
        Http = if ($code) { $code } else { "FAIL" }
        Pass = $ok
    }
}

if (-not $Quiet) {
    Write-Host "=== RSQ local E2E ==="
    Write-Host "rsbox: $Rsbox"
}

Ensure-RsqCerts
Stop-RsqOwned

$serverCfg = Join-Path $Root "examples\rsq-local-server-multi.json"
& $Rsbox check -c $serverCfg 2>&1 | Out-Null
if ($LASTEXITCODE -ne 0) { Write-Error "server config invalid: $serverCfg" }

$serverLog = Join-Path $env:TEMP "rsq-test-server.log"
$serverErrLog = Join-Path $env:TEMP "rsq-test-server.err.log"
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
    Get-Content $serverLog -Tail 15 -ErrorAction SilentlyContinue
    Get-Content $serverErrLog -Tail 15 -ErrorAction SilentlyContinue
    Write-Error "RSQ server failed to listen on 18443"
}

$results = @()
$results += Test-RsqClient "user-a" (Join-Path $Root "examples\rsq-local-client-user-a.json") "ok" $serverProc.Id
$results += Test-RsqClient "user-b" (Join-Path $Root "examples\rsq-local-client-user-b.json") "ok" $serverProc.Id
$results += Test-RsqClient "bad-pass" (Join-Path $Root "examples\rsq-local-client-bad-pass.json") "fail" $serverProc.Id

Stop-RsqOwned

if (-not $Quiet) {
    Write-Host ""
    $results | Format-Table -AutoSize
    $pass = ($results | Where-Object Pass).Count
    Write-Host "=== RSQ local: $pass / $($results.Count) passed ==="
}

if (($results | Where-Object { -not $_.Pass }).Count -gt 0) { exit 1 }
exit 0
