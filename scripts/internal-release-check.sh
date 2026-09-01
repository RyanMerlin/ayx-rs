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

workspace_version="$(grep -m1 '^version = ' Cargo.toml | sed -E 's/^version = "([^"]+)"/\1/')"
if [[ -z "$workspace_version" ]]; then
  echo "unable to find workspace version in Cargo.toml" >&2
  exit 1
fi
release_notes_name="v${workspace_version}-internal.1.md"
release_notes="docs/releases/${release_notes_name}"
if [[ ! -f "$release_notes" ]]; then
  echo "release notes not found: $release_notes -- create it before cutting an internal release" >&2
  exit 1
fi

dist="$repo_dir/dist/internal"
stage="$dist/ayx-x86_64-unknown-linux-gnu"
archive="$dist/ayx-x86_64-unknown-linux-gnu-internal.tar.gz"
mkdir -p "$dist"
rm -rf "$stage"
mkdir -p "$stage"
cp target/release/ayx "$stage/ayx"
cp README.md "$release_notes" "$stage/"
tar -czf "$archive" -C "$stage" ayx README.md "$release_notes_name"

verify="$(mktemp -d "$dist/archive-smoke-linux.XXXXXX")"
tar -xzf "$archive" -C "$verify"
test -x "$verify/ayx"
"$verify/ayx" --version
"$verify/ayx" --help >/dev/null
rm -rf "$verify"

echo "Internal WSL2/Linux artifact: $archive"
