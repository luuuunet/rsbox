# Convenience wrapper: build rsbox and deploy into g5_client.
param(
    [string]$G5Root = "D:\g5-client",
    [switch]$SkipBuild,
    [switch]$Launch
)

$ErrorActionPreference = "Stop"
$RsboxRoot = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$DeployScript = Join-Path $G5Root "scripts\deploy-rsbox.ps1"

if (-not (Test-Path $DeployScript)) {
    throw "G5 deploy script not found: $DeployScript (use -G5Root to override)"
}

$params = @{
    RsboxRoot = $RsboxRoot
}
if ($SkipBuild) { $params.SkipBuild = $true }
if ($Launch) { $params.Launch = $true }

& $DeployScript @params
