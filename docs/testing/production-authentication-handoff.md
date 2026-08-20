# Production authentication handoff

Status: hardened implementation complete; `AUTH_ROLLOUT=legacy` remains the
default. The legacy email-OTP adapter is still present and is the emergency
rollback path.

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
  -BaseUrl https://<region>.alteryxcloud.com
```

`BaseUrl` is required because the regional host is part of the credential
binding; it must match the workspace's actual Alteryx One region and is not
assumed to be `us1`. The script uses an isolated config home, `AYX_AUTH_ROLLOUT=canary`, the
`canary` keyring namespace, and session-only persistence by default. It never
accepts a password as a command-line argument and scans output for secret
fields before emitting it. Record only the exit status, expiry metadata, and
redacted output; do not attach OTPs, passwords, tokens, or the isolated
profile contents.

## Rollout and rollback

1. Keep the default on `AUTH_ROLLOUT=legacy` while the live canary and an
   internal/canary soak are completed.
2. Enable the new path for a controlled cohort only after live OTP success,
   no legacy-contract drift, clean secret-output checks, and acceptable
   recovery/telemetry results.
3. If a regression appears, set rollout back to legacy. Do not delete the
   legacy adapter during this release.
4. Decommission legacy in a separate release after the canary and soak evidence
   show that rollback is no longer needed, with a final live OTP contract test
   and an explicit removal approval.

## Useful checks

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace --locked
wsl.exe -d Ubuntu -- bash -lc 'cd /mnt/c/code/ayx-rs && cargo nextest run --workspace --locked --filter-expr "not test(concurrent_writes_never_tear_the_file)"'
```

The working tree was committed after these checks. Do not push or change the
rollout default without the live-canary evidence and a fresh review.
