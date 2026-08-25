# Production authentication handoff

Status: hardened implementation complete; the Wizard orchestration is the
default rollout for v0.16.0. The legacy email-OTP adapter remains present as
the explicit emergency rollback path.

## What is included

- A shared authentication state machine with bounded OTP, workspace-password,
  stale-reference, and transient-failure recovery.
- A versioned, secret-free agent protocol and platform-neutral HTTP, secure
  storage, browser/device, clock, and interaction boundaries.
- Binding-derived credential references covering account, issuer, region, base
  URL, workspace ID, and workspace GID.
- Windows Credential Manager, macOS Keychain, and Linux/FreeBSD Secret Service
  adapters through the native keyring abstraction.
- Explicit secure/session/plaintext persistence policy. Plaintext fallback is
  consent-based, owner-restricted, and warned once per profile policy.
- Transactional profile/keyring writes with a v2 journal. Recovery restores all
  pre-images, persists a `rollback_restored` phase, and retries partial backup
  cleanup safely after a crash or transient keyring failure.
- Central profile-loader and token-consumption binding enforcement. Legacy
  unbound `keyring:<profile>/<field>` references remain readable.
- A typed OTP compatibility contract and transport-level characterization,
  rejection-budget, transient-retry, password-mapping, and live-canary checks.

## Verification completed

| Environment | Result |
| --- | --- |
| Windows full nextest | 904 passed, 22 skipped; includes the slow concurrent sensitive-file test |
| Windows clippy | Passed with `-D warnings` |
| Windows formatting | Passed |
| WSL2/Ubuntu nextest | 904 passed, 23 skipped; the Windows stress test is excluded because it is platform-specific |
| WSL2/Ubuntu clippy | Passed with `-D warnings` |
| WSL2/Ubuntu formatting | Passed |
| macOS target check | `ayx-core` all-targets check passed for `x86_64-apple-darwin` |
| Terra hostile review | Final GO; no release-blocking findings |

The complete `ayx-one-api` macOS cross-target check still requires an Apple
SDK/compiler on macOS or CI; the Windows host cannot provide `cc` for that
target. This is an environment gate, not a source failure.

## Live canary gate

An operator must run the canary against a disposable profile/workspace with
real OTP access before changing rollout:

```powershell
pwsh -File .\scripts\live-auth-canary.ps1 `
  -ConfigHome C:\temp\ayx-auth-canary `
  -Profile disposable-canary `
  -WorkspaceGid <disposable-workspace-gid> `
  -BaseUrl https://<region>.alteryxcloud.com `
  -Rollout canary
```

`BaseUrl` is required because the regional host is part of the credential
binding; it must match the workspace's actual Alteryx One region and is not
assumed to be `us1`. The default `canary` run uses an isolated config home,
`AYX_AUTH_ROLLOUT=canary`, the `canary` keyring namespace, and session-only
persistence. To validate the v0.16.1 release-default compatibility lane, use
a separate isolated config home and `-Rollout legacy`; that run uses the normal
keyring namespace and should use `-SecretPolicy secure` only when persistence
is specifically under test. The script never accepts a password as a
command-line argument and scans output for secret fields before emitting it.
Record only the exit status, expiry metadata, and redacted output; do not
attach OTPs, passwords, tokens, or the isolated profile contents.

## Rollout and rollback

1. The v0.16.1 default and rollback lane are Legacy after the isolated live
   login, full One surface sweep, and release checks are green.
2. If a regression appears, keep `AYX_AUTH_ROLLOUT=legacy`; do not delete the
   legacy adapter during this release.
3. Keep `canary` reserved for isolated validation; it must not share ordinary
   profile or keyring state.
4. Decommission legacy in a separate release after a later canary, internal
   soak, telemetry review, final live OTP contract test, and explicit rollback
   approval.

## Useful checks

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace --locked
wsl.exe -d Ubuntu -- bash -lc 'cd /mnt/c/code/ayx-rs && cargo nextest run --workspace --locked --filter-expr "not test(concurrent_writes_never_tear_the_file)"'
```

The working tree should be clean after the release checks. Do not remove the
legacy adapter or publish a follow-up that changes the rollout policy without
fresh live evidence and review.
