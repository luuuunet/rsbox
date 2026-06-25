# 推送 GitHub Actions workflow（需要 gh 已授权 workflow scope）
# 1. gh auth refresh -h github.com -s workflow
# 2. .\scripts\push-workflows.ps1

$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
Set-Location $root

$files = @(
    ".github/workflows/ci.yml",
    ".github/workflows/release.yml",
    ".github/workflows/docker.yml"
)

$msg = "Add GitHub Actions for CI, release, and Docker builds"

foreach ($path in $files) {
    Write-Host "Uploading $path ..."
    $content = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes((Get-Content -Raw $path)))
    $sha = gh api "repos/luuuunet/rsbox/contents/$path" --jq .sha 2>$null
    $args = @(
        "api", "-X", "PUT", "repos/luuuunet/rsbox/contents/$path",
        "-f", "message=$msg",
        "-f", "content=$content",
        "-f", "branch=main"
    )
    if ($sha) { $args += @("-f", "sha=$sha") }
    gh @args | Out-Null
    Write-Host "  OK"
}

Write-Host ""
Write-Host "Creating tag v0.1.0 (triggers release build) ..."
$mainSha = gh api repos/luuuunet/rsbox/git/ref/heads/main --jq .object.sha
gh api repos/luuuunet/rsbox/git/refs -f ref="refs/tags/v0.1.0" -f sha="$mainSha" 2>$null
if ($LASTEXITCODE -ne 0) {
    gh api -X PATCH repos/luuuunet/rsbox/git/refs/tags/v0.1.0 -f sha="$mainSha" -f force=true
}
Write-Host "Done. Check: https://github.com/luuuunet/rsbox/actions"
