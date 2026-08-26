# ADR 0002: Production Authentication Boundary

- Status: accepted
- Date: 2026-08-19

## Context

Authentication has two incompatible responsibilities: preserving the exact
email-OTP/OIDC browser-compatible transport and giving humans and agents a
recoverable, observable orchestration contract. Credential stores also differ
by platform, and an existing profile may contain inline secrets.

## Decision

Keep `ayx-one-api::email_otp` behind the versioned
`LegacyOtpCompatibilityContract` and use `LegacyOtpAdapter` as the emergency
rollback path. Put state transitions, retry budgets, recovery actions,
credential identity binding, persistence policy, and the JSON agent protocol in
`ayx-core::auth`. Native Credential Manager, Keychain, and Secret Service
backends implement the platform-neutral secure-storage interface.

The Wizard path is the default after characterization and Terra release checks
pass. `AYX_AUTH_ROLLOUT=legacy` (or `otp`) is the explicit rollback setting.
New writes use a binding fingerprint that includes
account, issuer, region, base URL, workspace id, and workspace gid. Reads keep
supporting legacy inline and unbound keyring references. A secure-store failure
can be resolved by an interactive, owner-only plaintext fallback only after an
affirmative answer. The standalone CLI rejects session-only persistence because
it has no process-spanning session; future agent execution must specify its
persistence policy.

Profile writes use the existing lock plus atomic rename, remove crash-left temp
files on recovery, and roll back keyring entries if serialization or the file
commit fails. Doctor reports inline fields without values and offers an
explicit migration.

## Consequences

The transport contract is stable and can be rolled back independently. The
orchestrator is testable without live traffic, but the final release still
requires Wizard login/persistence/API-surface evidence, concurrency,
keyring-failure, secret-leakage, and Terra review evidence. The policy sidecar
is metadata only; it never stores credentials.
