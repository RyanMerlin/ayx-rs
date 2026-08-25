# Production authentication release gates

The authentication redesign is released in this order:

1. `cargo test -p ayx-core --lib auth`, the sensitive-file concurrency and
   recovery tests, and `cargo test -p ayx-one-api --test auth_compatibility`.
2. `cargo nextest run --workspace --locked`, `cargo fmt --all --check`, and
   `cargo clippy --workspace --all-targets -- -D warnings`.
3. Run `scripts/live-auth-canary.ps1` only with a disposable profile copied
   into an isolated config home. Its `-Rollout canary` mode sets
   `AYX_AUTH_LIVE_CANARY=1`, requires the real OTP interaction, checks that a
   PAT expiry is reported, and rejects secret-bearing output. Run separate
   `-UseDefaultWizard`, `-Rollout wizard`, and `-Rollout legacy` passes to
   validate the default, named Wizard, and rollback lanes. Use the default
   session-only policy for transport validation;
   use `-SecretPolicy secure` only when the isolated profile/keyring is
   disposable and binding persistence itself is part of the test. The local
   recorder test verifies the exact legacy endpoint order and one wrong-code
   re-prompt remains covered without a tenant.
4. Complete the Terra review with evidence for stale credentials,
   transient transport failures, concurrent/crash-safe writes, keyring failure,
   explicit plaintext fallback, session-only mode, migration, and secret-free
   agent responses. The reviewer must confirm that Wizard is the default for
   the v0.17 internal release and that both `--auth-flow legacy` and
   `AYX_AUTH_ROLLOUT=legacy` remain tested rollback paths.

The live gate is deliberately not part of ordinary tests: an OTP email and a
PAT are real external side effects. The compatibility contract and all
recovery decisions remain deterministic and testable without a tenant.
