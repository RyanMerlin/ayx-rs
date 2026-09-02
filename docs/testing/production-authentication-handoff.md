# Production authentication handoff

Status: v0.17 internal-release implementation complete; automated Windows and
WSL2 gates are green, while the existing local-dev live authentication gate
remains required before broad internal distribution. The Wizard
orchestration is the default rollout for `ayx one login`. The legacy email-OTP
adapter remains present as the explicit rollback path through
`--auth-flow legacy` or `AYX_AUTH_ROLLOUT=legacy`.

OAuth2.0 API access/refresh credentials are also supported as a separate,
workspace-scoped user-credential method. They are the recommended path for
unattended CLI, CI, and agent use: import the pair once with
`--auth-method oauth-refresh` and `--refresh-token-env NAME` or
`--refresh-token-stdin`, then let secure keyring persistence and automatic
refresh handle subsequent runs. OAuth credentials never silently fall back to
email OTP.

## What is included

- A shared authentication state machine with bounded OTP, workspace-password,
  stale-reference, and transient-failure recovery.
- A versioned, secret-free agent protocol and platform-neutral HTTP, secure
  storage, browser/device, clock, and interaction boundaries.
- Binding-derived credential references covering account, issuer, region, base
  URL, workspace ID, and workspace GID.
- Windows Credential Manager, macOS Keychain, and Linux/FreeBSD Secret Service
  adapters through the native keyring abstraction.
- Explicit secure/plaintext persistence for the standalone CLI. Plaintext
  fallback is affirmative-consent-based, owner-restricted, and remembered per
  profile; an explicit secure choice resets a prior plaintext choice.
- Transactional profile/keyring writes with a v2 journal. Recovery restores all
  pre-images, persists a `rollback_restored` phase, and retries partial backup
  cleanup safely after a crash or transient keyring failure.
- Central profile-loader and token-consumption binding enforcement. Legacy
  unbound `keyring:<profile>/<field>` references remain readable.
- Logout clears profile credentials and transactionally deletes local keyring
  entries only when no other profile references them; it intentionally does
  not attempt remote PAT revocation.
- A typed OTP compatibility contract and transport-level characterization,
  rejection-budget, transient-retry, and password-mapping checks.
- Wizard now supports the bounded workspace-password retry and a clear
  secure-keyring save decision after the first successful interactive login
  (defaulting to save; `n` declines). It does not silently switch to Legacy
  after an operation may have committed.
- OAuth refresh rotation preserves provider-issued replacement refresh tokens
  in the canonical workspace keyring slot, pre-refreshes before an applied
  mutation when the access token is expired or near expiry, and never replays a
  mutation after an uncertain 401 response.

## Verification completed

| Environment | Result |
| --- | --- |
| Native Windows | Required: `scripts/internal-release-check.ps1`, installed ZIP smoke, secure Credential Manager and live default/Wizard/Legacy gates |
| WSL2 Ubuntu | Required: `scripts/internal-release-check.sh`, installed tarball smoke, Secret Service/session behavior, and live default/Wizard/Legacy gates |
| Apple/macOS | Deferred for this internal release |
| Terra hostile review | Completed; no release-blocking findings in the RC baseline |

The complete `ayx-one-api` macOS cross-target check still requires an Apple
SDK/compiler on macOS or CI; the Windows host cannot provide `cc` for that
target. This is an environment gate, not a source failure.

## Live authentication gate

Live authentication always uses the existing `local-dev` profile and its
configured account/workspace. If that profile is missing, the test harness
asks for the minimum profile values before continuing:

```powershell
powershell -NoProfile -File .\scripts\live-auth-test.ps1 -Rollout default
```

The default run tests the enabled Wizard lane and then uses the persisted
`local-dev` credentials for read-only API checks. Use `-Rollout legacy` only
when explicitly exercising the rollback path.
The script uses the normal profile and keyring namespace, attaches the login
process directly to the console, and never accepts a password as a
command-line argument. Record only the exit status and non-sensitive API
check results.

For a normal human login, no auth flags are required. `--profile` takes
precedence, then `AYX_PROFILE`, then the active profile pointer, then
`default`; Wizard and secure persistence are the defaults. On the first
interactive login Wizard offers to save the workspace password securely with
`[Y/n]` (Enter saves it). A later login reuses that keyring value; `n` keeps
the password session-only, while `--save-workspace-password` is the explicit
automation shorthand.

For a machine-oriented login, use the OAuth method explicitly and avoid secret
arguments:

```powershell
$env:AYX_ONE_API_REFRESH_TOKEN |
  ayx one login --profile local-dev --auth-method oauth-refresh `
    --refresh-token-stdin --secret-policy secure
```

## Rollout and rollback

1. The v0.17 internal default is Wizard after the local-dev live login, full
   Windows/WSL2 installed-artifact sweep, and release checks are green.
2. If a regression appears, use `ayx one login --auth-flow legacy` or
   `AYX_AUTH_ROLLOUT=legacy`; do not delete the legacy adapter during this
   release.
3. Keep the existing local-dev profile as the standing live test fixture.
4. Decommission legacy in a separate release after internal soak, telemetry
   review, final live OTP contract test, and explicit rollback approval.

## Useful checks

```powershell
cargo fmt --all --check
cargo run -q -p xtask -- refresh-command-surface --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace --locked
powershell -NoProfile -File .\scripts\internal-release-check.ps1
wsl.exe -d Ubuntu -- bash -lc 'cd /path/to/ayx-rs && ./scripts/internal-release-check.sh'
```

The working tree should be clean after the release checks. Do not remove the
legacy adapter or publish a follow-up that changes the rollout policy without
fresh live evidence and review.
