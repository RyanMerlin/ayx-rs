param(
  [string]$Version = $env:AYX_VERSION,
  [string]$InstallDir = $env:AYX_INSTALL_DIR
)

$ErrorActionPreference = 'Stop'

if (-not $Version) { $Version = 'latest' }
if (-not $InstallDir) { $InstallDir = Join-Path $HOME '.local\bin' }

$repoOwner = 'RyanMerlin'
$repoName = 'ayx-cli'
$artifactName = 'ayx-x86_64-pc-windows-msvc.zip'

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
  Write-Host "make sure $InstallDir is on your PATH"
}
finally {
  Remove-Item -Recurse -Force $tmpDir -ErrorAction SilentlyContinue
}
