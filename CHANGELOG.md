# Changelog

## Unreleased

### Fixed

- **`ayx update` failed to extract release archives on every platform.**
  `self_update` was pulled with default features only, which include no archive
  backend, so self-update aborted with `ArchiveNotEnabled` — `.zip` on Windows
  (`Archive extension 'zip' not supported`) and equally `.tar.gz` on
  Linux/macOS. Enable `archive-tar` + `compression-flate2` (for the `.tar.gz`
  assets) and `archive-zip` + `compression-zip-deflate` (for the Windows
  `Compress-Archive` `.zip`). Note: upgrading *into* the first build that
  carries this fix still needs a one-time manual download, since the currently
  installed binary is the one that can't extract.

### Dependencies

- **`ayx-one-api`: `getrandom` 0.2 → 0.4.** The two CSPRNG helpers
  (`generate_pkce_challenge`, `generate_random_state`) move from the
  0.3-removed `getrandom::getrandom` free function to `getrandom::fill`.
  Behavior is unchanged — same OS entropy source, same fail-on-no-entropy
  contract — and new characterization tests lock the 256-bit verifier, the S256
  challenge relationship, and non-repetition. `ayx-one-api` now shares the
  `getrandom` 0.4.2 already pulled by `tempfile`; `getrandom` 0.2 stays in the
  tree only as a transitive dep of `secret-service`. Supersedes dependabot #57.

## 0.12.0 — 2026-07-07

### Added

- **Seamless Alteryx One first-run onboarding** (`#74`): `ayx onboard` parses a
  pasted workspace URL for its region and workspace gid and offers to run the
  email-OTP login immediately, so the wizard ends with you connected. Includes
  profile-split fixes so the onboarded profile is the one `auth login` writes its
  token into.
- **`ayx one datasets`** (`#82`): read the Alteryx One dataset library — `list`,
  `count`, plus `wrangled` (list/count/detail) and `imported` (detail).
- **`ayx one api`** (`#86`): One OpenAPI-spec introspection. `coverage` diffs the
  live spec against the wired-command inventory to surface gaps; plus `status`,
  `diagnose`, and `open-api-spec`.
- **Visual interface browser (TUI v2)** (`#68`, `#69`): a k9s-style resource
  browser (all five asset kinds, drill/filter/switch), a `Ctrl+K` command
  palette, `?` help, and inline editing, behind `AYX_TUI_V2=1 ayx tui`.
- **Install shadow warning** (`#67`): the installer warns when a different `ayx`
  earlier on `PATH` would shadow the freshly installed binary (Windows).

### Fixed

- **Onboarding yes/no defaults** (`#87`): prompts now honor the `[Y/n]` / `[y/N]`
  default they display. Pressing Enter at "Configure Alteryx Server" (shown as
  `[y/N]` on a fresh One onboard) correctly skips Server configuration instead of
  silently entering it and writing an empty server section.
- **Windows** (`#84`, `#85`): reserve a 16 MiB main-thread stack and enable the
  Windows `cli_smoke` job; remove the redundant command-dispatch worker thread.
- **Keyring test isolation** (`#81`): keyring tests use an in-memory mock store,
  so they no longer read or write the host OS keyring.

### Changed

- **`one ui` is gated behind a default-off cargo feature** (`#80`): the
  experimental visual-interface subtree is absent from the shipped binary.
- **Docs**: onboarding getting-started/connecting/configuration rewritten to the
  OTP flow (`#75`); One command descriptions backfilled (`#83`); command-surface
  coverage gaps captured (`#76`); README command tree reconciled with the shipped
  surface — `one ui` removed, `one api` and `one datasets` added (`#87`).

### Dependencies

- Bump `cmov` 0.5.3 → 0.5.4 (`#79`), `tui-input` 0.11.1 → 0.15.3 (`#73`), and
  `clap_complete`, `anyhow`, and `taiki-e/install-action` (`#71`, `#72`, `#77`,
  `#78`).

## 0.11.2 — 2026-06-27

### Fixed

- **Windows release asset** (`#63`): `scripts/install.ps1` downloads
  `ayx-x86_64-pc-windows-msvc.zip`, but the release workflow had no Windows build
  job, so every prior release was missing that asset and the PowerShell
  quick-start failed with a 404. Added a hardened `build-windows` job and wired
  it into the release pipeline (SHA256SUMS, Sigstore signing, SLSA attestation).
  This is the first release to publish a Windows binary.

### Added

- **Visual interface preview (TUI v2)** (`#62`): a new resource-browser TUI spine
  (The Elm Architecture + a `ResourceKind` registry) is available behind
  `AYX_TUI_V2=1 ayx tui`, currently browsing Alteryx One flows end-to-end. The
  existing `ayx tui` is unchanged without the flag. Foundations for a forthcoming
  workspace/asset browser.

## 0.11.1 — 2026-06-23

### Added

- `ayx secret prune` — removes keyring accounts orphaned by the v0.11.0
  profile_name to file-stem scope migration.  Dry-run by default; `--apply`
  to delete.  Targets the deterministic set of accounts writable by
  `secretize_config`; never enumerates the full keyring.  See
  [docs/releases/v0.11.1.md](docs/releases/v0.11.1.md).

## 0.11.0 — 2026-06-23

### Breaking changes

- **On-disk format** (`#50`, `#51`): the canonical config format now uses `client_secret_ref` /
  `curator_api_secret_ref` to store secrets indirectly (keyring or env references).
  Config files written by v0.11.0 are not readable by older binaries that lack
  the `_ref` fields. Existing plaintext configs load fine on upgrade; the ref is
  written on the next save (lazy migration).

### Features

- **Server-API secret consolidation** (`#51`): a single canonical source
  (`server_api.client_secret`) is now the authoritative secret for Alteryx Server
  connectivity. The legacy `api.auth.client_secret` and `server.curator_api_secret`
  fields are synthesized (derived, read-only) views of the same secret; writing to
  them is a no-op when they carry the same value as `server_api`. A mixed-state
  conflict (two representations resolving to different values) is detected at the
  write boundary and reported with field names and ref forms — never the resolved
  secret value itself.

### Migration notes

- **Keyring accounts re-key on next save** (lazy migration): the keyring account
  name now uses the on-disk file stem (standalone profiles) or `workspace.env`
  (workspace environments) as the stable scope, rather than the mutable
  `profile_name` field. After the first save, the old account (if any) may remain
  in your keyring; it is harmless and can be pruned with `ayx secret prune`
  (tracked in issue #4).

## 0.10.3 — 2026-06-22

### Security (dependencies)

- Bumped `quinn-proto` 0.11.14 → 0.11.15 to clear **RUSTSEC-2026-0185** (remote memory exhaustion / DoS via unbounded out-of-order stream reassembly), published the same day. `quinn-proto` is a transitive HTTP/3 QUIC dependency not on the CLI's HTTP/1.1 request path, so this is a `Cargo.lock`-only change with no behavior impact — but it restores a green `cargo audit` gate.

## 0.10.2 — 2026-06-22

### Security

- **Redirect-host allowlist** on the auth flow. The OTP→OIDC redirect follower now refuses to follow a `Location` to any host outside the base domain and its subdomains (e.g. `us1.alteryxcloud.com` allows `pingauth.alteryxcloud.com` but rejects `evil.com` and `alteryxcloud.com.evil.com`). An off-domain redirect is never requested, so no cookies are sent off-domain. (red-team M2)
- **Interaction-id shape validation.** The OIDC interaction id pulled from the redirect chain is now bounds-checked (6–128 chars, restricted charset) before use. (red-team M3)
- **Redacted two more raw response bodies** (`validatePasscode`, `/v4/auth/accounts` error paths) that previously interpolated unredacted bodies into errors — same leak class fixed earlier in the preflight path.

### Robustness

- Removed a latent `unwrap()` in the `auth diagnose` envelope builder (safe-by-construction today, but a footgun if the control flow changed).

### Tests

- 18 new unit tests for the redirect-host allowlist and interaction-id validation (306 total).

## 0.10.1 — 2026-06-22

### Removed

- Dropped the Playwright/headless-Chromium fallback from the Alteryx One first-login flow. The pure-HTTP reqwest flow (proven through v0.10.0) is now the only path — no `python3`, `playwright`, or `chromium` dependency, and no `AYX_ONE_AUTH_FORCE_BROWSER` / `AYX_ONE_AUTH_NO_FALLBACK` env vars. This removes ~505 lines (including an embedded Python script), drops the unused `tempfile` dependency, and resolves the red-team M4 finding (the workspace password was passed to the subprocess via an env var). The full browser implementation remains in git history if Alteryx ever changes their OIDC flow.
- Removed dead helpers orphaned by the earlier pure-HTTP refactor (`random_hex`, `wait_for_file`).

## 0.10.0 — 2026-06-22

Alteryx One authentication GA. This milestone follows a security and correctness
red-team of the auth flow and the API surface work; all blocking findings are fixed
and covered by tests (288 total, up from 255).

### Security

- `auth login` now warns when a 30-day PAT is stored inline (plaintext YAML) because the OS keyring is unavailable — previously silent on headless hosts. The inline-secret warning is shared with the onboarding path.
- Workspace preflight errors now redact the response body preview, matching the sibling parse-failure branch. No more raw response bodies (which can echo tokens/cookies) in error chains.
- The secret redactor now masks the field names this auth flow actually produces (`tokenValue`, `local-auth-workspace`, `x-csrf-token`, `passcode`, `passcodeReferenceId`, `secret`) plus bare JWT-shaped tokens (`eyJ…`).

### Workspace model

- The Alteryx One PAT is workspace-bound — the `x-alteryx-workspace-gid` header is ignored server-side; the token alone determines the workspace. The CLI now reflects this:
  - `workspace people` and `workspace admins` are argless (the old required `--workspace-id` was silently ignored and could imply the wrong workspace).
  - New `workspace switch --workspace-id <id>` selects an already-authenticated workspace credential instantly; if you have not logged into that workspace, it tells you to `auth login`.
  - `workspace invite-users` and the other membership mutations now reject an explicit `--workspace-id` that does not match the active workspace, instead of letting the path and the token diverge on a destructive operation.

### Correctness

- `connections connector-metadata template`: the connection-`type` heuristic now emits a `<jdbc|remotefile|…>` placeholder (with a `_note`) when it cannot confidently infer the type, instead of silently defaulting to `remotefile` for every non-relational connector.
- `job-groups list`: synthesized names now disambiguate multiple runs of one flow (`flow-{flowId} ({id})` / `flow-{flowId} @ {createdAt}`) instead of collapsing to a single `flow-{flowId}`.
- `apply_env_fallbacks`: restored uniform gap-fill precedence (env fills only an absent profile value) for `base_url`, `oauth_client_id`, `client_secret`, `token_endpoint_url`, matching the documented "last-resort fallback" contract.

### Tests

- 33 new deterministic tests: panic-regression guards for the four `--output-file` commands, plus unit coverage for the job-group name synthesizer, the connection-template builder, `resolve_workspace_id`, and the One-only-profile guard.

## 0.9.14 — 2026-06-22

### Bug fixes

- Fixed runtime panics in `flows export`, `server system-info`, `server runtime-settings`, and `tools workspace init`. Each defined a local `--output` (file path) arg that collided with the global `--output <text|json>` format flag, panicking on every invocation. The file arg is now `--output-file` on all four. `flows export` now exports a `.yxzp` package end-to-end.

### One API additions

- `connections connector-metadata template --connector <slug>`: generates a fillable JSON create-body template from connector metadata (derives `type`, `vendor`, `credentialType`, and a `params` skeleton). Unblocks `connections create` body construction.

### Documentation

- `docs/one-live-validation.md`: full per-endpoint live-verified status table — working surfaces, PAT-scope-blocked surfaces, absent routes, and tier-gated surfaces.

## 0.9.13 — 2026-06-22

### One API additions

- `flows permissions-get --flow-id <ID>`: read command for `GET /v4/flows/{id}/permissions`. Returns a clean `permission_denied` error (the endpoint is 403 under the current PAT scope) rather than a missing-command error. The existing `flows permissions` (POST, set permissions) is unchanged.
- `job-groups list`: synthesizes a display name (`flow-{flowId}`, falling back to `job-{id}`) when the API returns a null name, so flow-run job-groups are intelligible in text output.

## 0.9.12 — 2026-06-22

### One API endpoint fixes

- `flows update`: switched from `PUT /v4/flows/{id}` (returned 403) to `PATCH /v4/flows/{id}` (returns 200). Full CRUD on flows now works.
- `workspace people`: switched from `GET /v4/workspaces/{id}/people` (404) to `GET /v4/people` (200).
- `workspace admins`: switched from `GET /v4/workspaces/{id}/admins` (404) to `GET /v4/people?role=admin` (200).

### CLI ergonomics

- `--body <FILE>`: all 32 body mutation args now accept a path to a JSON file (previously the help text was ambiguous). Pass a file path or use `-` for stdin.
- `ayx one status` and `ayx one inventory` on One-only profiles: clean message instead of an internal config error.
- `platform workspace invite-users` and related membership commands: `--workspace-id` is now optional and defaults from the profile's `workspace_gid`.

### Documentation

- Billing, plans, and scheduling help text notes enterprise-tier requirement; commands return 404 on `platform_packaging` workspaces.
- `connector-metadata`: help text documents that connector enumeration (`list`) is not available via the v4 API; no `/v4/connectors` endpoint exists.

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
