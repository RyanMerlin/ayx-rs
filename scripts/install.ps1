param(
  [string]$Version = $env:AYX_VERSION,
  [string]$InstallDir = $env:AYX_INSTALL_DIR
)

$ErrorActionPreference = 'Stop'

if (-not $Version) { $Version = 'latest' }

$repoOwner = 'RyanMerlin'
$repoName = 'ayx-rs'
$artifactName = 'ayx-x86_64-pc-windows-msvc.zip'

function Test-OnPath {
  param([string]$PathToCheck)

  $pathEntries = @($env:PATH -split ';' | Where-Object { $_ })
  foreach ($entry in $pathEntries) {
    if ([System.IO.Path]::GetFullPath($entry.TrimEnd('\')) -eq [System.IO.Path]::GetFullPath($PathToCheck.TrimEnd('\'))) {
      return $true
    }
  }
  return $false
}

function Add-ToUserPath {
  param([string]$PathToAdd)

  $currentUserPath = [Environment]::GetEnvironmentVariable('Path', 'User')
  $entries = @()
  if ($currentUserPath) {
    $entries = @($currentUserPath -split ';' | Where-Object { $_ })
  }

  if ($entries -contains $PathToAdd) {
    return
  }

  $newUserPath = if ($currentUserPath) { "$currentUserPath;$PathToAdd" } else { $PathToAdd }
  [Environment]::SetEnvironmentVariable('Path', $newUserPath, 'User')
  $env:Path = "$PathToAdd;$env:Path"
}

function Get-InstallDir {
  if ($InstallDir) { return $InstallDir }

  $candidates = @(
    (Join-Path $HOME '.local\bin'),
    (Join-Path $HOME 'bin')
  )

  foreach ($candidate in $candidates) {
    $parent = Split-Path -Parent $candidate
    if (-not (Test-Path $parent)) { continue }
    try {
      New-Item -ItemType Directory -Force -Path $candidate | Out-Null
      return $candidate
    } catch {
      continue
    }
  }

  return (Join-Path $HOME '.local\bin')
}

$InstallDir = Get-InstallDir

function Require-Command {
  param([string]$Name)

  if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
    throw "missing required command: $Name"
  }
}

Require-Command Invoke-WebRequest
Require-Command Expand-Archive

$downloadUrl = if ($Version -eq 'latest') {
  "https://github.com/$repoOwner/$repoName/releases/latest/download/$artifactName"
} else {
  "https://github.com/$repoOwner/$repoName/releases/download/$Version/$artifactName"
}
$sumsUrl = if ($Version -eq 'latest') {
  "https://github.com/$repoOwner/$repoName/releases/latest/download/SHA256SUMS"
} else {
  "https://github.com/$repoOwner/$repoName/releases/download/$Version/SHA256SUMS"
}
$sigstoreUrl = if ($Version -eq 'latest') {
  "https://github.com/$repoOwner/$repoName/releases/latest/download/$artifactName.sigstore"
} else {
  "https://github.com/$repoOwner/$repoName/releases/download/$Version/$artifactName.sigstore"
}

$tmpDir = Join-Path ([System.IO.Path]::GetTempPath()) ("ayx-install-" + [guid]::NewGuid().ToString())
New-Item -ItemType Directory -Force -Path $tmpDir | Out-Null

try {
  $archivePath = Join-Path $tmpDir $artifactName
  try {
    Invoke-WebRequest -Uri $downloadUrl -OutFile $archivePath
  } catch {
    throw "failed to download $downloadUrl. $($_.Exception.Message)"
  }

  # Verify integrity against SHA256SUMS. Operators may bypass with
  # $env:AYX_SKIP_CHECKSUM='1' only for explicit reasons.
  if ($env:AYX_SKIP_CHECKSUM -ne '1') {
    $sumsPath = Join-Path $tmpDir 'SHA256SUMS'
    try {
      Invoke-WebRequest -Uri $sumsUrl -OutFile $sumsPath
    } catch {
      Write-Error "Could not fetch SHA256SUMS from $sumsUrl."
      Write-Error "Set `$env:AYX_SKIP_CHECKSUM='1' to install anyway (NOT recommended)."
      throw
    }
    $expectedLine = Get-Content $sumsPath | Where-Object { $_ -match "\s$([regex]::Escape($artifactName))$" } | Select-Object -First 1
    if (-not $expectedLine) {
      throw "SHA256SUMS does not contain an entry for $artifactName"
    }
    $expected = ($expectedLine -split '\s+')[0].ToLowerInvariant()
    $actual = (Get-FileHash -Path $archivePath -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($expected -ne $actual) {
      throw "checksum mismatch: expected $expected got $actual. Refusing to install a corrupted or tampered archive."
    }
    Write-Host "Checksum verified: $actual"
  }

  if ($env:AYX_VERIFY_SIGSTORE -eq '1') {
    $cosign = Get-Command cosign -ErrorAction SilentlyContinue
    if (-not $cosign) {
      throw "AYX_VERIFY_SIGSTORE=1 requires cosign on PATH"
    }
    $bundlePath = Join-Path $tmpDir "$artifactName.sigstore"
    Invoke-WebRequest -Uri $sigstoreUrl -OutFile $bundlePath
    & $cosign.Source verify-blob `
      --certificate-identity-regexp "^https://github.com/$repoOwner/$repoName/" `
      --certificate-oidc-issuer "https://token.actions.githubusercontent.com" `
      --bundle $bundlePath `
      $archivePath
    if ($LASTEXITCODE -ne 0) {
      throw "sigstore verification failed for $artifactName"
    }
    Write-Host "Sigstore provenance verified."
  }

  New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
  $extractDir = Join-Path $tmpDir 'extract'
  New-Item -ItemType Directory -Force -Path $extractDir | Out-Null
  try {
    Expand-Archive -Path $archivePath -DestinationPath $extractDir -Force
  } catch {
    $listing = & tar -tf $archivePath 2>$null
    if ($listing) {
      Write-Host "archive contents:"
      $listing | ForEach-Object { Write-Host $_ }
    }
    throw "failed to extract $downloadUrl. $($_.Exception.Message)"
  }

  $binaryPath = Get-ChildItem -Path $extractDir -Recurse -File -Filter 'ayx.exe' | Select-Object -First 1
  if (-not $binaryPath) {
    Write-Host "archive contents:"
    Get-ChildItem -Path $extractDir -Recurse | ForEach-Object { Write-Host $_.FullName }
    throw 'downloaded archive did not contain ayx.exe'
  }

  Copy-Item $binaryPath.FullName -Destination (Join-Path $InstallDir 'ayx.exe') -Force
  Add-ToUserPath $InstallDir

  Write-Host "installed ayx to $InstallDir\ayx.exe"
  if (Test-OnPath $InstallDir) {
    Write-Host "$InstallDir is already on your PATH"
  } else {
    Write-Host "added $InstallDir to your user PATH"
    Write-Host "open a new shell to use ayx immediately"
  }

  # Shadow check: a different `ayx` earlier on PATH silently wins (e.g. a stale
  # `cargo install` copy under ~/.cargo/bin, which rustup places ahead of
  # ~/.local/bin). We resolve against the PATH a freshly-opened shell will see —
  # the persisted Machine + User values, NOT this process's PATH, which we may
  # have just prepended $InstallDir to — so the warning reflects reality.
  $installedExe = Join-Path $InstallDir 'ayx.exe'
  $newShellPath = @(
    [Environment]::GetEnvironmentVariable('Path', 'Machine'),
    [Environment]::GetEnvironmentVariable('Path', 'User')
  ) -join ';'
  $shadow = $null
  foreach ($dir in @($newShellPath -split ';' | Where-Object { $_ })) {
    # Join-Path handles trailing backslashes (incl. drive roots like `C:\`);
    # do not TrimEnd, which would turn `C:\` into drive-relative `C:`.
    $candidate = Join-Path $dir 'ayx.exe'
    # -LiteralPath + SilentlyContinue: a malformed/wildcard PATH entry must not
    # turn this read-only diagnostic into a terminating failure under Stop.
    if (Test-Path -LiteralPath $candidate -ErrorAction SilentlyContinue) {
      if ([System.IO.Path]::GetFullPath($candidate) -ne [System.IO.Path]::GetFullPath($installedExe)) {
        $shadow = $candidate
      }
      break
    }
  }
  if ($shadow) {
    Write-Warning "another 'ayx' is earlier on your PATH and will shadow this install:"
    Write-Warning "    shadow:    $shadow"
    Write-Warning "    installed: $installedExe"
    Write-Warning "Remove the shadowing copy (e.g. Remove-Item '$shadow'), or move"
    Write-Warning "$InstallDir ahead of it on PATH, then reopen your shell."
  }

  # Optional: install PowerShell completions. Best-effort.
  # Skip entirely when $env:AYX_SKIP_COMPLETIONS = '1'.
  if ($env:AYX_SKIP_COMPLETIONS -ne '1') {
    try {
      $profileDir = Split-Path -Parent $PROFILE
      if (-not (Test-Path $profileDir)) {
        New-Item -ItemType Directory -Force -Path $profileDir | Out-Null
      }
      $completionPath = Join-Path $profileDir 'ayx-completions.ps1'
      & (Join-Path $InstallDir 'ayx.exe') completions powershell | Set-Content -Path $completionPath -Encoding UTF8
      Write-Host "installed PowerShell completions to $completionPath"
      Write-Host "add '. $completionPath' to your `$PROFILE to enable them on shell start"
    } catch {
      Write-Host "skipped PowerShell completions: $($_.Exception.Message)"
    }
  }
}
finally {
  Remove-Item -Recurse -Force $tmpDir -ErrorAction SilentlyContinue
}
