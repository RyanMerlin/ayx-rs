---
title: Releases
description: Versioned release notes for ayx.
sidebar:
  order: 0
---

Release notes for each tagged version of `ayx`. For current behavior, use the live docs above; for a specific binary, read the notes for that version.

## v0.16.0

**Wizard authentication is now the default.** `ayx one login` uses the
binding-aware orchestration and secure persistence path by default; set
`AYX_AUTH_ROLLOUT=legacy` for the explicit rollback adapter, or use the
isolated `canary` rollout for validation. Regional One hosts remain explicit,
and the release documents session-only and consent-based plaintext fallback
options for constrained environments.

**The Alteryx One surface remains honestly measured.** The current live
baseline is 63.1% of comparable OpenAPI operations, with remaining gaps
documented as roadmap work. Corrected routes, dataset filters, workflow
pagination, mutation dry-run protection, and error classification are included
from the v0.15.0 development cycle.

## v0.15.0

**`ayx one workflows` adds a real CLI surface for Alteryx One cloud-native canvas workflows.** The ULID-keyed `/svc-workflow` family now supports list, count, assets, detail, dependencies, engines, tools, copy, and share; it is distinct from the integer-id-keyed Designer Cloud `one flows` family. Detail and count identify their client-side synthesis, share documents its recovered request shape, and arbitrary workflow authoring remains out of scope.

**BREAKING: `ayx one api coverage --output json` changed shape.** `stale[].command` is now `stale[].commands`, `coverage_pct` is nullable, and `inventory_total` plus `outside_spec_namespace` report endpoints outside the `/v4` spec namespace.

**`one workflows list --all` no longer silently under-delivers, and `actions export --output json` is valid JSON again.** `--all` now reports `complete: true`/`false` against the endpoint's own item count rather than assuming one page is everything; `actions export`'s YAML moved into the JSON envelope instead of printing ahead of it.

## v0.14.0

**Actions and workflows now have validated machine-readable I/O contracts.** Declared input/output schemas are checked at load and run time, before steps execute and after results complete; schema introspection reports whether a contract was declared or inferred.

**BREAKING: the tactics/action and workflow command renames are complete.** `ayx tactics` is now `ayx actions`, `ayx workflows` is now `ayx actions workflows`, and `ayx workflow` is now `ayx designer workflow`. JSON keys, registry filenames/directories, YAML `kind` and list keys, and the registry schema version all changed; there are no compatibility aliases.

**Mongo mutations and undo now execute only through bounded, approval-backed templates.** Preview/apply audit artifacts, backup and approval evidence, and guarded undo checks are required for live mutation.

## v0.13.2

**Windows live commands no longer abort after successful output.** The cached HTTP client lifetime is safe at process exit, and sensitive profile/onboarding/audit writes are atomic and lock-protected. Workspace-password prompts are masked, and the two One flow list views now explain their distinction.

## v0.13.1

**Bare command groups now render real clap help, and every `one` command has a useful description.**

## v0.13.0

**BREAKING: the `ayx one` hierarchy is now primitive-first and required resource ids are positional.** The `platform` namespace was dissolved, login/auth/identity paths were renamed, and commands such as `one flows detail <id>` replace required `--<noun>-id` flags; no pre-release compatibility aliases exist.

**Command help, output contracts, and release documentation were hardened.** Stable command families gained descriptions, error JSON no longer gets stray text, output values are constrained, and the project now carries its independent-status disclaimer.

## v0.12.2

**`ayx update` still failed after v0.12.1 on Linux and macOS.** The `.tar.gz` release archive named every member `./ayx` (from `tar -C "$root" .`), but `self_update` matches archive entries against the exact path `ayx`, so `./ayx != ayx` and self-update failed with `Could not find the required path in the archive: "ayx"`. Archive members are now packaged explicitly so the binary sits at `ayx`. Windows was already fixed by v0.12.1 — its `.zip` stores `ayx.exe` at the root.

## v0.12.1

**`ayx update` failed to extract release archives on every platform.** `self_update` was pulled with default features only, which ship no archive backend, so self-update aborted with `ArchiveNotEnabled` — `Archive extension 'zip' not supported` on Windows, and the equivalent failure for `.tar.gz` on Linux/macOS. Enabled `archive-tar` + `compression-flate2` (for `.tar.gz`) and `archive-zip` + `compression-zip-deflate` (for the Windows `.zip`) to restore both formats. Note: upgrading *into* this fix from an older binary still needs one manual download, since the currently installed binary is the one that can't extract.

Dependency: `ayx-one-api`'s `getrandom` moved from 0.2 to 0.4 (the PKCE-challenge and OAuth-state CSPRNG helpers now use `getrandom::fill`; same OS entropy source, no behavior change).

## v0.12.0

**New command surfaces and a smoother first run.**

- **Seamless Alteryx One onboarding.** `ayx onboard` parses a pasted workspace URL for its region and gid and offers to log in over email OTP immediately, so the wizard finishes connected. It also fixes a profile-split where the onboarded profile and the `auth login` token target could diverge.
- **Yes/no prompt fix.** Onboarding prompts now honor the `[Y/n]` / `[y/N]` default they display — pressing Enter at "Configure Alteryx Server" (shown `[y/N]` on a fresh One onboard) skips Server config instead of silently entering it.
- **`ayx one datasets`.** Read the One dataset library: `list`, `count`, `wrangled` (list/count/detail), and `imported` (detail).
- **`ayx one api`.** OpenAPI-spec introspection, including `coverage` to diff the live spec against the wired command inventory.
- **Visual interface browser (TUI v2).** A k9s-style resource browser with a `Ctrl+K` command palette and `?` help, behind `AYX_TUI_V2=1 ayx tui`.
- **`one ui` gated off.** The experimental visual-interface command subtree is now behind a default-off cargo feature and is absent from the shipped binary.
- **Windows.** A reserved 16 MiB main-thread stack and the Windows `cli_smoke` job; the redundant command-dispatch worker thread was removed.

## v0.11.2

**Windows release binary + TUI v2 preview.** The release pipeline now builds and publishes a signed Windows binary; the PowerShell quick-start previously 404'd because no Windows asset was shipped. A resource-browser TUI spine is available behind `AYX_TUI_V2=1 ayx tui`.

## v0.11.1

**`ayx secret prune`.** Removes keyring accounts orphaned by the v0.11.0 profile-name → file-stem scope migration. Dry-run by default; `--apply` to delete.

## v0.11.0

**Indirect secret storage (breaking on-disk format).** Config now stores secrets by reference (`client_secret_ref` / `curator_api_secret_ref`) in the OS keyring or an `env:` ref rather than inline. Existing plaintext configs load fine and migrate on the next save; older binaries cannot read the new `_ref` fields.

## v0.10.3

**Dependency security bump.** `quinn-proto` was bumped from 0.11.14 to 0.11.15 to clear **RUSTSEC-2026-0185**, a remote memory-exhaustion / DoS advisory in a transitive HTTP/3 QUIC dependency. `quinn-proto` is not on the CLI's HTTP/1.1 request path, so this is a `Cargo.lock`-only change with no behavior impact, but it restores a green `cargo audit` gate.

## v0.10.2

**Auth-transport security hardening.**

- **Redirect-host allowlist.** The OIDC redirect follower now refuses off-domain redirects. Only the configured base host, its parent domain, and sibling subdomains are accepted (e.g. `us1.alteryxcloud.com` permits `pingauth.alteryxcloud.com`; a redirect to `evil.com` is rejected with an error).
- **Interaction-id shape validation.** The OIDC interaction id is validated at parse time: 6–128 characters, restricted charset. Malformed values are rejected before any network request is made.
- **Broader response-body redaction.** Two additional error paths (`validatePasscode` and `/v4/auth/accounts`) now redact response bodies in error output. Combined with prior redaction, all major auth-flow error paths suppress raw server responses.
- **Latent unwrap removed in `auth diagnose`.** A panic path reachable under certain error conditions in the diagnose command has been removed.

Known limitation (tracked for a follow-up): loading a profile that contains an `env:`-backed secret ref and then saving it can materialize the resolved value as a concrete secret, dropping the `env:` indirection. Preserving `env:` refs through a load→save round-trip is tracked but not yet fixed.

## v0.10.1

**Playwright fallback removed.**  The email-OTP first-login flow is now pure-HTTP only (reqwest). The headless-Chromium fallback path that was present as a last resort has been removed. There are no longer any `python3`, `playwright`, or `chromium` dependencies for authentication. The `AYX_ONE_AUTH_FORCE_BROWSER` and `AYX_ONE_AUTH_NO_FALLBACK` environment variables have been removed.

The separate `--browser` PKCE auth-code flow on `auth login` is unaffected.

## v0.10.0

**Workspace model clarified — the token determines the workspace.**  The `x-alteryx-workspace-gid` header is ignored server-side; switching workspaces requires `workspace switch` (re-points to an already-authenticated credential) or `auth login` (authenticates a new one).

**`workspace switch --workspace-id <id>`** — new command that instantly makes an already-authenticated workspace credential active.  Errors with guidance to run `auth login` if the credential doesn't exist yet.

**`workspace people` and `workspace admins` are now argless.**  `--workspace-id` has been removed; both commands are scoped to the active workspace via the token.

**Membership mutations reject a mismatched `--workspace-id`.**  `invite-users`, `remove-user`, `suspend-users`, `unsuspend-users`, `transfer`, and `transfer-assets` now error if an explicit `--workspace-id` doesn't match the active workspace.  Omit the flag and use `workspace switch` to change workspaces first.

**`auth login` warns on inline secret storage.**  When no OS keyring backend is available, the command prints a warning that credentials will be stored in the config file as plaintext.  Configuring a keyring backend (macOS Keychain, `libsecret`, Windows Credential Manager) eliminates the plaintext-at-rest risk.

**`connections connector-metadata template` placeholder output.**  When the connection type cannot be confidently inferred, `type` now emits a `<jdbc|remotefile|…>` placeholder and a `_note` field explaining the ambiguity, instead of always defaulting to `remotefile`.

- [v0.9.14](/releases/v0914/)
- [v0.9.13](/releases/v0913/)
- [v0.9.12](/releases/v0912/)
- [v0.9.10](/releases/v0910/)
- [v0.9.9](/releases/v099/)
