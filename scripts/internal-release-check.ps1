[CmdletBinding()]
param(
    [switch]$SkipAudit
)

$ErrorActionPreference = "Stop"
$repo = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
Set-Location $repo

function Invoke-Checked {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Command,
        [Parameter(Mandatory = $false)]
        [string[]]$Arguments = @()
    )

    Write-Host "> $Command $($Arguments -join ' ')"
    & $Command @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "$Command failed with exit code $LASTEXITCODE"
    }
}

if (-not (Get-Command cargo-nextest -ErrorAction SilentlyContinue)) {
    throw "cargo-nextest is required; install it before running the internal release check"
}

Invoke-Checked cargo @("fmt", "--all", "--check")
Invoke-Checked cargo @("run", "-q", "-p", "xtask", "--", "refresh-command-surface", "--check")
Invoke-Checked cargo @("clippy", "--workspace", "--all-targets", "--locked", "--", "-D", "warnings")
Invoke-Checked cargo @("nextest", "run", "--workspace", "--locked")
Invoke-Checked cargo @("build", "--workspace", "--release", "--locked")
if (-not $SkipAudit) {
    Invoke-Checked cargo @("audit", "--deny", "warnings")
}

$cargoTomlMatch = Select-String -Path (Join-Path $repo "Cargo.toml") -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1
if (-not $cargoTomlMatch) {
    throw "unable to find workspace version in Cargo.toml"
}
$workspaceVersion = $cargoTomlMatch.Matches[0].Groups[1].Value
$releaseNotesName = "v$workspaceVersion-internal.1.md"
$releaseNotes = Join-Path $repo "docs\releases\$releaseNotesName"
if (-not (Test-Path -LiteralPath $releaseNotes)) {
    throw "release notes not found: docs/releases/$releaseNotesName -- create it before cutting an internal release"
}

$dist = Join-Path $repo "dist\internal"
$stage = Join-Path $dist "ayx-x86_64-pc-windows-msvc"
$archive = Join-Path $dist "ayx-x86_64-pc-windows-msvc-internal.zip"
New-Item -ItemType Directory -Path $dist -Force | Out-Null
if (Test-Path -LiteralPath $stage) {
    Remove-Item -LiteralPath $stage -Recurse -Force
}
New-Item -ItemType Directory -Path $stage -Force | Out-Null
Copy-Item (Join-Path $repo "target\release\ayx.exe") (Join-Path $stage "ayx.exe")
Copy-Item (Join-Path $repo "README.md") $stage
Copy-Item $releaseNotes $stage
Compress-Archive -Path (Join-Path $stage "*") -DestinationPath $archive -Force

$verify = Join-Path $dist "archive-smoke-windows"
if (Test-Path -LiteralPath $verify) {
    Remove-Item -LiteralPath $verify -Recurse -Force
}
New-Item -ItemType Directory -Path $verify -Force | Out-Null
Expand-Archive -LiteralPath $archive -DestinationPath $verify -Force
Invoke-Checked (Join-Path $verify "ayx.exe") @("--version")
Invoke-Checked (Join-Path $verify "ayx.exe") @("--help")
Remove-Item -LiteralPath $verify -Recurse -Force

Write-Host "Internal Windows artifact: $archive"
