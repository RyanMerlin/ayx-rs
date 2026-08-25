[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$ConfigHome,

    [Parameter(Mandatory = $true)]
    [string]$Profile,

    [Parameter(Mandatory = $true)]
    [string]$WorkspaceGid,

    [Parameter(Mandatory = $true)]
    [uri]$BaseUrl,

    [ValidateSet("legacy", "wizard", "canary")]
    [string]$Rollout = "canary",

    [switch]$UseDefaultWizard,

    [string]$BinaryPath,

    [ValidateSet("session", "secure")]
    [string]$SecretPolicy = "session",

    [switch]$SaveWorkspacePassword
)

$ErrorActionPreference = "Stop"

if (-not (Test-Path -LiteralPath $ConfigHome -PathType Container)) {
    New-Item -ItemType Directory -Path $ConfigHome -Force | Out-Null
}

# The canary namespace prevents binding-derived keyring accounts from colliding
# with ordinary credentials. Session-only is the default so an operator can
# prove the real OTP/PAT exchange without persisting a credential. The default
# lane is Wizard; `legacy` selects the compatibility-pinned OTP lane, while
# `canary` additionally isolates any keyring writes under the canary namespace.
$env:AYX_CONFIG_HOME = (Resolve-Path -LiteralPath $ConfigHome).Path
if ($UseDefaultWizard) {
    Remove-Item Env:AYX_AUTH_ROLLOUT -ErrorAction SilentlyContinue
    Remove-Item Env:AUTH_ROLLOUT -ErrorAction SilentlyContinue
} else {
    $env:AYX_AUTH_ROLLOUT = $Rollout
}
if (-not $UseDefaultWizard -and $Rollout -eq "canary") {
    $env:AYX_AUTH_LIVE_CANARY = "1"
} else {
    Remove-Item Env:AYX_AUTH_LIVE_CANARY -ErrorAction SilentlyContinue
}

if ($BaseUrl.Scheme -ne "https" -or [string]::IsNullOrWhiteSpace($BaseUrl.Host)) {
    throw "BaseUrl must be an HTTPS Alteryx One regional URL"
}
$normalizedBaseUrl = $BaseUrl.GetLeftPart([System.UriPartial]::Authority).TrimEnd('/')

$arguments = @(
    "--output", "json", "one", "login",
    "--profile", $Profile,
    "--workspace-gid", $WorkspaceGid,
    "--base-url", $normalizedBaseUrl,
    "--secret-policy", $SecretPolicy
)
if ($SaveWorkspacePassword) {
    $arguments += "--save-workspace-password"
}

Write-Host "Starting isolated live authentication canary for profile '$Profile'."
Write-Host "OTP and any password prompts remain interactive; no secret is accepted as a script argument."
if ($UseDefaultWizard) {
    Write-Host "Rollout: default Wizard (AYX_AUTH_ROLLOUT unset)"
} else {
    Write-Host "Rollout: $Rollout"
}
if ([string]::IsNullOrWhiteSpace($BinaryPath)) {
    $output = (& cargo run -q -p ayx-rs -- @arguments 2>&1 | Out-String)
} else {
    $resolvedBinary = (Resolve-Path -LiteralPath $BinaryPath).Path
    $output = (& $resolvedBinary @arguments 2>&1 | Out-String)
}
$exitCode = $LASTEXITCODE

if ($exitCode -ne 0) {
    throw "live authentication canary failed with exit code $exitCode"
}
if ($output -notmatch "Token expires:") {
    throw "live authentication canary did not report the PAT expiry"
}

# The login envelope intentionally reports lengths and policy, never token or
# password values. Catch an accidental secret-bearing response before calling
# this run evidence.
if ($output -match '(?i)"(?:access_token|refresh_token|tokenValue|passcode|password)"\s*:') {
    throw "live authentication output appears to contain secret material; do not attach this output"
}

# Only emit output after the complete safety scan. The replacement is a second
# defense for structured output if a future response adds a known secret field
# without matching the strict check above.
$safeOutput = $output -replace '(?i)("?(?:access_token|refresh_token|tokenValue|passcode|password)"?\s*[:=]\s*)("[^"]*"|[^,\s}]+)', '$1<redacted>'
Write-Output $safeOutput

Write-Host "Live authentication canary completed. Preserve the isolated config home for review; do not reuse it as production config."
