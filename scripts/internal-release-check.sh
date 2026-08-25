#!/usr/bin/env bash
set -euo pipefail

repo_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_dir"

if [[ "${AYX_INTERNAL_RELEASE_ALLOW_NON_WSL2:-}" != "1" \
    && "$(uname -r)" != *microsoft-standard* ]]; then
  echo "Run this check inside WSL2 Ubuntu (or set AYX_INTERNAL_RELEASE_ALLOW_NON_WSL2=1 for CI packaging)." >&2
  exit 1
fi

if ! command -v cargo-nextest >/dev/null 2>&1; then
  echo "cargo-nextest is required; install it before running the internal release check." >&2
  exit 1
fi

run_checked() {
  echo "> $*"
  "$@"
}

run_checked cargo fmt --all --check
run_checked cargo run -q -p xtask -- refresh-command-surface --check
run_checked cargo clippy --workspace --all-targets --locked -- -D warnings
run_checked cargo nextest run --workspace --locked
run_checked cargo build --workspace --release --locked
if [[ "${AYX_INTERNAL_RELEASE_SKIP_AUDIT:-}" != "1" ]]; then
  run_checked cargo audit --deny warnings
fi

dist="$repo_dir/dist/internal"
stage="$dist/ayx-x86_64-unknown-linux-gnu"
archive="$dist/ayx-x86_64-unknown-linux-gnu-internal.tar.gz"
mkdir -p "$dist"
rm -rf "$stage"
mkdir -p "$stage"
cp target/release/ayx "$stage/ayx"
cp README.md scripts/install.sh docs/releases/v0.17.0-internal.1.md "$stage/"
tar -czf "$archive" -C "$stage" ayx README.md install.sh v0.17.0-internal.1.md

echo "Internal WSL2/Linux artifact: $archive"
