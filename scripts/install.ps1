param(
  [string]$Version = $env:AYX_VERSION,
  [string]$InstallDir = $env:AYX_INSTALL_DIR
)

$ErrorActionPreference = 'Stop'

if (-not $Version) { $Version = 'latest' }

$repoOwner = 'RyanMerlin'
$repoName = 'ayx-cli'
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

$tmpDir = Join-Path ([System.IO.Path]::GetTempPath()) ("ayx-install-" + [guid]::NewGuid().ToString())
New-Item -ItemType Directory -Force -Path $tmpDir | Out-Null

try {
  $archivePath = Join-Path $tmpDir $artifactName
  try {
    Invoke-WebRequest -Uri $downloadUrl -OutFile $archivePath
  } catch {
    throw "failed to download $downloadUrl. $($_.Exception.Message)"
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
}
finally {
  Remove-Item -Recurse -Force $tmpDir -ErrorAction SilentlyContinue
}
