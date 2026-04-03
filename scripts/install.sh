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

if [[ "$VERSION" == "latest" ]]; then
  DOWNLOAD_URL="https://github.com/${REPO_OWNER}/${REPO_NAME}/releases/latest/download/${BINARY_NAME}-${PLATFORM}.tar.gz"
else
  DOWNLOAD_URL="https://github.com/${REPO_OWNER}/${REPO_NAME}/releases/download/${VERSION}/${BINARY_NAME}-${PLATFORM}.tar.gz"
fi

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT
ARCHIVE="$TMPDIR/${BINARY_NAME}-${PLATFORM}.tar.gz"

echo "Downloading ${DOWNLOAD_URL}"
curl -fsSL "$DOWNLOAD_URL" -o "$ARCHIVE"

mkdir -p "$INSTALL_DIR"
tar -xzf "$ARCHIVE" -C "$INSTALL_DIR" "$BINARY_NAME"
chmod +x "$INSTALL_DIR/$BINARY_NAME"

echo "installed ${BINARY_NAME} to ${INSTALL_DIR}/${BINARY_NAME}"
echo "make sure ${INSTALL_DIR} is on your PATH"
