# Changelog

## 0.9.3 — 2026-06-01

First complete release since 0.9.1. The 0.9.2 tag never published artifacts
because its release build failed on the Windows job; this release drops Windows
to ship cleanly on Linux and macOS.

### Platform support

- Drop Windows from CI and the release pipeline. Tests run on Linux and macOS;
  release artifacts are `x86_64-unknown-linux-gnu` and the two macOS targets.
- Fix the Windows-only `cli_smoke` build break that triggered this (the
  `std::fs` import is now gated to match its `#[cfg(not(windows))]` usage),
  kept for correctness even though Windows is no longer built.

### Dependencies

- Defer the breaking `keyring` 4.x and `axum` 0.8.x upgrades; stay on the
  latest 3.x / 0.7.x (both `cargo-audit` clean) and add `dependabot.yml` ignore
  rules so the breaking majors stop being re-proposed.

## 0.9.1 — 2026-05-29

### CI and release fixes

- Pull in the current `cargo-audit` ignore set and lockfile refresh so CI matches the upstream passing dependency state.
- Switch GitHub Actions test jobs from `cargo test` to `cargo nextest run` for faster, more consistent workspace validation.
- Replace the broken GitHub Actions lint action, opt workflows into the Node.js 24 runtime early, and keep shell globs actionlint-safe.
- Fix release signing secret scoping so Windows/macOS signing and notarization steps can actually run when secrets are present.
- Make SBOM collection deterministic for the current `cargo-cyclonedx` output layout and fail the SBOM job if no JSON files are produced.

### CLI maintenance

- Preserve catalog coverage after the `main.rs` refactor by restoring the stronger catalog describe/tag-filter unit tests.
- Add workspace summary parsing coverage in `ayx-one-api` for list responses that use `workspaceName`, `workspace_id`, and related aliases.
- Correct the local source install command in the README to point at the real binary crate path (`ayx-rs/`).

## 0.9.0 — 2026-05-27

### Dependency modernization

- Bump workspace dependencies to current patch releases: `clap_complete`, `openssl`, `openssl-sys`, `reqwest`, `rustls-webpki`, `serde_json`, and `tower-http`.
- Add the direct `base64` dependency in `ayx-rs` so the dashboard/server code uses the workspace-managed crate version.

### Workflow and runtime hardening

- Keep YXDB handling flexible while making workflow parsing safer and more explicit about malformed inputs.
- Preserve structured failure handling in `ayx-rs` and keep dashboard password handling from mutating opaque secrets.

### Release and docs cleanup

- Refresh the public release docs, install scripts, CI release workflow, and release checklist to match the current repository shape.
- Update the changelog and package version so tag-based CI can publish the next release line cleanly.

### Dependency upgrades

- `reqwest` 0.12 → 0.13 (workspace). Feature set updated to `rustls` (was `rustls-tls`) and adds `form`.
- `zip` 0.6 → 8 (workspace). `ayx-server` and `ayx-workflow` now consume the workspace pin instead of pinning locally.
- `sha2` 0.10 → 0.11 (workspace).
- `self_update` 0.43.1 → 0.44.0 (`ayx-rs`).

### Code changes required by the upgrades

- `zip`: `FileOptions` → `SimpleFileOptions` in `ayx-server/src/upgrade/service.rs` and `ayx-workflow/src/lib.rs`.
- `sha2`: `format!("{:x}", hasher.finalize())` no longer compiles because the new `Array<u8, ...>` return type does not implement `LowerHex`. Switched `ayx-workflow/src/cloud_convert.rs::checksum` to the same byte-iter idiom already used in `ayx-server/src/upgrade/manifest.rs::compute_sha256`.

### `ayx onboard` fixes

- Skip the storage backend section entirely when "Configure Alteryx Server" is N. Previously the RuntimeSettings.xml, AlteryxService.exe, and Mongo restore-target prompts ran regardless of the server answer.
- Drop the "Designer user install" yes/no prompt. The service detector now always probes `%LOCALAPPDATA%\Alteryx\bin` in addition to `C:\Program Files\Alteryx\bin`, so per-user Designer installs are picked up without asking.
- Drop the "Embedded Mongo restore target path" prompt. The value is resolved at restore time from `RuntimeSettings.xml` (`ayx-server/src/mongo.rs::resolve_embedded_restore_target_path`); existing profile values are preserved.

### Verification

- `cargo build --workspace` clean.
- `cargo test -p ayx-workflow -p ayx-server -p ayx-rs` passes (including `workflow_canary` and `one_live_smoke` integration tests).

## 0.7.0

See commit `162fb05`.
