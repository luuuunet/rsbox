param(
    [Parameter(Mandatory = $true)]
    [string]$Config,
    [string]$Rsbox = "",
    [int]$Port = 17890,
    [string]$TestUrl = "https://www.cloudflare.com/cdn-cgi/trace",
    [switch]$KeepRunning
)

$ErrorActionPreference = "Stop"

if (-not $Rsbox) {
    $candidates = @(
        "$PSScriptRoot\..\target\release\rsbox.exe",
        "$env:APPDATA\com.example\g5_client\bin\rsbox.exe"
    )
    foreach ($c in $candidates) {
        if (Test-Path $c) { $Rsbox = (Resolve-Path $c).Path; break }
    }
}
if (-not $Rsbox -or -not (Test-Path $Rsbox)) {
    Write-Error "rsbox.exe not found. Pass -Rsbox or build with: cargo build --release -p rsbox"
}

$Config = (Resolve-Path $Config).Path
Write-Host "rsbox:  $Rsbox"
Write-Host "config: $Config"

& $Rsbox check -c $Config
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$existing = Get-Process rsbox -ErrorAction SilentlyContinue
if ($existing) {
    Write-Host "Stopping existing rsbox (PID $($existing.Id -join ','))..."
    taskkill /F /IM rsbox.exe | Out-Null
    Start-Sleep -Seconds 2
}

$logDir = Join-Path $env:TEMP "rsbox-test-logs"
New-Item -ItemType Directory -Path $logDir -Force | Out-Null
$logFile = Join-Path $logDir ("run-{0:yyyyMMdd-HHmmss}.log" -f (Get-Date))

Write-Host "Starting rsbox (log: $logFile)..."
$proc = Start-Process -FilePath $Rsbox -ArgumentList @("run", "-c", $Config) -PassThru -WindowStyle Hidden -RedirectStandardError $logFile -RedirectStandardOutput $logFile

Start-Sleep -Seconds 3
if ($proc.HasExited) {
    Write-Host "--- rsbox exited early ---"
    Get-Content $logFile -Tail 40
    exit 1
}

$listen = Get-NetTCPConnection -LocalPort $Port -State Listen -ErrorAction SilentlyContinue
if (-not $listen) {
    Write-Host "--- port $Port not listening ---"
    Get-Content $logFile -Tail 40
    taskkill /F /PID $proc.Id | Out-Null
    exit 1
}

Write-Host "Proxy listening on 127.0.0.1:$Port"
Write-Host "curl $TestUrl ..."
curl.exe -x "http://127.0.0.1:$Port" -sS -o NUL -w "HTTP=%{http_code} time=%{time_total}s`n" --connect-timeout 20 --max-time 45 $TestUrl
$curlExit = $LASTEXITCODE

$closeWait = (Get-NetTCPConnection -LocalPort $Port -State CloseWait -ErrorAction SilentlyContinue | Measure-Object).Count
Write-Host "CloseWait on :$Port = $closeWait"

if (-not $KeepRunning) {
    taskkill /F /PID $proc.Id | Out-Null
    Write-Host "rsbox stopped."
} else {
    Write-Host "rsbox still running (PID $($proc.Id)). Use: taskkill /F /IM rsbox.exe"
}

exit $curlExit
