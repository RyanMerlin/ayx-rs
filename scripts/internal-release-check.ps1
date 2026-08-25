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
Copy-Item (Join-Path $repo "scripts\install.ps1") $stage
Copy-Item (Join-Path $repo "docs\releases\v0.17.0-internal.1.md") $stage
Compress-Archive -Path (Join-Path $stage "*") -DestinationPath $archive -Force

Write-Host "Internal Windows artifact: $archive"
