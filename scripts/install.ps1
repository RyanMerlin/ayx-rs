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

$tmpDir = Join-Path ([System.IO.Path]::GetTempPath()) ("ayx-install-" + [guid]::NewGuid().ToString())
New-Item -ItemType Directory -Force -Path $tmpDir | Out-Null

try {
  $archivePath = Join-Path $tmpDir $artifactName
  Invoke-WebRequest -Uri $downloadUrl -OutFile $archivePath

  New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
  Expand-Archive -Path $archivePath -DestinationPath $InstallDir -Force

  Write-Host "installed ayx to $InstallDir\ayx.exe"
  if (Test-OnPath $InstallDir) {
    Write-Host "$InstallDir is already on your PATH"
  } else {
    Write-Host "make sure $InstallDir is on your PATH"
    Write-Host "for this session: `$env:PATH = `"$InstallDir;$env:PATH`""
  }
}
finally {
  Remove-Item -Recurse -Force $tmpDir -ErrorAction SilentlyContinue
}
