param(
    [string]$Rsbox = "$env:APPDATA\com.example\g5_client\bin\rsbox.exe",
    [string]$ConfigDir = "$PSScriptRoot\..\examples\generated\protocol-tests",
    [int]$Port = 17891,
    [string]$TestUrl = "https://www.cloudflare.com",
    [string[]]$Only = @()
)

$ErrorActionPreference = "Continue"
if (-not (Test-Path $Rsbox)) { $Rsbox = "$PSScriptRoot\..\target\release\rsbox.exe" }
if (-not (Test-Path $Rsbox)) { Write-Error "rsbox not found"; exit 1 }

$configs = Get-ChildItem $ConfigDir -Filter "*.json" | Sort-Object Name
if ($Only.Count -gt 0) {
    $configs = $configs | Where-Object { $Only -contains $_.BaseName }
}
if (-not $configs) { Write-Error "No configs in $ConfigDir"; exit 1 }

function Stop-Rsbox {
    taskkill /F /IM rsbox.exe 2>$null | Out-Null
    taskkill /F /IM sing-box.exe 2>$null | Out-Null
    Start-Sleep -Seconds 1
}

function Test-WireGuard($cfgPath) {
    Stop-Rsbox
    & $Rsbox check -c $cfgPath 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) {
        return [pscustomobject]@{ Protocol = "wireguard"; Check = "FAIL"; Http = "-"; Time = "-"; CloseWait = "-" }
    }
    $isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
    if (-not $isAdmin) {
        return [pscustomobject]@{ Protocol = "wireguard"; Check = "OK"; Http = "SKIP(no-admin)"; Time = "-"; CloseWait = "-" }
    }
    $logErr = Join-Path $env:TEMP "rsbox-test-wireguard-err.log"
    $p = Start-Process -FilePath $Rsbox -ArgumentList @("run", "-c", $cfgPath) -PassThru -WindowStyle Hidden `
        -RedirectStandardError $logErr
    $iface = "wg-rsbox"
    $serverIp = "10.66.66.1"
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $ready = $false
    for ($i = 0; $i -lt 20; $i++) {
        Start-Sleep -Seconds 1
        if ($p.HasExited) { break }
        if (Get-NetAdapter -Name $iface -ErrorAction SilentlyContinue) { $ready = $true; break }
    }
    if (-not $ready) {
        Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
        $tail = Get-Content $logErr -Tail 5 -ErrorAction SilentlyContinue
        return [pscustomobject]@{ Protocol = "wireguard"; Check = "OK"; Http = "NO_TUN"; Time = "-"; CloseWait = "-"; Note = ($tail -join " ") }
    }
    $ping = ping.exe -n 2 -w 3000 $serverIp 2>&1 | Out-String
    $sw.Stop()
    Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
    Start-Sleep -Seconds 1
    $ok = $ping -match "Reply from|TTL="
    [pscustomobject]@{
        Protocol = "wireguard"
        Check    = "OK"
        Http     = if ($ok) { "PING_OK" } else { "FAIL(ping)" }
        Time     = "$([math]::Round($sw.Elapsed.TotalSeconds,1))s"
        CloseWait = "-"
    }
}

function Test-RsqRemote($cfgPath) {
    Stop-Rsbox
    & $Rsbox check -c $cfgPath 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) {
        return [pscustomobject]@{ Protocol = "rsq"; Check = "FAIL"; Http = "-"; Time = "-"; CloseWait = "-" }
    }
    $logOut = Join-Path $env:TEMP "rsbox-test-rsq-out.log"
    $logErr = Join-Path $env:TEMP "rsbox-test-rsq-err.log"
    $p = Start-Process -FilePath $Rsbox -ArgumentList @("run", "-c", $cfgPath) -PassThru -WindowStyle Hidden `
        -RedirectStandardOutput $logOut -RedirectStandardError $logErr
    $listen = $null
    for ($i = 0; $i -lt 30; $i++) {
        Start-Sleep -Milliseconds 500
        if ($p.HasExited) { break }
        $listen = Get-NetTCPConnection -LocalPort $Port -State Listen -ErrorAction SilentlyContinue
        if ($listen) { break }
    }
    if ($p.HasExited -or -not $listen) {
        Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
        $tail = Get-Content $logErr -Tail 6 -ErrorAction SilentlyContinue
        return [pscustomobject]@{ Protocol = "rsq"; Check = "OK"; Http = "NO_LISTEN"; Time = "-"; CloseWait = "-"; Note = ($tail -join " ") }
    }
    Start-Sleep -Milliseconds 800
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $code = (curl.exe -x "http://127.0.0.1:$Port" -sS -o NUL -w "%{http_code}" --connect-timeout 30 --max-time 60 $TestUrl 2>$null) | Out-String
    $code = ($code -replace '[^\d]', '').Trim()
    $sw.Stop()
    Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
    Start-Sleep -Seconds 1
    $ok = $code -match "^(200|204|301|302)$"
    [pscustomobject]@{
        Protocol = "rsq"
        Check    = "OK"
        Http     = if ($ok) { $code } else { "FAIL($code)" }
        Time     = "$([math]::Round($sw.Elapsed.TotalSeconds,1))s"
        CloseWait = "-"
    }
}

function Test-RsqLocal {
    $basic = Join-Path $PSScriptRoot "test-rsq-local.ps1"
    $sub = Join-Path $PSScriptRoot "test-rsq-subscription-local.ps1"
    & $basic -Quiet
    if ($LASTEXITCODE -ne 0) {
        return [pscustomobject]@{ Protocol = "rsq"; Check = "OK"; Http = "FAIL(basic)"; Time = "-"; CloseWait = "-" }
    }
    & $sub -Quiet
    if ($LASTEXITCODE -ne 0) {
        return [pscustomobject]@{ Protocol = "rsq"; Check = "OK"; Http = "FAIL(sub)"; Time = "-"; CloseWait = "-" }
    }
    return [pscustomobject]@{ Protocol = "rsq"; Check = "OK"; Http = "200"; Time = "-"; CloseWait = "-" }
}

function Test-Protocol($name, $cfgPath) {
    if ($name -eq "wireguard") {
        return Test-WireGuard $cfgPath
    }
    if ($name -eq "rsq") {
        $raw = Get-Content $cfgPath -Raw -ErrorAction SilentlyContinue
        if ($raw -match '"server"\s*:\s*"127\.0\.0\.1"') {
            return Test-RsqLocal
        }
        return Test-RsqRemote $cfgPath
    }
    Stop-Rsbox
    & $Rsbox check -c $cfgPath 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) {
        return [pscustomobject]@{ Protocol = $name; Check = "FAIL"; Http = "-"; Time = "-"; CloseWait = "-" }
    }
    $logOut = Join-Path $env:TEMP "rsbox-test-$name-out.log"
    $logErr = Join-Path $env:TEMP "rsbox-test-$name-err.log"
    $p = Start-Process -FilePath $Rsbox -ArgumentList @("run", "-c", $cfgPath) -PassThru -WindowStyle Hidden `
        -RedirectStandardOutput $logOut -RedirectStandardError $logErr
    $listen = $null
    for ($i = 0; $i -lt 20; $i++) {
        Start-Sleep -Milliseconds 500
        if ($p.HasExited) { break }
        $listen = Get-NetTCPConnection -LocalPort $Port -State Listen -ErrorAction SilentlyContinue
        if ($listen) { break }
    }
    if ($p.HasExited) {
        $tail = Get-Content $logErr -Tail 5 -ErrorAction SilentlyContinue
        return [pscustomobject]@{ Protocol = $name; Check = "OK"; Http = "CRASH"; Time = "-"; CloseWait = "-"; Note = ($tail -join " ") }
    }
    $listen = Get-NetTCPConnection -LocalPort $Port -State Listen -ErrorAction SilentlyContinue
    if (-not $listen) {
        Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
        return [pscustomobject]@{ Protocol = $name; Check = "OK"; Http = "NO_LISTEN"; Time = "-"; CloseWait = "-" }
    }
    $maxRetry = if ($name -match 'reality|shadowtls') { 3 } else { 1 }
    $connectTimeout = if ($name -match 'shadowtls') { 45 } else { 20 }
    $maxTime = if ($name -match 'shadowtls') { 90 } else { 40 }
    $httpOk = $false
    $code = ""
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    for ($attempt = 1; $attempt -le $maxRetry; $attempt++) {
        $curlErr = Join-Path $env:TEMP "rsbox-curl-$name.err"
        $code = (curl.exe -x "http://127.0.0.1:$Port" -sS -o NUL -w "%{http_code}" --connect-timeout $connectTimeout --max-time $maxTime $TestUrl 2> $curlErr) | Out-String
        $code = ($code -replace '[^\d]', '').Trim()
        # Windows schannel may return exit 56 after a successful HTTP response.
        $httpOk = $code -match "^(200|204|301|302)$"
        if ($httpOk -or $attempt -eq $maxRetry) { break }
        Start-Sleep -Seconds 1
    }
    $sw.Stop()
    Start-Sleep -Seconds 1
    $c = Get-NetTCPConnection -LocalPort $Port -ErrorAction SilentlyContinue
    $cw = ($c | Where-Object State -eq CloseWait).Count
    Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
    Start-Sleep -Seconds 1
    [pscustomobject]@{
        Protocol = $name
        Check    = "OK"
        Http     = if ($httpOk) { $code.Trim() } else { "FAIL($code)" }
        Time     = "$([math]::Round($sw.Elapsed.TotalSeconds,1))s"
        CloseWait = $cw
    }
}

Write-Host "=== rsbox protocol test ($($configs.Count) protocols) ==="
Write-Host "rsbox: $Rsbox"
Write-Host ""

$results = @()
foreach ($f in $configs) {
    $name = $f.BaseName
    Write-Host -NoNewline "Testing $name ... "
    $r = Test-Protocol $name $f.FullName
    $results += $r
    $status = if ($r.Http -match "200|204|301|302|PING_OK") { "PASS" } elseif ($r.Http -match "SKIP") { "SKIP" } else { "FAIL" }
    Write-Host $status
}

Write-Host ""
$results | Format-Table -AutoSize
$pass = ($results | Where-Object { $_.Http -match "200|204|301|302|PING_OK" }).Count
$skip = ($results | Where-Object { $_.Http -match "SKIP" }).Count
Write-Host "=== $pass / $($results.Count) passed ($skip skipped) ==="
