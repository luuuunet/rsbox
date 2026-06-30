param(
    [string]$Rsbox = "$PSScriptRoot\..\target\release\rsbox.exe",
    [string]$Config = "$PSScriptRoot\..\examples\generated\protocol-tests\shadowtls-ss-v3.json",
    [string]$TestUrl = "https://www.cloudflare.com"
)

$name = [System.IO.Path]::GetFileNameWithoutExtension($Config)
& "$PSScriptRoot\test-all-protocols.ps1" `
    -Rsbox $Rsbox `
    -ConfigDir (Split-Path $Config -Parent) `
    -TestUrl $TestUrl `
    -Only $name
