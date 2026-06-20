# Changelog

## 0.9.10 — 2026-06-20

### Docs and release cleanup

- Move the public docs site to Astro/Starlight under `site/`.
- Remove stale dashboard and legacy docs references from the public docs surface.
- Rehome the runtime fixture under `docs/fixtures/RuntimeSettings.xml`.
- Keep the CLI spec and command-surface docs aligned with the live 0.9.10 binary.

### Verification

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets --locked -- -D warnings`
- `cargo test --workspace --locked`
- `cargo run -q -p xtask -- refresh-command-surface --check`

## 0.9.8 — 2026-06-16

### One hardening and release prep

- Tighten the One transport envelope so live requests, dry-runs, auth failures, and backend errors report a stable shape with request metadata.
- Add a table-driven live validation matrix for the wired One surface so the smoke suite proves real API reachability instead of just envelope construction.
- Standardize auth failure, permission failure, and transport failure classification so blocked environments are reported explicitly.
- Update the contributor and release docs to prefer `cargo nextest run` and align the public release checklist with the current CI matrix.

### Verification

- `cargo nextest run -p ayx-one-api --lib`
- `cargo nextest run -p ayx-rs --test one_live_smoke`

## 0.9.7 — 2026-06-15

### Progressive discovery

- Add a first-class `ayx discover` entry point for the live `clap` tree.
- Keep `catalog` as the machine-readable registry view while discovery becomes the progressive agent substrate.
- Regenerate the command-surface docs and smoke tests so the live binary, docs, and catalog stay aligned.

## 0.9.6 — 2026-06-15

### Workspace credential mapping

- Add workspace-scoped One API credentials and prefer them over legacy top-level tokens when a workspace is known.
- Resolve One refresh and auth status paths through the active workspace credential when present.
- Keep the release docs and smoke tests aligned with the `us1` API host and auth issuer split.

## 0.9.5 — 2026-06-15

### One API host and auth fixes

- Require an explicit `AYX_ONE_BASE_URL` for One API requests instead of inferring the API host from the token endpoint.
- Keep `AYX_ONE_TOKEN_ENDPOINT_URL` pointed at the auth issuer and normalize `/as` to `/as/token` when refreshing access tokens.
- Align the One platform workspace and role routes with the published v4 OpenAPI surface.
- Refresh the user and agent guidance in the sample config and docs so the API host and auth host are clearly separated.

### Verification

- `cargo nextest run -p ayx-core one_token_endpoint`
- `cargo nextest run -p ayx-one-api refresh_token_uses_refresh_token_only`
- `cargo nextest run -p ayx-rs`

## 0.9.4 — 2026-06-02

Completes the two breaking dependency upgrades that 0.9.3 deliberately deferred.
Both landed as isolated, reviewed changes and are green on Linux and macOS CI.

### Dependencies

- Migrate `keyring` 3.6 → 4.0. keyring 4.x moved the `Entry` API to
  `keyring-core` and split the platform credential stores into separate crates
  that are registered at runtime. ayx-core now depends on `keyring-core` plus a
  per-OS store (zbus Secret Service on Linux, native Keychain on macOS, native
  Credential Manager on Windows) and registers it once before first use. Also
  replaces a fragile error-string match with `Error::NoEntry` so not-found
  handling is correct under the new error type.
- Upgrade `axum` 0.7 → 0.8 and convert the dashboard router to the new
  path-capture syntax (`:id` → `{id}`, `*path` → `{*path}`); the old form is a
  router-build panic under 0.8, not a compile error.
- Drop the `keyring` / `axum` major-version ignore rules from `dependabot.yml`
  now that the deferred migrations are done, so future updates are tracked again.

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
- Switch GitHub Actions test jobs to `cargo nextest run` for faster, more consistent workspace validation.
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
- `cargo nextest run -p ayx-workflow -p ayx-server -p ayx-rs` passes (including `workflow_canary` and `one_live_smoke` integration tests).

## 0.7.0

See commit `162fb05`.
