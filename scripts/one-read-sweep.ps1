[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$BinaryPath,

    [string]$ConfigHome = "C:\code\ayx-win-otp-test",

    [string]$Profile = "windows-otp",

    [ValidateSet("text", "json")]
    [string]$Output = "text",

    [switch]$Quiet,

    [string]$WorkspaceId = "91946",
    [string]$WorkspaceGid = "01KMGF85WTTEJZ397MW1RBD9ZB",
    [string]$WorkflowId = "01KVWJA412RB8PJ9CTA4BF67SH",
    [string]$ConnectionId = "44865",
    [string]$JobGroupId = "4087561",
    [string]$PlanId = "128557",
    [string]$ScheduleId = "39506",
    [string]$ConnectorSlug = "gsheetsuser",
    [string]$LogDirectory = $env:TEMP
)

<#
.SYNOPSIS
Runs the read-only Alteryx One Windows validation sweep against known test assets.

.DESCRIPTION
Every API command in this script is read-only. It never passes --apply, never
launches a browser, and never places credentials in command-line arguments.
The JSON summary log contains only command labels, exit codes, and timing. API
response bodies stay on the console for reviewer inspection unless `-Quiet` is
used for an unattended status-only pass.

Designer Cloud's legacy `ayx one flows` family is intentionally excluded from
this acceptance sweep. Cloud-native `ayx one workflows` is the One workflow
surface under test.
#>

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if (-not (Test-Path -LiteralPath $BinaryPath -PathType Leaf)) {
    throw "ayx binary not found: $BinaryPath"
}
if (-not (Test-Path -LiteralPath $ConfigHome -PathType Container)) {
    throw "AYX_CONFIG_HOME does not exist: $ConfigHome"
}
if (-not (Test-Path -LiteralPath (Join-Path $ConfigHome "profiles\$Profile.yaml") -PathType Leaf)) {
    throw "Profile '$Profile' was not found under $ConfigHome\profiles"
}
if (-not (Test-Path -LiteralPath $LogDirectory -PathType Container)) {
    New-Item -ItemType Directory -Path $LogDirectory -Force | Out-Null
}

$resolvedBinary = (Resolve-Path -LiteralPath $BinaryPath).Path
$env:AYX_CONFIG_HOME = (Resolve-Path -LiteralPath $ConfigHome).Path
$runStarted = Get-Date
$results = [System.Collections.Generic.List[object]]::new()

function Invoke-OneRead {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Label,

        [Parameter(Mandatory = $true)]
        [string[]]$Arguments,

        [int[]]$ExpectedExitCodes = @(0)
    )

    Write-Host "`n=== $Label ===" -ForegroundColor Cyan
    Write-Host ("ayx " + ($Arguments -join " ")) -ForegroundColor DarkGray
    $started = Get-Date
    $previousErrorActionPreference = $ErrorActionPreference
    try {
        # Native stderr is diagnostic output, not a PowerShell exception.
        $ErrorActionPreference = "Continue"
        if ($Quiet) {
            & $resolvedBinary @Arguments *>&1 | Out-Null
        } else {
            & $resolvedBinary @Arguments
        }
        $exitCode = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $previousErrorActionPreference
    }

    $passed = $ExpectedExitCodes -contains $exitCode
    $results.Add([pscustomobject]@{
        label = $Label
        command = "ayx " + ($Arguments -join " ")
        exit_code = $exitCode
        expected_exit_codes = $ExpectedExitCodes
        passed = $passed
        elapsed_ms = [int][Math]::Round(((Get-Date) - $started).TotalMilliseconds)
    })
    if ($passed) {
        Write-Host "PASS ($exitCode)" -ForegroundColor Green
    } else {
        Write-Warning "FAILED: exit code $exitCode; expected $($ExpectedExitCodes -join ', ')"
    }
}

function OneArgs {
    param([string[]]$Command)
    # The profile is active in AYX_CONFIG_HOME state. Most One leaves do not
    # accept an individual --profile flag, so do not append one here.
    return @("one") + $Command + @("--output", $Output, "--no-input")
}

# Product/auth baseline. A successful OTP login is valid even though it does
# not carry a refresh token; record doctor warnings as findings, not success.
Invoke-OneRead "version" @("--version")
Invoke-OneRead "profile show" @("profile", "show", $Profile, "--output", $Output)
Invoke-OneRead "One auth status" (OneArgs @("auth", "status"))
Invoke-OneRead "One auth diagnose" (OneArgs @("auth", "diagnose"))
Invoke-OneRead "One current user" (OneArgs @("whoami"))
Invoke-OneRead "One doctor auth" (OneArgs @("doctor", "auth"))
Invoke-OneRead "One doctor identity" (OneArgs @("doctor", "identity"))
Invoke-OneRead "One doctor discovery" (OneArgs @("doctor", "discover"))

# Workspace and membership. `person list` and `workspace people` are both
# deliberately run while their duplication/consolidation is tracked.
Invoke-OneRead "workspace current" (OneArgs @("workspace", "current"))
Invoke-OneRead "workspace current via GID selector" (OneArgs @("workspace", "current", "--workspace", $WorkspaceGid))
Invoke-OneRead "workspace list" (OneArgs @("workspace", "list"))
Invoke-OneRead "workspace detail" (OneArgs @("workspace", "detail", $WorkspaceId))
Invoke-OneRead "workspace current configuration" (OneArgs @("workspace", "current-configuration"))
Invoke-OneRead "workspace configuration schema" (OneArgs @("workspace", "current-configuration-schema"))
Invoke-OneRead "workspace people" (OneArgs @("workspace", "people"))
Invoke-OneRead "workspace admins" (OneArgs @("workspace", "admins"))
Invoke-OneRead "workspace groups" (OneArgs @("workspace", "groups"))
Invoke-OneRead "workspace global groups" (OneArgs @("workspace", "groups-global"))
Invoke-OneRead "workspace cloud configs" (OneArgs @("workspace", "cloud-configs", $WorkspaceId))
Invoke-OneRead "person current" (OneArgs @("person", "current"))
Invoke-OneRead "person list" (OneArgs @("person", "list", "--all", "--output-limit", "5"))
Invoke-OneRead "managed IAM roles" (OneArgs @("role", "list"))
Invoke-OneRead "API access-token list" (OneArgs @("token", "list"))

# Cloud-native One workflows. These use ULIDs and the /svc-workflow service;
# no run/cancel/copy/share/upload/delete command is invoked in this read sweep.
Invoke-OneRead "workflow list with pagination" (OneArgs @("workflows", "list", "--all", "--output-limit", "5"))
Invoke-OneRead "workflow count" (OneArgs @("workflows", "count"))
Invoke-OneRead "workflow detail" (OneArgs @("workflows", "detail", $WorkflowId))
Invoke-OneRead "workflow dependencies" (OneArgs @("workflows", "dependencies", $WorkflowId))
Invoke-OneRead "workflow assets" (OneArgs @("workflows", "assets", "--output-limit", "5"))
Invoke-OneRead "workflow engines" (OneArgs @("workflows", "engines", $WorkflowId))
Invoke-OneRead "workflow tools" (OneArgs @("workflows", "tools"))
Invoke-OneRead "workflow console URL" (OneArgs @("open", "workflow", $WorkflowId, "--print"))

# Connections. Connector metadata requires a known connector slug because the
# public v4 API does not offer connector enumeration.
Invoke-OneRead "connection list" (OneArgs @("connections", "list", "--all", "--output-limit", "5"))
Invoke-OneRead "connection count" (OneArgs @("connections", "count"))
Invoke-OneRead "connection detail" (OneArgs @("connections", "detail", $ConnectionId))
Invoke-OneRead "connection status" (OneArgs @("connections", "status", $ConnectionId))
Invoke-OneRead "connection permissions" (OneArgs @("connections", "permissions", "list", $ConnectionId))
Invoke-OneRead "connector defaults" (OneArgs @("connections", "connector-metadata", "defaults", $ConnectorSlug))
Invoke-OneRead "connector publish info (may be scope denied)" (OneArgs @("connections", "connector-metadata", "publish-info", $ConnectorSlug)) @(0, 5)
Invoke-OneRead "connector metadata" (OneArgs @("connections", "connector-metadata", "detail", $ConnectorSlug))

# The current command label is intentionally retained for this run so its live
# semantics can be tested before the planned rename from job-groups to jobs.
Invoke-OneRead "job-library list" (OneArgs @("job-groups", "list", "--all", "--output-limit", "5"))
Invoke-OneRead "job-library count" (OneArgs @("job-groups", "count"))
Invoke-OneRead "job-library detail" (OneArgs @("job-groups", "detail", $JobGroupId))
Invoke-OneRead "job-library status" (OneArgs @("job-groups", "status", $JobGroupId))
Invoke-OneRead "job-library inputs (fixture may not be JDBC)" (OneArgs @("job-groups", "inputs", $JobGroupId)) @(0, 2)
Invoke-OneRead "job-library outputs" (OneArgs @("job-groups", "outputs", $JobGroupId))
Invoke-OneRead "job-library jobs" (OneArgs @("job-groups", "jobs", $JobGroupId))
Invoke-OneRead "job-library publications" (OneArgs @("job-groups", "publications", $JobGroupId))
Invoke-OneRead "job-library profile (fixture may have no profiling data)" (OneArgs @("job-groups", "profile", $JobGroupId)) @(0, 2)
Invoke-OneRead "job-library profile results (fixture may have no profiling data)" (OneArgs @("job-groups", "profile-results", $JobGroupId)) @(0, 2)
Invoke-OneRead "job-library PDF results (fixture may have no profiling data)" (OneArgs @("job-groups", "pdf-results", $JobGroupId)) @(0, 2)

# Plans and schedules are tier-dependent. A nonzero result is a capability or
# permissions finding to record, not a reason to retry with --apply.
Invoke-OneRead "plans list" (OneArgs @("plans", "list", "--all", "--output-limit", "5"))
Invoke-OneRead "plans count" (OneArgs @("plans", "count"))
Invoke-OneRead "plan detail" (OneArgs @("plans", "detail", $PlanId))
Invoke-OneRead "plan full" (OneArgs @("plans", "full", $PlanId))
Invoke-OneRead "plan run parameters" (OneArgs @("plans", "run-parameters", $PlanId))
Invoke-OneRead "plan schedules" (OneArgs @("plans", "schedules", $PlanId))
Invoke-OneRead "plan permissions" (OneArgs @("plans", "permissions", $PlanId))
Invoke-OneRead "One doctor plans" (OneArgs @("doctor", "plans"))
Invoke-OneRead "scheduling list" (OneArgs @("scheduling", "list", "--all", "--output-limit", "5"))
Invoke-OneRead "scheduling count" (OneArgs @("scheduling", "count"))
Invoke-OneRead "scheduling detail" (OneArgs @("scheduling", "detail", $ScheduleId))
Invoke-OneRead "One doctor scheduling" (OneArgs @("doctor", "scheduling"))

# Data-prep-adjacent / empty-state families. An empty successful response is
# useful evidence; do not create sample assets during an acceptance sweep.
Invoke-OneRead "dataset list" (OneArgs @("datasets", "list"))
Invoke-OneRead "dataset count" (OneArgs @("datasets", "count"))
Invoke-OneRead "wrangled dataset list" (OneArgs @("datasets", "wrangled", "list"))
Invoke-OneRead "wrangled dataset count" (OneArgs @("datasets", "wrangled", "count"))
Invoke-OneRead "output-object list" (OneArgs @("output-objects", "list"))
Invoke-OneRead "output-object count" (OneArgs @("output-objects", "count"))
Invoke-OneRead "write-setting list" (OneArgs @("write-settings", "list"))
Invoke-OneRead "write-setting count" (OneArgs @("write-settings", "count"))

# API registry/introspection. Keep the raw spec out of this sweep because it
# is large; status, diagnostics, inventory, and coverage exercise the paths.
Invoke-OneRead "One inventory" (OneArgs @("inventory"))
Invoke-OneRead "One API status" (OneArgs @("api", "status"))
Invoke-OneRead "One API diagnose" (OneArgs @("api", "diagnose"))
Invoke-OneRead "One API coverage" (OneArgs @("api", "coverage"))

$runFinished = Get-Date
$summary = [pscustomobject]@{
    schema = "ayx.one-read-sweep.v1"
    started_at = $runStarted.ToString("o")
    finished_at = $runFinished.ToString("o")
    binary = $resolvedBinary
    config_home = $env:AYX_CONFIG_HOME
    profile = $Profile
    output = $Output
    passed = @($results | Where-Object passed).Count
    failed = @($results | Where-Object { -not $_.passed }).Count
    results = $results
}
$summaryPath = Join-Path $LogDirectory ("ayx-one-read-sweep-" + $runStarted.ToString("yyyyMMdd-HHmmss") + ".json")
$summary | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $summaryPath -Encoding utf8

Write-Host "`nSummary: $($summary.passed) passed, $($summary.failed) failed" -ForegroundColor Cyan
Write-Host "Safe command/timing summary: $summaryPath"
if ($summary.failed -gt 0) {
    exit 1
}
