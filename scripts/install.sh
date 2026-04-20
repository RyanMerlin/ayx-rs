#!/usr/bin/env bash
set -euo pipefail

REPO_OWNER="RyanMerlin"
REPO_NAME="ayx-cli"
BINARY_NAME="ayx"
VERSION="${AYX_VERSION:-latest}"
INSTALL_DIR="${AYX_INSTALL_DIR:-$HOME/.local/bin}"

detect_platform() {
  local os arch
  os="$(uname -s)"
  arch="$(uname -m)"

  case "$os" in
    Linux) os_part="unknown-linux-gnu" ;;
    Darwin) os_part="apple-darwin" ;;
    MINGW*|MSYS*|CYGWIN*|Windows_NT)
      os_part="pc-windows-msvc"
      arch="${arch/x86_64/amd64}"
      ;;
    *)
      echo "unsupported OS: $os" >&2
      exit 1
      ;;
  esac

  case "$arch" in
    x86_64|amd64) arch_norm="x86_64" ;;
    aarch64|arm64) arch_norm="aarch64" ;;
    *)
      echo "unsupported architecture: $arch" >&2
      exit 1
      ;;
  esac

  echo "${arch_norm}-${os_part}"
}

PLATFORM="$(detect_platform)"

is_on_path() {
  local dir path_entry
  dir="$1"
  IFS=':' read -r -a path_entry <<< "${PATH:-}"
  for entry in "${path_entry[@]}"; do
    if [[ "$entry" == "$dir" ]]; then
      return 0
    fi
  done
  return 1
}

pick_install_dir() {
  local candidate
  local path_entries
  if [[ -n "${AYX_INSTALL_DIR:-}" ]]; then
    echo "$INSTALL_DIR"
    return
  fi

  IFS=':' read -r -a path_entries <<< "${PATH:-}"
  for candidate in "${path_entries[@]}"; do
    if [[ -n "$candidate" && -d "$candidate" && -w "$candidate" ]]; then
      echo "$candidate"
      return
    fi
  done

  for candidate in /usr/local/bin /usr/bin "${HOME}/.local/bin" "${HOME}/bin"; do
    if [[ -d "$candidate" && -w "$candidate" ]]; then
      echo "$candidate"
      return
    fi
  done

  echo "$INSTALL_DIR"
}

INSTALL_DIR="$(pick_install_dir)"

require_cmd() {
  local cmd="$1"
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "missing required command: $cmd" >&2
    exit 1
  fi
}

require_cmd curl
require_cmd tar

if [[ "$VERSION" == "latest" ]]; then
  DOWNLOAD_URL="https://github.com/${REPO_OWNER}/${REPO_NAME}/releases/latest/download/${BINARY_NAME}-${PLATFORM}.tar.gz"
else
  DOWNLOAD_URL="https://github.com/${REPO_OWNER}/${REPO_NAME}/releases/download/${VERSION}/${BINARY_NAME}-${PLATFORM}.tar.gz"
fi

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT
ARCHIVE="$TMPDIR/${BINARY_NAME}-${PLATFORM}.tar.gz"

echo "Downloading ${DOWNLOAD_URL}"
if ! curl -fsSL "$DOWNLOAD_URL" -o "$ARCHIVE"; then
  echo "failed to download ${DOWNLOAD_URL}" >&2
  exit 1
fi

mkdir -p "$INSTALL_DIR"
EXTRACT_DIR="$TMPDIR/extract"
mkdir -p "$EXTRACT_DIR"
if ! tar -xzf "$ARCHIVE" -C "$EXTRACT_DIR"; then
  echo "failed to extract ${DOWNLOAD_URL}" >&2
  echo "archive contents:" >&2
  tar -tzf "$ARCHIVE" >&2 || true
  exit 1
fi

if [[ -f "$EXTRACT_DIR/$BINARY_NAME" ]]; then
  cp "$EXTRACT_DIR/$BINARY_NAME" "$INSTALL_DIR/$BINARY_NAME"
else
  BINARY_PATH="$(find "$EXTRACT_DIR" -type f -name "$BINARY_NAME" | head -n 1)"
  if [[ -z "${BINARY_PATH:-}" ]]; then
    echo "downloaded archive did not contain ${BINARY_NAME}" >&2
    echo "archive contents:" >&2
    find "$EXTRACT_DIR" -maxdepth 2 -print >&2 || true
    exit 1
  fi
  cp "$BINARY_PATH" "$INSTALL_DIR/$BINARY_NAME"
fi

chmod +x "$INSTALL_DIR/$BINARY_NAME"

echo "installed ${BINARY_NAME} to ${INSTALL_DIR}/${BINARY_NAME}"
if is_on_path "$INSTALL_DIR"; then
  echo "${INSTALL_DIR} is already on your PATH"
else
  echo "make sure ${INSTALL_DIR} is on your PATH"
  echo "for this shell: export PATH=\"${INSTALL_DIR}:\$PATH\""
  if [[ -w "${HOME}" ]]; then
    PROFILE_FILE="${HOME}/.profile"
    if ! grep -qsF "export PATH=\"${INSTALL_DIR}:\$PATH\"" "$PROFILE_FILE" 2>/dev/null; then
      printf '\nexport PATH="%s:$PATH"\n' "$INSTALL_DIR" >> "$PROFILE_FILE"
      echo "added PATH export to ${PROFILE_FILE} for future shells"
    fi
  fi
fi
