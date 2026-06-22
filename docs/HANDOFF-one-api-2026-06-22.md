# Handoff — Alteryx One API surface + auth hardening (2026-06-22)

Session handoff for continuing the Alteryx One work in `ayx-rs`. Written at **v0.10.3**.
Read this first, then `docs/one-api-surface-audit.md` (the audit checklist) and
`docs/one-live-validation.md` (per-endpoint status).

## Where things stand

The Alteryx One API surface audit (5 phases) is **complete**, and the auth flow has been
through a security + correctness red-team with **all blocking and deferred findings
closed**. Shipped across four releases today:

- **v0.10.0** — Auth GA. All 5 red-team blockers + hardening. Established the
  workspace-bound-token model; made `workspace people`/`admins` argless; added
  `workspace switch`. 288 tests.
- **v0.10.1** — Removed the Playwright/browser fallback (pure-HTTP is the only first-login
  path). Resolved red-team M4. ~505 lines deleted.
- **v0.10.2** — Auth-transport hardening: redirect-host allowlist (M2), interaction-id
  validation (M3), two more redacted error bodies, removed a latent unwrap. 306 tests.
- **v0.10.3** — Dependency security bump: `quinn-proto` 0.11.14 → 0.11.15 for
  RUSTSEC-2026-0185 (transitive HTTP/3 dep, not on the request path; restores green
  `cargo audit`).

Earlier in the day (v0.9.12–v0.9.14) landed the surface fixes: `flows update` PUT→PATCH,
`workspace people`/`admins` endpoints, the `--output-file` panic-class fix,
`flows permissions-get`, `job-groups` name synthesis, and the `connections` template
generator.

## Key model facts (verified live — do not re-derive)

1. **The PAT is workspace-bound.** The `x-alteryx-workspace-gid` request header is IGNORED
   server-side — a real gid, a bogus gid, and no header all return the same workspace's
   data. The token alone determines the workspace. This is why `workspace people`/`admins`
   are argless and why `workspace switch` re-points `expected_workspace_id` to a different
   stored per-workspace credential rather than changing a gid.
2. **The PAT scope wall.** These return `AccessControlException` (403) under the
   workspace-bearer PAT: `flows permissions-get`, `flows parameters` (recipeParameters),
   `platform role list`, `connections dry-run`. The PAT can create/read/delete flows and
   connections but not these. Resolving needs a UI-minted token or broader OAuth scopes at
   the `POST /v4/apiAccessTokens` mint step. NOT a CLI bug.
3. **Absent routes (404):** `flows validate`, `/v4/connectors` (no connector enumeration),
   `webhook-flow-tasks`. **Tier-gated (enterprise-only, 404 on platform_packaging):**
   billing, plans, scheduling.
4. **Auth flow:** pure-HTTP `reqwest` OTP → workspace-OIDC → 30-day PAT. The flow crosses
   `us1.alteryxcloud.com` and `pingauth.alteryxcloud.com`. The redirect follower is now
   constrained to the base domain + subdomains. The separate `--browser` PKCE auth-code
   flow on `auth login` is a different, intact path.

## Test account

- Account `ryan.merlin@alteryx.com`, workspace `alteryx-fde` (id=91946, gid
  `01KMGF85WTTEJZ397MW1RBD9ZB`, tier `platform_packaging`).
- Workspace password in `.env` as `AYX_ONE_WS_PASSWORD`.
- The live PAT is stored in the `default` profile (`~/.config/ayx/profiles/default.yaml`,
  `alteryx_one.access_token_ref: inline:<base64-json>`). 30-day TTL from mint.
- The binary builds to `/workspace/cargo-target/debug/ayx` (NOT `./target`).

## Open / next items (in rough priority order)

1. **Phase 4 — tier-gated surface validation.** Billing, plans, and scheduling all 404 on
   `platform_packaging`. They are documented as enterprise-tier-gated, but this is unverified
   against an actual enterprise workspace. If/when an enterprise workspace is available,
   confirm whether these are genuinely tier-gated or whether the `ayx-rs` endpoint templates
   are wrong (`/billing/v1/`, `/plans/v1/`, `/scheduling/v1/`). Until then, the current
   "enterprise-tier-gated" framing in the docs and help text stands.

2. **`connections create` end-to-end (Phase 3 PARTIAL).** The `connections template`
   generator unblocks body construction, but a real `create --apply` needs valid connector
   credentials (OAuth token for gsheets, service-account key for bigquery) not available in
   this environment, and `connections dry-run` hits the PAT scope wall (403). Revisit when
   credentials are available or with a UI-minted token.

3. **`env:`-ref round-trip (red-team security L1, tracked).** An `env:`-backed secret ref
   (`access_token_ref: env:FOO`) can be materialized into a concrete secret if a profile is
   loaded and then re-saved, relocating the secret into keyring/inline storage. NOT an active
   vulnerability (no current path exploits it), but a load→save round-trip should preserve
   `env:` refs as-is. The fix touches the secret-write path (`ayx-core/src/secrets.rs`
   `store_secret_with_fallback` + the secretize-on-write path in `profile.rs`) — make it
   carefully, with tests, not in a rush.

4. **Broader OAuth scope at PAT mint (unblocks the scope wall).** If the scope-walled
   surfaces (permissions, recipeParameters, roles, dryRun) matter, investigate requesting
   broader scopes at `POST /v4/apiAccessTokens` in the OTP flow, or supporting a UI-minted
   token. This would unblock several read surfaces at once.

## Lower-priority / nice-to-have (from the red-team, non-blocking)

- Security L2: `--debug` trace prints endpoint URLs / workspace gid / account email (run
  through the redactor, so no tokens — reconnaissance aid only). Acceptable; could gate
  behind a separate flag.
- The `connections template` `type` heuristic is best-effort (emits a `<jdbc|remotefile|…>`
  placeholder + `_note` when it can't infer). Could improve if the connector-metadata API
  exposes a clearer type signal.

## How to work in this repo

- Profile: ayx-rs dev is centralized to the **Work** profile. Repo has an identity-neutral
  `CLAUDE.md`; build/test/lint flow is in `CONTRIBUTING.md`.
- Verify: `cargo clippy --workspace --all-targets -- -D warnings` and
  `cargo nextest run --workspace --locked` (306 tests + 24 live-gated skips). `cargo fmt --all`.
- Release: bump `Cargo.toml` workspace version → CHANGELOG entry → `docs/releases/vX.Y.Z.md`
  → commit → annotated `vX.Y.Z` tag → push tags. The `v*` tag fires `build-release.yml`
  (Linux + macOS binaries, sigstore-signed) plus CI and docs-deploy.
- Docs site is Astro/Starlight in `site/` (NOT the dead `docs-site/`). It deploys to CF
  Pages project `ayx-rs` from `main`.
- Live API probing during the audit was done with raw `curl` using the decoded PAT from the
  profile + `Authorization: Bearer <token>` (the `x-alteryx-workspace-gid` header is
  optional — it's ignored). Clean up any test flows you create (`DELETE /v4/flows/{id}`) —
  the workspace should stay at 0 flows.
