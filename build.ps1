# Build rsbox desktop CLI and mobile rsb-libbox library.
$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

Write-Host "==> Building rsbox (desktop CLI)..." -ForegroundColor Cyan
cargo build --release -p rsbox
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "==> Building rsb-libbox (mobile FFI)..." -ForegroundColor Cyan
cargo build --release -p rsb-libbox
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host ""
Write-Host "Done." -ForegroundColor Green
Write-Host "  Desktop: target\release\rsbox.exe"
Write-Host "  Libbox:  target\release\rsb_libbox.dll (Windows) / librsb_libbox.so (Linux/Android)"
