[CmdletBinding()]
param(
    [ValidateSet("default", "wizard", "legacy")]
    [string]$Rollout = "default",

    [switch]$SaveWorkspacePassword,

    [string]$BinaryPath
)

$ErrorActionPreference = "Stop"
$profileName = "local-dev"
$configHome = if ($env:AYX_CONFIG_HOME) {
    $env:AYX_CONFIG_HOME
} else {
    Join-Path $env:APPDATA "ayx"
}
$resolvedConfigHome = Resolve-Path -LiteralPath $configHome -ErrorAction SilentlyContinue |
    Select-Object -First 1
if ($resolvedConfigHome) {
    $configHome = $resolvedConfigHome.Path
}
$profileDir = Join-Path $configHome "profiles"
$profilePath = Join-Path $profileDir "$profileName.yaml"

function Read-ProfileValue {
    param([string]$Name)
    $line = Select-String -LiteralPath $profilePath -Pattern "^\s+${Name}:\s*(.+?)\s*$" |
        Select-Object -First 1
    if (-not $line) { return $null }
    return $line.Matches[0].Groups[1].Value.Trim("'`" ")
}

if (-not (Test-Path -LiteralPath $profilePath -PathType Leaf)) {
    Write-Host "Profile '$profileName' was not found at $profilePath."
    $accountEmail = Read-Host "Account email"
    $baseUrl = Read-Host "Alteryx One regional base URL (https://<region>.alteryxcloud.com)"
    $workspaceGid = Read-Host "Workspace GID"
    if ([string]::IsNullOrWhiteSpace($accountEmail) -or
        [string]::IsNullOrWhiteSpace($baseUrl) -or
        [string]::IsNullOrWhiteSpace($workspaceGid)) {
        throw "Account email, base URL, and workspace GID are required to create local-dev"
    }
    New-Item -ItemType Directory -Path $profileDir -Force | Out-Null
    @"
profile_name: local-dev
alteryx_one:
  account_email: $accountEmail
  base_url: $baseUrl
  workspace_gid: $workspaceGid
"@ | Set-Content -LiteralPath $profilePath -Encoding utf8
}

$env:AYX_CONFIG_HOME = (Resolve-Path -LiteralPath $configHome).Path
Remove-Item Env:AYX_AUTH_ROLLOUT -ErrorAction SilentlyContinue
Remove-Item Env:AUTH_ROLLOUT -ErrorAction SilentlyContinue

$baseUrl = Read-ProfileValue "base_url"
$workspaceGid = Read-ProfileValue "workspace_gid"
if ([string]::IsNullOrWhiteSpace($baseUrl) -or [string]::IsNullOrWhiteSpace($workspaceGid)) {
    throw "Profile '$profileName' must contain alteryx_one.base_url and workspace_gid"
}
if (-not $baseUrl.StartsWith("https://")) {
    throw "Profile '$profileName' base_url must use HTTPS"
}

$arguments = @(
    "--output", "json", "one", "login",
    "--profile", $profileName,
    "--workspace-gid", $workspaceGid,
    "--base-url", $baseUrl,
    "--secret-policy", "secure"
)
if ($Rollout -ne "default") {
    $arguments += @("--auth-flow", $Rollout)
}
if ($SaveWorkspacePassword) {
    $arguments += "--save-workspace-password"
}

Write-Host "Starting live authentication for existing profile '$profileName'."
Write-Host "Rollout: $Rollout; persistence: secure"
Write-Host "OTP and password prompts are interactive and are never accepted as script arguments."

$resolvedBinary = if ($BinaryPath) {
    (Resolve-Path -LiteralPath $BinaryPath).Path
} else {
    Join-Path (Get-Location) "target\release\ayx.exe"
}
if (-not (Test-Path -LiteralPath $resolvedBinary -PathType Leaf)) {
    throw "Release binary not found at $resolvedBinary; build it before running this test"
}

# Do not pipe an interactive native process: PowerShell 5.1 turns its normal
# stderr progress into ErrorRecords and can interfere with a TTY credential
# exchange. The CLI writes directly to the console instead, so credentials are
# neither recorded nor passed through PowerShell objects.
& $resolvedBinary @arguments
$exitCode = $LASTEXITCODE
if ($exitCode -ne 0) {
    throw "live authentication failed with exit code $exitCode"
}

function Invoke-ReadOnlyApiCheck {
    param(
        [string]$Label,
        [string[]]$CommandArguments
    )
    $apiCapturePath = Join-Path ([System.IO.Path]::GetTempPath()) ("ayx-live-api-" + [guid]::NewGuid().ToString() + ".log")
    $previousErrorActionPreference = $ErrorActionPreference
    try {
        # Keep native stderr available to the output/secret scan without
        # promoting ordinary CLI diagnostics to terminating PowerShell errors.
        $ErrorActionPreference = "Continue"
        & $resolvedBinary @CommandArguments 2>&1 | Tee-Object -FilePath $apiCapturePath
        $apiExitCode = $LASTEXITCODE
        $apiOutput = Get-Content -LiteralPath $apiCapturePath -Raw
    } finally {
        $ErrorActionPreference = $previousErrorActionPreference
        Remove-Item -LiteralPath $apiCapturePath -Force -ErrorAction SilentlyContinue
    }
    if ($apiExitCode -ne 0) {
        throw "$Label failed with exit code $apiExitCode"
    }
    if ($apiOutput -match '(?i)"(?:access_token|refresh_token|tokenValue|passcode|password)"\s*:') {
        throw "$Label output appears to contain secret material; do not attach this output"
    }
    Write-Host "$Label passed"
}

Invoke-ReadOnlyApiCheck "one auth status" @(
    "--output", "json", "one", "auth", "status", "--profile", $profileName
)
Invoke-ReadOnlyApiCheck "one workspace current" @(
    "--output", "json", "one", "workspace", "current", "--profile", $profileName
)
Invoke-ReadOnlyApiCheck "one workspace list" @(
    "--output", "json", "one", "workspace", "list", "--profile", $profileName, "--limit", "10"
)
Write-Host "Live authentication and API-surface checks completed for existing profile '$profileName'."
