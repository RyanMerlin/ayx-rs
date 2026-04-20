#!/usr/bin/env bash
set -euo pipefail

SOURCE_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PUBLIC_ROOT="${1:-$(cd "${SOURCE_ROOT}/.." && pwd)/ayx-cli}"

if [[ ! -d "${PUBLIC_ROOT}/.git" ]]; then
  echo "public repo path is not a git checkout: ${PUBLIC_ROOT}" >&2
  exit 1
fi

items=(
  "README.md"
  "docs/cli-spec.md"
  "scripts/install.sh"
  "scripts/install.ps1"
)

for item in "${items[@]}"; do
  mkdir -p "${PUBLIC_ROOT}/$(dirname "$item")"
  cp "${SOURCE_ROOT}/${item}" "${PUBLIC_ROOT}/${item}"
done

echo "synced public files to ${PUBLIC_ROOT}"
