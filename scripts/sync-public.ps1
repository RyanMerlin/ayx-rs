param(
  [string]$PublicRepoPath = (Join-Path (Split-Path -Parent $PSScriptRoot) '..\ayx-cli')
)

$ErrorActionPreference = 'Stop'

$sourceRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$publicRoot = (Resolve-Path $PublicRepoPath).Path

if (-not (Test-Path (Join-Path $publicRoot '.git'))) {
  throw "public repo path is not a git checkout: $publicRoot"
}

$items = @(
  'README.md',
  'docs\cli-spec.md',
  'scripts\install.sh',
  'scripts\install.ps1'
)

foreach ($item in $items) {
  $source = Join-Path $sourceRoot $item
  $destination = Join-Path $publicRoot $item
  $destinationDir = Split-Path -Parent $destination

  New-Item -ItemType Directory -Force -Path $destinationDir | Out-Null
  Copy-Item $source -Destination $destination -Force
}

Write-Host "synced public files to $publicRoot"
