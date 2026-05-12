#!/usr/bin/env bash
set -euo pipefail

REPO_OWNER="RyanMerlin"
REPO_NAME="ayx-rs"
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
  if [[ -n "${AYX_INSTALL_DIR:-}" ]]; then
    echo "$INSTALL_DIR"
    return
  fi

  local candidate
  local parent

  for candidate in "${HOME}/.local/bin" "${HOME}/bin" /usr/local/bin /usr/bin; do
    parent="$(dirname "$candidate")"
    if [[ -d "$candidate" && -w "$candidate" ]]; then
      echo "$candidate"
      return
    fi
    if [[ -d "$parent" && -w "$parent" ]]; then
      mkdir -p "$candidate" 2>/dev/null || true
      if [[ -d "$candidate" && -w "$candidate" ]]; then
        echo "$candidate"
        return
      fi
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
require_cmd sha256sum 2>/dev/null || require_cmd shasum

if [[ "$VERSION" == "latest" ]]; then
  DOWNLOAD_URL="https://github.com/${REPO_OWNER}/${REPO_NAME}/releases/latest/download/${BINARY_NAME}-${PLATFORM}.tar.gz"
  SUMS_URL="https://github.com/${REPO_OWNER}/${REPO_NAME}/releases/latest/download/SHA256SUMS"
else
  DOWNLOAD_URL="https://github.com/${REPO_OWNER}/${REPO_NAME}/releases/download/${VERSION}/${BINARY_NAME}-${PLATFORM}.tar.gz"
  SUMS_URL="https://github.com/${REPO_OWNER}/${REPO_NAME}/releases/download/${VERSION}/SHA256SUMS"
fi

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT
ARCHIVE="$TMPDIR/${BINARY_NAME}-${PLATFORM}.tar.gz"
SUMS="$TMPDIR/SHA256SUMS"

echo "Downloading ${DOWNLOAD_URL}"
if ! curl -fsSL "$DOWNLOAD_URL" -o "$ARCHIVE"; then
  echo "failed to download ${DOWNLOAD_URL}" >&2
  exit 1
fi

# Verify integrity against SHA256SUMS published alongside the release.
# Operators can opt out with AYX_SKIP_CHECKSUM=1 only for explicit reasons
# (air-gapped mirror, etc.); the default is to verify.
if [[ "${AYX_SKIP_CHECKSUM:-0}" != "1" ]]; then
  echo "Fetching SHA256SUMS for verification"
  if curl -fsSL "$SUMS_URL" -o "$SUMS"; then
    archive_basename="$(basename "$ARCHIVE")"
    expected="$(awk -v f="$archive_basename" '$2 == f { print $1 }' "$SUMS" | head -n1)"
    if [[ -z "$expected" ]]; then
      echo "SHA256SUMS does not contain an entry for ${archive_basename}; aborting." >&2
      echo "Set AYX_SKIP_CHECKSUM=1 to bypass (not recommended)." >&2
      exit 1
    fi
    if command -v sha256sum >/dev/null 2>&1; then
      actual="$(sha256sum "$ARCHIVE" | awk '{print $1}')"
    else
      actual="$(shasum -a 256 "$ARCHIVE" | awk '{print $1}')"
    fi
    if [[ "$expected" != "$actual" ]]; then
      echo "checksum mismatch: expected $expected got $actual" >&2
      echo "Refusing to install a corrupted or tampered archive." >&2
      exit 1
    fi
    echo "Checksum verified: $actual"
  else
    echo "WARNING: could not fetch SHA256SUMS from ${SUMS_URL}." >&2
    echo "Set AYX_SKIP_CHECKSUM=1 to install anyway (NOT recommended)." >&2
    exit 1
  fi
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

# ---------------------------------------------------------------------------
# Optional: install shell completions. Best-effort — failure is non-fatal.
# Skip entirely when AYX_SKIP_COMPLETIONS=1 (CI / locked-down hosts).
# ---------------------------------------------------------------------------
if [[ "${AYX_SKIP_COMPLETIONS:-0}" != "1" ]]; then
  shell_name="$(basename "${SHELL:-bash}")"
  case "$shell_name" in
    bash)
      candidates=("${HOME}/.local/share/bash-completion/completions" "/usr/local/etc/bash_completion.d" "/etc/bash_completion.d")
      for d in "${candidates[@]}"; do
        if [[ -d "$d" && -w "$d" ]]; then
          if "${INSTALL_DIR}/${BINARY_NAME}" completions bash > "${d}/${BINARY_NAME}" 2>/dev/null; then
            echo "installed bash completions to ${d}/${BINARY_NAME}"
            break
          fi
        fi
      done
      ;;
    zsh)
      # zsh: drop into $fpath; ~/.zfunc is a common writable choice.
      d="${HOME}/.zfunc"
      mkdir -p "$d" 2>/dev/null || true
      if "${INSTALL_DIR}/${BINARY_NAME}" completions zsh > "${d}/_${BINARY_NAME}" 2>/dev/null; then
        echo "installed zsh completions to ${d}/_${BINARY_NAME}"
        echo "ensure 'fpath=(${d} \$fpath)' and 'autoload -Uz compinit && compinit' are in your .zshrc"
      fi
      ;;
    fish)
      d="${HOME}/.config/fish/completions"
      mkdir -p "$d" 2>/dev/null || true
      if "${INSTALL_DIR}/${BINARY_NAME}" completions fish > "${d}/${BINARY_NAME}.fish" 2>/dev/null; then
        echo "installed fish completions to ${d}/${BINARY_NAME}.fish"
      fi
      ;;
    *)
      echo "shell '${shell_name}' completions not auto-installed."
      echo "Run: ${BINARY_NAME} completions <shell> > <your completions dir>"
      ;;
  esac
fi
