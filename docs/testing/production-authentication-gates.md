# Production authentication release gates

The authentication redesign is released in this order:

1. `cargo test -p ayx-core --lib auth`, the sensitive-file concurrency and
   recovery tests, and `cargo test -p ayx-one-api --test auth_compatibility`.
2. `cargo nextest run --workspace --locked`, `cargo fmt --all --check`, and
   `cargo clippy --workspace --all-targets -- -D warnings`.
3. Run `scripts/live-auth-test.ps1` against the existing `local-dev` profile.
   It discovers the normal config home and profile metadata, requires the
   real OTP interaction without recording it. The default run validates
   Wizard persistence and immediately runs read-only `one auth status` and
   `one workspace current` calls using the same `local-dev` profile. Legacy is
   exercised only as an explicit rollback check if Wizard needs rollback. The
   local recorder test verifies the exact legacy endpoint order and one
   wrong-code re-prompt remains covered without live traffic.
4. Complete the Terra review with evidence for stale credentials,
   transient transport failures, concurrent/crash-safe writes, keyring failure,
   affirmative plaintext fallback, policy reset back to secure, migration, safe
   local logout cleanup, session-only rejection by the standalone CLI, and
   secret-free agent responses. The reviewer must confirm that Wizard is the default for
   the v0.17 internal release and that both `--auth-flow legacy` and
   `AYX_AUTH_ROLLOUT=legacy` remain tested rollback paths.

The live gate is deliberately not part of ordinary tests: an OTP email and a
PAT are real external side effects. The compatibility contract and all
recovery decisions remain deterministic and testable without a tenant.
