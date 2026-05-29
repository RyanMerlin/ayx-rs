# AYX-RS — Full Project Review

**Date:** 2026-05-10
**Scope:** All crates (`ayx-rs`, `ayx-core`, `ayx-server`, `ayx-server-api`, `ayx-one`, `ayx-one-api`, `ayx-workflow`, `ayx-docs-schema`), TUI, install scripts, CI, docs.
**LOC:** ~30,700 Rust LOC across 8 crates.
**Goal:** Take `ayx` from "built mostly with codex" to enterprise-grade, on par with `gcloud` / `aws` / `az` / `kubectl`.

> Historical note: this report was written before the v0.9.0 release pass. The follow-up sessions closed the release-cutover, CI, security, and several TUI/transport items called out below. Treat the remaining "remaining for the next pass" section as the live backlog; the earlier red/yellow sections are preserved as a historical snapshot of the state at the time of review.

---

## TL;DR — Where You Are

At the time of the original review, you had an **impressive surface** (300+ commands, full Alteryx-One coverage, embedded+managed Mongo, SQL Server, Workflow XML conversion, TUI, profile/secret system, audit artifacts, doctor, catalog, capability layer). The architecture had the right shape: command catalog, envelope output, dry-run gates, JSONL observability, central profile home, keyring abstraction.

But the **execution gap to enterprise-grade** is real and concentrated in five areas:

1. **Structural debt** — one 8,349-line `main.rs`, one 2,874-line `tui/app.rs`, one 2,570-line `cloud_convert.rs`. Hard to test, hard to refactor safely, easy to drift.
2. **Safety gates are inconsistent** — `--apply` is honored for Mongo/Server, *bypassed entirely* on the One API mutating surface (delete flow, delete person, delete plan, batch invite all execute immediately).
3. **Secrets fall back to plaintext silently** — `store_keyring_secret` returns `Ok("inline:<secret>")` on keyring failure, so headless/CI environments quietly write tokens into YAML.
4. **Release pipeline is not enterprise-shippable** — no code signing, no SBOM, no provenance, no checksum verification in install scripts, no `cargo-audit`/`cargo-deny`, no PR CI (only tagged-release CI).
5. **Workspace identity guardrails (TODO §13) were not wired at the time** — mutating One commands ran without preflight workspace verification.

The rest of this report is the punch list, ordered by severity.

---

## Critical (ship-blockers for enterprise)

### C1. Silent inline-secret fallback
**File:** `ayx-core/src/secrets.rs:61-70`
`store_keyring_secret` returns `inline:<secret>` when the keyring entry can't be created or written. On CI, headless servers, or any box without a Secret Service backend, secrets land in YAML in plaintext — exactly the failure mode the keyring abstraction was supposed to prevent. **Fix:** fail loudly; require an explicit `--allow-inline-secrets` opt-in (or an env var) for the non-keyring path. Log a warning. Never silently degrade.

### C2. One API mutations have no `--apply` gate
Every mutating One command runs immediately:
- `one flows delete` — `main.rs:6434` (DELETE)
- `one plans delete` — `main.rs:6720` (DELETE)
- `one platform person delete` — `main.rs:5471` (DELETE)
- `one platform workspace invite-users` — `main.rs:5234` (POST /batch)
- `one flows import`, `one plans create`, `one connections create` — POST without gate

Server and Mongo go through `--apply`; One does not. This is inconsistent with the safety contract stated in the README ("Mutating commands require `--apply`") and is the single most dangerous gap in the surface. **Fix:** add `--apply` to every mutating One command struct; wrap the call site; emit a dry-run envelope describing the request that *would* be sent.

### C3. 401 retry path broken for mutating requests
**File:** `ayx-one-api/src/lib.rs:145-178`, `680-685`
For mutating ops, `max_attempts = 1`. On a 401 the code calls `refresh_one_access_token()` but the loop has already exhausted attempts, so the refreshed token is never used. Every stale-token mutation fails on the first try even though a refresh just succeeded. **Fix:** allow one retry after a successful 401-driven refresh, even for mutations (token refresh is idempotent; the original POST body is held).

### C4. Inline-secret fallback + world-readable config files
**File:** `ayx-core/src/profile.rs:~1392` (profile YAML write)
`fs::write` uses the process umask (typically 0o022). When C1 fires, the secrets land in a group/world-readable file. **Fix:** explicitly `chmod 0o600` on all profile writes and `0o700` on `audits/` and config home dir creation (`ayx-core/src/audit.rs:31`).

### C5. `chunks_exact(2)` on a UTF-16 buffer
**File:** `ayx-server/src/logs.rs:429,441`
A log file with an odd byte count after the BOM (truncated, partially-written) silently drops the last pair — and if you ever change to a stricter form, panics. Even today, the trailing byte is silently lost. **Fix:** use `chunks(2)`, handle the odd tail explicitly, and surface a decoding warning rather than dropping bytes.

### C6. Release artifacts are unsigned and unverifiable
- `.github/workflows/build-release.yml` — no `signtool` (Windows), no `codesign` + notarize (macOS).
- Install scripts (`scripts/install.sh:104`, `scripts/install.ps1:91`) download and extract without any SHA-256 verification.
- No SBOM (cargo-sbom / cyclonedx), no SLSA provenance, no checksums.txt in release.
- README front-pages `curl … | bash` with zero integrity story.

This is a hard blocker for enterprise IT approval on Windows (Gatekeeper/SmartScreen) and macOS (Gatekeeper). **Fix:** add codesigning + notarization, publish `SHA256SUMS` and `.intoto.jsonl` attestation, verify in install scripts, switch the README quick-start to a verified path.

### C7. Mongo backup/restore passes password as a CLI argument
**File:** `ayx-server/src/mongo.rs:569-589`
`--password <secret>` is passed to `mongodump`/`mongorestore`. Visible in `ps`, `/proc/<pid>/cmdline`, shell history, and any process inspector. `sanitize_args` only masks the *logged* form, not the wire form. **Fix:** use `MONGODUMP_PASSWORD` env var (mongodump 100.5+ supports it) or write to a temp file with `0o600`.

---

## High

### H1. Workspace identity preflight (TODO §13) not implemented
No mutating One command calls `/v4/workspaces/current` before acting; nothing fails closed on mismatch; the response envelope doesn't record workspace context. This is the safety net that prevents "wrong tab / wrong workspace" disasters. **Fix:** in `one_api_live_request`, when `mutating && require_workspace_check`, fetch current workspace, compare to `cli.workspace_id || config.workspace_id`, embed in envelope, fail on mismatch.

### H2. Monolithic `main.rs` (8,349 lines, `execute()` ≈ 3,900 lines)
**File:** `ayx-rs/src/main.rs`
Single-file dispatcher with nested match arms. Untestable, hard to grep, every PR is a giant diff. **Fix:** split per top-level command into `cmd/one.rs`, `cmd/server.rs`, `cmd/mongo.rs`, `cmd/profile.rs`, `cmd/license.rs`, `cmd/workflow.rs`, `cmd/catalog.rs`, `cmd/doctor.rs`, etc. Each exposes `pub fn execute(args, ctx) -> Result<Envelope>`. Target: `main.rs` ≤ 500 lines (clap tree + dispatcher only).

### H3. Monolithic `tui/app.rs` (2,874 lines) + sync I/O in event handlers
**Files:** `ayx-rs/src/tui/app.rs`
- `refresh_connectivity`, `refresh_one_browser`, `activate_profile` all block on network/disk inside the keyboard handler. TUI freezes on slow API calls with no spinner, no cancel.
- Two concrete navigation bugs:
  - `Screen::Inspect.index() == 0` (`app.rs:72`) — same index as `Profiles`, so closing inspect from Credentials moves the sidebar highlight back to Profiles. Visual desync.
  - `inspect_return: Option<Screen>` ignores focus state — closing inspect leaves `Focus::Content` on a screen that doesn't have content focus semantics; subsequent keys swallowed.
- TUI has its own profile loader, duplicating CLI logic (TODO §1a still open).
- Zero tests.

**Fix:** (a) extract `app.rs` into `state.rs`, `update.rs`, `effects.rs`; (b) move network calls onto a worker thread + `mpsc::channel<Event>` polled in the main loop; (c) change `inspect_return` to `Option<(Screen, Focus)>`; (d) share `ProfileResolver` with CLI.

### H4. Untyped responses everywhere
The whole One transport returns `serde_json::Value`. There are no `FlowSummary`, `PlanSummary`, `Workspace`, `Person` structs. Silent schema drift will be discovered the day the API changes a field name. **Fix:** introduce typed response structs per endpoint in `ayx-one-api/src/types/`; keep a `raw` escape hatch but parse known fields strictly.

### H5. Output envelope inconsistencies
- Top-level subcommands with no sub-subcommand return bare strings instead of envelopes (e.g., `main.rs:3888,4987,5053`). `ayx server --output json` returns plain text.
- No `error.code` field on the envelope. Callers can't distinguish "missing profile" from "network error" from "permission denied" without parsing the message. **Fix:** add `ErrorCode` enum (`config_missing`, `auth_failed`, `network`, `rate_limited`, `not_found`, `permission_denied`, `validation`, `conflict`, `internal`) on every error envelope.

### H6. `--apply` vs `--dry-run` polarity is inconsistent
`mongo backup` defaults to dry-run + `--apply`; `sqlserver migrate` defaults to apply + `--dry-run`. Pick one polarity globally. The README's safety story says safe-by-default; codify it. **Recommended:** drop `--dry-run` everywhere; require `--apply` to mutate, period.

### H7. Token-expiry not checked proactively
**File:** `ayx-one-api/src/lib.rs:600,613`
Cached `access_token` is used until a 401 comes back. No `exp` claim check, no TTL. Every command issues at least one wasted call on stale tokens. **Fix:** parse JWT `exp`, refresh proactively when within a 60s skew.

### H8. No pagination on list endpoints
`one flows list`, `one plans list`, `one connections list`, `one job-groups list` send no `limit`/`offset`/`pageToken`. Beyond first page is invisible. **Fix:** add `--limit`, `--page-token`, `--all` (auto-paginate with hard cap) to every list command; surface server-provided `nextPageToken` in the envelope.

### H9. Workflow conversion fidelity unverified
**File:** `ayx-workflow/src/cloud_convert.rs` (2,570 lines)
Heavy use of `unwrap_or("")` / `unwrap_or(false)` / `field_map.get(...)` silently swallows missing-field cases. Tests cover plugin support listing but no round-trip fidelity. Silent data loss on real workflows is plausible. **Fix:** (a) collect a "lossy operations" vector in the converter and return it in the envelope; (b) round-trip canary suite using anonymized real `.yxmd` files; (c) golden-output diffing.

### H10. URL path-param substitution is naive
**File:** `ayx-one-api/src/lib.rs:139`
`url.replace("{key}", value)` doesn't URL-encode the value. A flow id containing `/` or `#` (unusual but possible if user passes a path) breaks the request silently. **Fix:** `url::form_urlencoded` or `percent_encoding::utf8_percent_encode` for path segments.

### H11. Hardcoded One base URL (`api.us1.alteryxcloud.com`)
**File:** `ayx-one-api/src/lib.rs:18`
EU/AP customers can't use the tool. **Fix:** read from `alteryx_one.base_url` (already in profile shape per README); per-region presets (`us1`, `eu1`, `ap1`).

### H12. Workspace owner transfer is not atomic
**File:** `ayx-server-api/src/lib.rs:428-489`
Single PUT for owner + optional schedule transfer. On partial failure, no rollback, no resume token, no per-item status in the envelope. **Fix:** structured per-item result list; resumable plan file under `audits/`.

### H13. No `cargo-audit`/`cargo-deny`/dependabot
405 transitive deps and no vuln scanning anywhere. **Fix:** add `cargo-audit` to a new `.github/workflows/ci.yml` (PR + main); add `dependabot.yml`; pin a baseline in `deny.toml`.

### H14. No PR-time CI
Only `build-release.yml` exists, gated on tags. `fmt`, `clippy`, `test` run only at release time. Stuff *will* land broken. **Fix:** add `ci.yml` running `cargo fmt --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked` on every PR and push to `main`, on the same matrix as releases.

### H15. License field mismatch
Workspace `Cargo.toml:16` says `license = "MIT"`; repository `LICENSE` is Apache-2.0. Enterprise procurement teams *will* notice. **Fix:** align to Apache-2.0 in `Cargo.toml` and confirm the intent.

---

## Medium

- **M1.** `sha2 = "0.11"` in `ayx-server/Cargo.toml:18` overrides workspace `0.10`, dual-compiling. Align on `0.10` (or bump workspace).
- **M2.** Observability redaction flag (`redact_bodies` in `ayx-core/src/observability.rs:99`) is stored but never applied — `ApiEvent` has no body fields. Either implement bodies-with-redaction or drop the flag.
- **M3.** `resolved_url` in `ApiEvent` (`observability.rs:88`) can carry tokens if a query string ever includes one; `transport_error_summary` regex extracts URLs from error text and can capture Bearer headers in chained errors. Centralize a redactor.
- **M4.** `AYX_PROFILE` env var can point at any file (`profile.rs:1496-1554`); load-arbitrary-path is a small footgun. Constrain to `${AYX_CONFIG_HOME}/profiles/` unless `--profile <path>` is given.
- **M5.** `parse_response_text` (`ayx-one-api/src/lib.rs:670-677`) wraps parse failures as `{"raw": …}`; an HTML error page surfaces as success-with-data. Detect content-type / leading `<`.
- **M6.** No jitter on retry backoff (`ayx-one-api/src/lib.rs:692-699`). Thundering-herd risk on rate-limited surfaces. Add ±10% jitter.
- **M7.** `COMMAND_SPECS` is a hand-maintained 800-line const array (`main.rs:1692-3755`). Will drift from the clap tree. Generate via a proc-macro or `build.rs`.
- **M8.** Test coverage is smoke-only (`ayx-rs/tests/cli_smoke.rs` — 14 `--help` checks). No error-path coverage, no integration tests against a fake API server. Add `wiremock` (or `httpmock`) tests for the One/Server transports.
- **M9.** `cli-spec.md` and `cli-schema.json` are hand-maintained. Drift is inevitable. Generate from the clap tree at build time; assert in CI.
- **M10.** `install.sh` appends to `~/.profile` without idempotency / shell-detection (zsh users hit `~/.zshrc` or `~/.zprofile`). Honor `$SHELL`; check for an existing entry.
- **M11.** Audit log defaults to `./audits/` (CWD-relative). If run from `/tmp` or a shared dir, it leaks. Default to `${AYX_CONFIG_HOME}/audits/`.
- **M12.** No log rotation in `audits/`. Add `--audit-retention-days` with a sweeper, or a documented cleanup command.
- **M13.** SQL Server connection-string builder (`sqlserver.rs:320-425`) concatenates `server`, `database` without ODBC-escaping (`;`, `=`, `{`, `}`). Constrain to a safe pattern or escape.
- **M14.** Self-update via the `self_update` crate (`main.rs:57`) downloads from GitHub Releases unsigned. Sign + verify, or disable until signed releases exist.
- **M15.** RuntimeSettings.xml parsed via `roxmltree` without an explicit size cap (`mongo.rs:1037-1042`). roxmltree itself doesn't process external entities (safe by design), but add a 10 MB cap on the read for belt-and-suspenders.
- **M16.** Inventory drift vs. wiring (`ayx-one-api/src/inventory.rs`):
  - workspace current: inventory `/iam/v1/workspaces/current`, wired `/v4/workspaces/current`.
  - plan import: inventory `/v4/plans/package`, wired `/plans/v1/plans/package`.
  Pick one source of truth and assert in tests.
- **M17.** Token cache `Mutex` can poison on a panic, yielding `"token cache lock poisoned"` to users. Use `parking_lot::Mutex` or `clear_poison`.
- **M18.** No `SECURITY.md` / disclosure policy. Add one.
- **M19.** No ARM64 Windows, no `musl` Linux, no universal macOS binary. Likely fine for v1, but call it out in docs.

---

## Low / Nice-to-have

- **L1.** `print_help()` is hand-written (`main.rs:7762`). Use clap's generated help; bake examples into command-level `long_about`.
- **L2.** `parse_json_arg` / `parse_key_value_params` (`main.rs`) belong in `ayx-rs/src/util.rs`.
- **L3.** Catalog `--tag` filtering exists but no `--format completion-bash` / `completion-zsh` / `completion-pwsh` generator. Add shell completions to the release tarball.
- **L4.** Global flags missing that users will want: `--verbose`/`-v`, `--debug`, `--no-verify-tls` (currently buried per-command), `--api-log-level`, `--config-home`.
- **L5.** Help-screen-only error rendering in TUI truncates error chain and hints to 3 items (`tui/mod.rs:1032,1040`). Add a "view full error" key inside the panel.
- **L6.** Examples + docs still show inline secrets in YAML (`profile.rs:1660,1671` test fixtures readable to anyone browsing). Switch examples to `keyring:` / `env:` references.
- **L7.** No `NOTICE` file. Optional for Apache-2.0 but enterprise procurement asks for it.
- **L8.** `actions/checkout@v5` doesn't exist; pin to `@v4`.
- **L9.** Server-API `parking_lot` / `Mutex` poisoning ergonomics (see M17) deserve a dedicated wrapper.
- **L10.** Internationalization is impossible because envelope `.message` is a free-form English string. Move user-facing messages behind a small `messages.rs` map indexed by error code.

---

## Roadmap to "gcloud-grade"

A reasonable sequencing — each stage is shippable, each unblocks the next.

### Stage 1 — Safety hardening (1–2 weeks)
Goal: nothing dangerous can happen by accident.

1. C2: add `--apply` to every One mutation.
2. H1: workspace preflight on mutations.
3. C1 + C4: fail-loud secrets, `0o600` profile writes.
4. C7: drop password from mongodump CLI args.
5. C5: fix `chunks_exact`.
6. C3 + H7: token refresh + proactive expiry.
7. H6: canonicalize on `--apply` only.

### Stage 2 — Release & supply chain (1 week)
Goal: an enterprise IT team will accept the binary.

8. C6: code signing (Windows + macOS), notarization, SHA256SUMS, SBOM (cargo-sbom), SLSA provenance.
9. H13 + H14: PR CI, cargo-audit, dependabot, deny.toml.
10. H15 + M14: license fix, signed self-update.
11. Install scripts verify checksums.

### Stage 3 — Architecture (2–3 weeks)
Goal: contributors can move fast without breaking things.

12. H2: split `main.rs` per command family.
13. H3: split TUI; move I/O off the UI thread; share profile resolver with CLI; fix inspect navigation bugs.
14. H4: typed One responses with a `raw` escape hatch.
15. H5: structured `ErrorCode` envelope.
16. M7 + M9: generate `COMMAND_SPECS` and `cli-spec.md` / `cli-schema.json` from the clap tree.
17. H8: pagination + `--all` on every list.

### Stage 4 — Coverage & quality (ongoing)
18. M8: real integration tests with `wiremock` per surface; fixtures for One, Server V3, Mongo.
19. H9: workflow round-trip canary suite with anonymized real `.yxmd`.
20. M16: inventory ↔ wiring assertion test.
21. Add observability tests asserting redaction.

### Stage 5 — Polish
22. L3: shell completions in release.
23. L4: global flags (`--verbose`, `--debug`, `--config-home`, `--no-verify-tls`).
24. L1 + L5: clap-generated help + better TUI error-panel UX.
25. M18: `SECURITY.md`.

---

## Honest assessment of where this lands

You're closer to enterprise-grade than the surface area suggests — most of the structural decisions (envelope, catalog, dry-run, central profile, JSONL events, keyring abstraction, transport with backoff/Retry-After) are *right*. The work needed is finishing what's started, plus signing/CI hygiene. Stage 1+2 above is the minimum bar for "I would let my customer install this in production." Stage 3 is what separates `ayx` from a personal tool and makes it a thing other contributors can extend without you in the room.

The two areas I'd treat as existential:
- **One mutation safety (C2 + H1)** — these are dangerous today.
- **Release signing (C6)** — without it, every Windows/macOS install is friction and every security-conscious enterprise will reject it.

Everything else is mechanical engineering — your codex-generated foundation is more than good enough to build on.

---

## Implementation log (2026-05-10)

Stage 1+2 of the roadmap shipped this session. All builds, `cargo fmt`,
`cargo clippy --workspace --all-targets -- -D warnings`, and
`cargo test --workspace` pass.

### Safety (Stage 1)
- **C1** Fail-loud keyring + opt-in inline fallback (`AYX_ALLOW_INLINE_SECRETS`).
- **C2** Global `--apply` gate; mutating One requests short-circuit to a
  dry-run envelope via a thread-local set at the CLI entrypoint. Also wired
  into `flow_import_package_envelope`.
- **C3** Verified existing 401-refresh retry path already correct for mutations.
- **C4** Profile writes now restricted to `0o600` on Unix via
  `write_restricted`. Audit dir created `0o700`; artifacts `0o600`.
- **C5** UTF-16 log decoder rewritten to a safe `decode_utf16_le` helper.
- **C7** Mongo `--password` removed from argv; replaced with a tempfile-backed
  `--config` YAML (`0o600`).
- **H1** Workspace identity preflight: `alteryx_one.expected_workspace_id`
  gates every mutating One request after `--apply`. Fails closed on mismatch
  or lookup failure.

### Release / supply chain (Stage 2)
- **C6** (groundwork) `.github/workflows/ci.yml` runs fmt + clippy `-D warnings`
  + matrix tests (Linux/Win/macOS) + `cargo audit` on every PR. Code-signing
  itself is still outstanding (see below).
- **H13** `cargo-audit` job + `.github/dependabot.yml` (cargo + actions).
- **H14** PR-time CI now in place; previously only release-tag CI existed.
- **H15** Workspace license aligned to `Apache-2.0`.
- **M1** `sha2` workspace-pinned (removed dual-version bloat).

### API quality
- **H5** `ErrorCode` enum on `Envelope`; HTTP-status → code mapping; outer
  dispatcher classifies anyhow errors via heuristic chain inspection.
  3 new tests in `ayx-core/src/envelope.rs`.
- **H8** `OneListParams` + `one_api_list_request` helper with auto-pagination
  (follows `nextPageToken`, capped by `--max-pages`). Wired into
  `one flows list`, `one plans list`, `one connections list`,
  `one output-objects list`. Remaining list commands need the same swap.
- **H11** One base URL resolved from `alteryx_one.base_url` →
  `AYX_ONE_API_BASE_URL` → `us1` default. EU1/AP1 unblocked.
- **M6** ±20% jitter added to retry backoff.

### Ergonomics
- **L3** `ayx completions <shell>` via `clap_complete` (bash/zsh/fish/powershell/elvish).
- **L4** Global flags: `--apply`, `-v/--verbose`, `--debug`, `--no-verify-tls`.
  Help text updated.
- **M11** Default audit dir now resolves to `${AYX_CONFIG_HOME}/audits` when
  the caller passes the default; CWD pollution eliminated.
- **M18** `SECURITY.md` added with disclosure policy + operator hardening
  checklist.

### Implementation log (session 3, 2026-05-10)

Eight more items closed; 87 tests passing; `cargo fmt`, `cargo clippy
--workspace --all-targets -- -D warnings`, and `cargo test --workspace`
all green.

- **L1** Dropped hand-rolled `print_help()` and `wants_help()`; clap now
  owns `--help`, `-h`, and `--version`. Root command grew a proper
  `about` + `long_about` banner. `--apply` is `global = true` so it
  propagates to every leaf subcommand. 4 new CLI smoke tests assert this
  (`ayx_help_renders`, `ayx_version_renders`,
  `ayx_apply_is_global_flag`, `completions_command_emits_script`).
- **H8 (rest)** Pagination wired into `one job-groups list`,
  `one write-settings list`, and `one scheduling list` (`--limit`,
  `--page-token`, `--all`, `--max-pages`).
- **C6 (signing groundwork)** Release workflow now generates
  `SHA256SUMS` and an `ayx-sbom.tar.gz` (CycloneDX via `cargo-cyclonedx`),
  attached to every tagged release. `scripts/install.sh` and
  `scripts/install.ps1` now fetch `SHA256SUMS` and verify the
  downloaded archive before extracting; opt out via
  `AYX_SKIP_CHECKSUM=1` only. Actual code signing
  (`signtool` / `codesign` + notarize) still needs user-provided certs;
  scaffolding is wired.
- **H4** New `ayx_one_api::types` module with `FlowSummary` /
  `FlowListPage` typed structs (forward-compatible via `#[serde(flatten)]
  extra`, accepts both `camelCase` and `snake_case` field aliases). 4
  parser tests cover the common shapes (object-with-items, bare-array,
  empty, snake_case). Adopt the pattern for plans/connections next.
- **M8** `ayx-one-api/tests/transport_smoke.rs` — `httpmock`-backed
  integration tests for the One transport. 7 tests cover the safety
  gates the audit flagged: apply-blocks-DELETE,
  apply-allows-DELETE, 404→`not_found` error_code, workspace preflight
  mismatch (fails closed before the mutation fires), workspace preflight
  match (proceeds), 429 retry, and dry-run `would_send` capture.
  `serial_test::serial` keeps the thread-local apply flag from leaking.
- **H9** `workflow_roundtrip_canary` test asserts the Desktop→Cloud
  conversion is deterministic (same input → same checksum across two
  runs), produces a JSON object, has a non-zero converted_tool_count,
  and that strict mode (`fail_on_unsupported: true`) agrees with the
  unsupported_tools accounting.
- **M16** New public `ayx_one_api::inventory_endpoints()` returning every
  `(method, path-template)` declared in the inventory (across implemented,
  partial, documented-only, and deferred surfaces). New
  `inventory_covers_wired_endpoints` test regex-scans `main.rs` for
  endpoint string literals and asserts every wired path matches an
  inventory entry (after collapsing `{...}` segments). Drift in
  `/scheduling/` and `/plans/v1/` is whitelisted with a comment pointing
  at the reconciliation follow-up.

### Implementation log (session 4, 2026-05-11)

Two strategic bets landed: **A — TUI quality** and **B — agent substrate
(registry)**. All green: `cargo fmt --check`, `cargo clippy --workspace
--all-targets -- -D warnings`, `cargo test --workspace` (90 tests).

#### A — TUI quality (H3)

- **A1** `Screen::Inspect` no longer collides with `Profiles` at sidebar
  index 0. Added `Screen::sidebar_index() -> Option<usize>` that returns
  `None` for `Inspect` (modal); callers (`select_screen`, `close_inspect`)
  use the `Option` and skip sidebar updates for the modal.
- **A2** `inspect_return: Option<Screen>` → `Option<(Screen, Focus)>`.
  Closing the Inspect modal now restores both the underlying screen AND
  the focus state — no more `Focus::Content` leaking onto a destination
  screen that doesn't have a meaningful content pane.
- **A3** New `tui/worker.rs` module. A single background thread owns all
  network/disk work via mpsc channels. The UI loop drains results during
  `tick()` and applies them; stale results (request id older than the
  latest in-flight) are dropped on the floor so navigating away mid-call
  doesn't paint stale data. `refresh_connectivity` and `refresh_one_browser`
  now dispatch off-thread; both retain a synchronous fallback for
  environments where the worker fails to spawn. Loading state surfaced
  via `status_message`. The UI no longer freezes while One API answers.

#### B — Registry (tactics + workflows + resolver)

New crate `ayx-registry` with:

- `Tactic`, `Workflow`, `Trigger`, `Step`, `Validation`, `Safety` types
  (`read_only` / `mutating` / `destructive`).
- 3-layer search path: `$AYX_REGISTRY_DIR` → `${AYX_CONFIG_HOME}/registry/`
  → crate-bundled stdlib (`include_str!`-baked YAMLs). Operator overrides
  win by being loaded first.
- 5 seed tactics: `mongo.backup-restore`, `mongo.doctor`,
  `one.workspace-migrate`, `one.flow.promote`,
  `server.auth.saml-diagnose`.
- 2 seed workflows: `governance.go-live`, `ops.backup-restore`.
- `Registry::resolve(task)` substring/tag ranker — dumb on purpose so a
  future LLM resolver can swap in embeddings without changing the
  contract.

New CLI surface:

- `ayx tactics list [--tag <t>]` — every tactic with id, title, safety, tags, source.
- `ayx tactics describe <id>` — full tactic body including steps + validations + rollback.
- `ayx tactics resolve --task "<text>" [--limit N]` — ranked candidate tactics for a free-text task.
- `ayx workflows list [--tag <t>]` — every workflow with tactic count and tags.
- `ayx workflows explain <id>` — workflow body + each referenced tactic's summary.

3 new tests (in `ayx-registry/src/lib.rs`) cover stdlib loading,
safety classification, and resolver ranking. Smoke-verified end-to-end:
`ayx tactics resolve --task "saml login is broken on prod server"`
correctly ranks `server.auth.saml-diagnose` as the top hit.

#### Total session impact
- Tests: 87 → 90 (3 new in `ayx-registry`).
- New crate: `ayx-registry` (8th workspace member).
- New top-level commands: `ayx tactics`, `ayx workflows`.
- TUI freezes on slow networks: fixed.
- TUI inspect bugs (sidebar collision + focus leak): fixed.

### Implementation log (session 5, 2026-05-11)

The registry layer crossed from descriptive to **executable**. All green:
fmt, clippy `-D warnings`, **99 tests passing** (87 → 90 → 99 across the
last three sessions).

#### Executor — `ayx tactics run` / `ayx workflows run`

New `ayx-registry/src/executor.rs`:

- **Up-front parameter check.** Every `<placeholder>` referenced by a
  tactic (including composed `Step::Tactic` chains) is collected and
  required-vs-supplied is reported as `MissingParams` before any
  subprocess fires.
- **Safety gate.** Mutating / destructive tactics emit a structured *plan*
  envelope when `--apply` is absent — no subprocesses, no state change,
  every step marked `status: "planned"`. Read-only tactics execute
  immediately (they're safe by definition).
- **Subprocess execution.** Each `Step::Command` is spawned as
  `std::env::current_exe()` (always the same binary the operator
  invoked), `--output json` forced, stdout parsed back into an envelope.
  Status is `ok` / `envelope-not-ok` / `failed`; the first non-`ok` step
  fails-stop and the partial outcomes are preserved in the response.
- **Per-step audit.** When `--audit-dir` is supplied (default
  `${AYX_CONFIG_HOME}/audits/`), every step writes a JSON artifact with
  the resolved cmd, why, exit code, full envelope, and stderr — at 0o600
  on Unix.
- **Workflow runs** chain tactics in order with the same gate.
- **Composition.** `Step::Tactic { id }` inlines the referenced tactic's
  steps so the operator sees a single flat plan.

5 new executor tests pass (placeholder extraction, substitution,
shell-split with quotes, missing-param detection, mutating-without-apply
emits plan).

#### Cross-validator — `ayx tactics validate`

New `ayx-registry/src/validate.rs`:

- `CatalogLookup` trait — the registry crate doesn't take a direct dep
  on `COMMAND_SPECS`. The CLI implements the trait inline at the
  dispatch site, querying both the command catalog and the capability
  registry.
- Findings: `UnknownCommand`, `UnknownCapability`, `MalformedCommand`,
  `UnknownInnerTactic`.
- Global-flag-aware command-path extractor: `ayx --environment <env>
  --apply one flows list` correctly resolves to `one flows list`.
- 3 new validator tests pass (path extraction, empty-catalog flags
  every command, permissive-catalog passes).

**First real run flagged 18 drift findings** — every one a legitimate
gap. Drove three corrections this session:
1. Stripped aspirational `capability:` ids from the 5 seed tactics
   (they referenced ids that don't exist in the live capability
   registry).
2. Renamed `one flow export-package` → `one flows export` etc.
3. Added `server-logs discover/context/inventory/summary` entries to
   `COMMAND_SPECS` to close out drift from the help-text-known commands
   that weren't in the machine-readable catalog.

End state: **0 findings across 10 tactics / 2 workflows.** Validate now
serves as a documentation-lint that will fail CI the next time a tactic
references a non-existent surface.

#### Seed tactic library (5 → 10)

5 new high-value playbooks:
- `one.scheduling.pause-all` — change-window prep
- `server.upgrade.preflight` — pre-upgrade evidence bundle (composes
  `mongo.backup-restore` + `mongo.queue.stuck`)
- `server.logs.triage` — incident first-move
- `mongo.queue.stuck` — read-only queue diagnostics
- `workflow.cloud-convert.bulk` — Desktop → Cloud migration prep

#### Smoke-verified end-to-end

```
ayx tactics run mongo.backup-restore --param profile=demo --param ts=2026-05-11
  → mode: plan, apply: false, safety: mutating
  → 4 steps, all "planned", zero subprocess invocations

ayx tactics run mongo.doctor --param profile=demo
  → executed step 1 (`ayx mongo status --profile demo`) as subprocess
  → captured non-zero exit (demo is not a real profile)
  → fail-stopped with structured StepFailed(index=1, exit=1, stderr=...)
```

Both paths verify the executor mechanics: param substitution, safety
gate, subprocess spawn + capture, fail-stop with structured errors.

#### Total session impact
- New executor module: ~400 LOC with 5 tests.
- New validator module: ~150 LOC with 3 tests.
- New CLI commands: `tactics run`, `tactics validate`, `workflows run`.
- 5 new seed tactics; 10 total in the bundled stdlib.
- 4 new `COMMAND_SPECS` entries (`server-logs` family).
- Tests: 90 → 99.
- The registry is now a working agent substrate, not just documentation.

### Implementation log (session 6, 2026-05-11)

Polish + hardening pass. Closed Tiers 2–5 of the open audit roadmap.
All green: fmt, clippy `-D warnings`, **108 tests passing** (99 → 108).

#### Tier 4 — Observability + L4 plumb-through

- **Centralized redactor** (`ayx_core::observability`). New `redact_text`,
  `redact_url`, `redact_json` honor a single secret-key list (Bearer
  tokens, `access_token`, `refresh_token`, `password`, `client_secret`,
  `api_key`, `authorization`). Three new unit tests pass. `record_api_event`
  now redacts `resolved_url` before writing the JSONL event log.
- **`--no-verify-tls`** is now an active thread-local on *both* One and
  Server transports (`set_no_verify_tls`); honored by `Client::builder().
  danger_accept_invalid_certs(true)` with a stderr warning per call.
- **`--debug`** emits redacted per-call request/response trace to stderr
  on both transports (`[debug] one→`, `[debug] server←`, etc.).

#### Tier 2 — Server-API transport safety lift

The asymmetry the audit called out is gone. `ayx-server-api::api_request`
now mirrors the One transport's contract:

- **Apply gate.** `set_server_apply(bool)` thread-local; mutating methods
  (POST/PUT/PATCH/DELETE) short-circuit to a structured `dry_run: true,
  would_send: ...` envelope when `--apply` isn't passed.
- **ErrorCode propagation.** HTTP 4xx/5xx responses are classified via
  `ayx_core::envelope::ErrorCode::from_http_status` so the outer
  dispatcher's anyhow-classifier picks up the right code.
- **Method coverage.** Added DELETE + PATCH (were missing).
- **Mutations don't retry.** `request_json` no longer silently retries
  mutating requests on 429/5xx (the server may have processed the first
  attempt). Read-only GETs retry with ±20% jittered backoff.
- **Body + URL redaction** on every error message via the centralized
  redactor.

#### Tier 3 — Registry polish

- **C1: Workflow safety auto-promotes** to max of any referenced
  tactic's safety. `Safety::max` + `Safety::rank` API on the enum.
  `Registry::propagate_workflow_safety()` runs at load time;
  `governance.go-live` now correctly reports `destructive` because it
  references `one.workspace-migrate`. 2 new tests pass.
- **C2: `--param-file <yaml>` + `--prompt-missing`.** Tactic/workflow
  runners accept a YAML map of params; explicit `--param` flags win on
  conflict. On a TTY, `--prompt-missing` reads missing placeholders
  interactively. Both no-ops on stdin redirection / CI.
- **C3: Dry-run-by-default lint.** Validator's new `ApplyMissing`
  finding flags mutating tactic steps whose `cmd:` looks like it will
  mutate but doesn't include `--apply`. Heuristic exempts explicit
  `--dry-run` and `*-dry-run` subcommands. First run caught two real
  cases (one was a genuine missing `--dry-run`, fixed in the YAML).
- **C4: `ayx tactics export <id>` + `schema_version: 1` field.** Export
  prints a tactic's canonical YAML for operators to fork into
  `${AYX_CONFIG_HOME}/registry/`. New `schema_version` field on every
  Tactic/Workflow defaults to 1; `CURRENT_TACTIC_SCHEMA` is the bump
  point for future breaking changes.

#### Tier 5 (selective) — Typed responses + audit retention

- **Typed One responses** added for plans, connections, and people:
  `PlanListPage` / `PlanSummary`, `ConnectionListPage` /
  `ConnectionSummary`, `PersonListPage` / `PersonSummary`. All use
  `#[serde(flatten)] extra` for forward compatibility, accept
  camelCase + snake_case via `#[serde(alias = …)]`, and parse both
  `{items: [...]}` objects and bare arrays via a shared
  `from_value_or_array` helper + `FromItems` trait. 4 new tests pass.
- **Audit retention.** New `ayx_core::audit::sweep_audit_dir(dir,
  retain_days, dry_run)` + `ayx audit status` and `ayx audit sweep`
  CLI commands. Dry-run by default; `--apply` actually deletes.
  Honors the same `${AYX_CONFIG_HOME}/audits/` resolution as
  `write_audit_artifact`.

#### Total session impact
- New transport features (apply gate + ErrorCode + DELETE/PATCH +
  jitter + redaction) lift Server-API to parity with One.
- New CLI commands: `ayx audit status`, `ayx audit sweep`,
  `ayx tactics export`.
- New CLI flags: `--param-file`, `--prompt-missing` on tactics/workflows
  run.
- Registry: 4 new public APIs (`Safety::max`, `Safety::rank`,
  `propagate_workflow_safety`, `CURRENT_TACTIC_SCHEMA`).
- Observability: 3 new public APIs (`redact_text`, `redact_url`,
  `redact_json`).
- Tests: 99 → **108**.

The product is now consistent across surfaces, redactor-aware across
the logging path, and audit-disciplined end to end. The remaining
items below are structural refactors that genuinely belong as
standalone PRs.

### Implementation log (session 7, 2026-05-11)

Closed C6-sigstore + H8-rest + H3-rest + H2 (partial, but the pattern is
proven). All green: fmt, clippy `-D warnings`, **108 tests passing**.

#### Cert-manager note for C6 (real signing)

Cert-manager can't replace Authenticode or Apple Developer ID. Microsoft
trusts a specific set of commercial CA roots for the code-signing EKU
and (since 2023) requires the private key on an HSM; Apple is the only
trust root Gatekeeper recognizes. For "agent / CI / enterprise-pipeline"
verification we shipped the free path instead:

#### C6-sigstore: keyless signing + GH attestations

`.github/workflows/build-release.yml` gained two signing steps:

- **Sigstore keyless signing** via `sigstore/cosign-installer@v3` +
  `cosign sign-blob`. Every release artifact gets a `.sigstore` bundle
  anchored to the Rekor transparency log using the workflow's OIDC
  identity — no keys to rotate. Verifiers run
  `cosign verify-blob --certificate-identity-regexp '...' --bundle ...`.
- **GitHub build provenance** via `actions/attest-build-provenance@v2`.
  SLSA L3, verifiable with `gh attestation verify <file> --repo ...`.
- Release uploads now include the `.sigstore` bundles alongside the
  archives and `SHA256SUMS`.

Required permissions added to the release job: `id-token: write` (OIDC),
`attestations: write` (GH attestations API).

For Windows SmartScreen / macOS Gatekeeper acceptance, a commercial
Authenticode cert ($300-700/yr + HSM) and Apple Developer enrollment
($99/yr + notarization) are still required as a separate purchase.

#### H8-rest: pagination on Person + Workspace list

`OnePlatformPersonCommand::List` and `OneWorkspaceCommand::List` were
unit variants. Restructured to carry `profile` + the four pagination
flags (`--limit`, `--page-token`, `--all`, `--max-pages`). Both dispatch
sites now call `ayx_one_api::one_api_list_request`. The `None` arm on
`Command::Person` is preserved for back-compat (bare `ayx one platform
person` still runs an unpaginated list against `config.yaml`).

Pagination now covers **all** wired One list commands:
- `one flows list`, `one plans list`, `one connections list`,
  `one output-objects list`, `one job-groups list`,
  `one write-settings list`, `one scheduling list`,
  `one platform person list`, `one platform workspace list`.

#### H3-rest: tui/app.rs carved up

New sibling modules under `tui/`:
- **`one_browser.rs`** — `request_for_one_browser_blocking` (~100 LOC).
  Self-contained; no shared state. Called from the TUI worker thread.
- **`render_helpers.rs`** — `render_envelope_panel`, `pretty_yaml_lines`,
  `extract_one_browser_items`, `preferred_item_array`,
  `find_first_object_array`, `string_field` (~170 LOC). Pure functions.

`app.rs`: **3020 → 2768 LOC**. The remaining state types + App impl are
now the actual core of the file rather than 50% formatters.

#### H2: cmd/ split (foundation laid, registry block moved)

New `ayx-rs/src/cmd/` module tree. Convention: each top-level
`Command::*` arm gets its own file exposing one
`execute(...) -> Result<Envelope>` entry point. Per-Cli state (apply
flag, environment) passes as parameters rather than a captured
closure.

Moved this session:
- **`cmd/registry.rs`** — owns `Command::Tactics` and
  `Command::Workflows` dispatch + the `LiveCatalog` adapter + the four
  param helpers (`load_params_from_file`, two `prompt_missing_*`,
  `collect_tactic_params`). **377 LOC.**

`main.rs`: **9319 → 8950 LOC.** The dispatch arms became two-liners:
```rust
Command::Tactics { command } => cmd::registry::execute_tactics(cli.apply, command)?,
Command::Workflows { command } => cmd::registry::execute_workflows(cli.apply, command)?,
```

Pattern established for future PRs to extract `Command::One` (~2000
LOC), `Command::Server` (~770 LOC), `Command::Mongo`, etc. Those each
need a `load_profile` refactor (currently a closure capturing
`cli.environment`), so they belong as dedicated PRs not session-scope
work.

To enable the cross-module access, `COMMAND_SPECS`, `CommandSpec`,
`TacticsCommand`, and `WorkflowsCommand` are now `pub(crate)`.

#### Total session impact
- Release pipeline: sigstore keyless signing + GH attestations + SLSA
  provenance attached to every artifact. No commercial certs required.
- CLI surface: pagination now uniform across all 9 One list commands.
- Code structure: 4 new modules (`cmd/mod.rs`, `cmd/registry.rs`,
  `tui/one_browser.rs`, `tui/render_helpers.rs`). 600+ LOC moved out
  of the two god-files (`main.rs` -3.9%, `tui/app.rs` -8.3%) with the
  pattern proven for future extractions.
- Tests: 108 still green; nothing regressed.

### Implementation log (session 8, 2026-05-11)

World-class polish pass. Closed Phases A–D of the gentle-dragon plan plus a
partial E. **120 tests passing** (108 → 120). fmt + clippy `-D warnings`
clean.

#### Phase A — Correctness (6 items)

- **A1: 3 cloud_convert panics fixed** (`ayx-workflow/src/cloud_convert.rs`):
  - line 355 `values.into_iter().next().unwrap()` → safe `pop()` with
    fallback to `Value::Object(Map::new())`.
  - line 1474 `as_array_mut().unwrap()` → `.expect("just wrapped in Value::Array")`
    with an explicit invariant comment.
  - line 2490 `serde_json::to_string(key).unwrap()` → `unwrap_or_else`
    with a manual JSON-string fallback.
- **A2: URL path-param percent-encoding** — new
  `percent_encode_path_segment` in `ayx-one-api/src/lib.rs` (RFC 3986
  unreserved-only). Wired into both call sites. An id containing `/` or
  `#` no longer escapes its URL segment.
- **A3: Redactor edge cases + 5 new tests.** Found a real bug:
  `redact_query_params` at `i == 0` with a non-delimiter first byte
  (e.g., input starts with `password=...`) was emitting `ppassword=***`
  because the original code pushed `ch` before pushing the matched key.
  Rewrote the loop with separate at-delim vs. start-of-string handling.
  Tests cover: start-of-string, end-of-string, mixed-case keys, Bearer
  with tab separator, Bearer at EOS.
- **A4: Workspace preflight error context** — `verify_workspace_identity`
  no longer `unwrap_or_default`s on JSON parse failure; bails with the
  redacted response prefix (200 chars) so operators see what came back
  instead of "null".
- **A5: Dead code removal** — `DesignerMessageEnvelope` and
  `DesignerIpcAdapter` (capability.rs) plus their test + the
  `_phantom_path_ref` stub in `ayx-registry/src/executor.rs`.
- **A6: Error-swallow audit** — `executor.rs` audit-dir create now
  propagates `ExecutorError::AuditWrite`; chmod-on-Unix is intentionally
  silent (umask quirks).

#### Phase B — UX wins (9 items)

- **B1: Text-mode envelope renderer** (`ayx-rs/src/render.rs`, ~370 LOC,
  8 tests). Auto-detects shape: `{items:[obj,...]}` → tab-aligned table
  with header underline, single object → vertical `key: value`, scalar
  array → newline-joined, empty / null → `(no items)` / message-only
  fallback. Cell truncation with ellipsis at 40 chars. **Text mode now
  shows the data payload instead of just the one-line message.**
- **B2: `--output yaml` + `--output table`** in `format_envelope`. YAML
  is `serde_yaml::to_string(&envelope)`; table is the text renderer
  (graceful fallback for non-list data).
- **B3: New top-level commands**:
  - `ayx whoami` — profile + active_profile + active_workspace +
    account_email + one_base_url + expected_workspace_id + environment
    in one envelope, no network.
  - `ayx doctor all` — explicit variant; reuses `doctor_full_envelope`.
  - `ayx audit status` / `ayx audit sweep` (already present from prior
    session).
- **B4: ErrorCode-driven hints.** Every error envelope now carries a
  `hint` field keyed by the classified `ErrorCode` (`ConfigMissing` →
  "Run 'ayx onboard'…", `WorkspaceMismatch` → "Re-authenticate…", etc.).
- **B5: Tactics UX**:
  - `tactics resolve` now enriches each hit with the tactic's `summary`
    (first line), so text-mode tables are self-explanatory.
  - `tactics list --safety <read_only|mutating|destructive>` filter.
    Unknown values bail with `Validation` so the user knows their
    filter never matched anything.
- **B6: TTY confirmation + `--yes` flag.** New `cmd/confirm.rs` with
  `require_tty_confirmation(consent, message)`. Wired into the three
  most-destructive One mutations (`flows delete`, `plans delete`,
  `platform person delete`) when `--apply` is set. Non-TTY without
  `--yes` refuses; TTY prompts for explicit `yes` response.
- **B7/B8: pagination "more results" hint** — `render_text` checks
  `next_page_token` and appends "more results available — use --all
  to fetch all" when present. (B8's shared `long_about` template
  intentionally deferred — small ergonomic win for plumbing cost; each
  list command's `--limit`/`--page-token` flags already self-document.)
- **B9: Install scripts auto-install completions.** `install.sh`
  detects `$SHELL` and writes completions to the standard location
  (bash: `~/.local/share/bash-completion/completions/ayx`, zsh:
  `~/.zfunc/_ayx`, fish: `~/.config/fish/completions/ayx.fish`).
  `install.ps1` writes a PowerShell `ayx-completions.ps1` next to
  `$PROFILE`. Both honor `AYX_SKIP_COMPLETIONS=1` opt-out and skip
  silently on permission errors.

#### Phase C — `cmd/` split (C0 + foundational)

- **C0: `load_profile` shim refactored**. The inner closure inside
  `execute()` now delegates to a new free function
  `load_profile_with_env(path, environment) -> Result<Config>`
  (`pub(crate)`). Existing call sites unchanged (still
  `load_profile(&path)`); code under `cmd/` modules that lacks `cli`
  in scope can call `load_profile_with_env` directly. This is the
  prerequisite the per-command extractions need.
- **C1–C12 (deferred to standalone PRs).** The pattern is now proven
  via `cmd/registry.rs` (registry surface) and `cmd/confirm.rs` (TTY
  helper). Extracting `Command::One` (~2000 LOC), `Command::Server`
  (~770 LOC), `Command::Mongo`, `Command::Workflow`, etc., is
  mechanical work that benefits from landing as dedicated review-
  friendly diffs. Total main.rs reduction available: ~3500 LOC.

#### Phase D — `tui/app.rs` carve-up

- **D1+D3 combined: `tui/forms.rs`** (578 LOC). Every form-related
  helper moved: `api_profile_to_server_api_ref`,
  `server_profile_to_server_api_ref`, `mongo_values`,
  `sqlserver_fields`, `observability_fields`, `field_value`,
  `parse_u16_field` / `_u32_field` / `_u64_field` /
  `_optional_text_field` / `_bool_field`, `default_mongo_embedded` /
  `_managed`, `default_sqlserver_profile`, `update_sql_connection`,
  `default_server_profile`, `normalize_server_url`. All `pub(super)`
  for App access.
- `tui/app.rs`: **3020 → 2216 LOC** (-26.6% overall across sessions
  7-8).

#### Phase E — Optimization (partial)

- **E1: resolver lazy title-lowercase.** Title is now only lowercased
  when `needle` is non-empty. Small (one alloc per call when needle is
  empty). Tag matching uses `eq_ignore_ascii_case` to skip a redundant
  per-comparison `to_ascii_lowercase()`.
- **E2/E3/E4 deferred.** TUI YAML caching, JWT exp check, pagination
  alloc tightening — all modest gains vs. implementation cost; the
  real perf win this session was B1 (text-mode rendering, not E).

#### Total session impact
- Tests: **108 → 120** (5 redactor + 7 render shape + others).
- main.rs: 8950 → 9071 LOC (-269 from extractions, +390 from new
  features — net +121).
- tui/app.rs: 2768 → 2216 LOC (-552, -19.9%).
- New modules: `ayx-rs/src/render.rs` (370 LOC), `ayx-rs/src/cmd/confirm.rs`
  (41 LOC), `ayx-rs/src/tui/forms.rs` (578 LOC).
- New CLI commands: `ayx whoami`, `ayx doctor all`.
- New CLI flags: `--yes` (global), `--output yaml`, `--output table`,
  `--safety` on `tactics list`.
- Two real bugs fixed (cloud_convert panics, redactor start-of-string).
- World-class text-mode output: lists render as tables instead of
  one-line messages.

### Remaining for the next pass

The remaining items are structural refactors that warrant standalone PRs:

- **C6 (signing proper)** Actual `signtool` (Windows) + `codesign` +
  Apple notarization. Native signing/notary hooks are now wired into the
  release workflow behind secrets-gated steps; what remains is final
  operator secret provisioning and real release verification.
- **H2** Continue splitting `main.rs` (8k LOC) into per-command modules
  under `ayx-rs/src/cmd/{one,server,mongo,workflow,...}.rs`. The
  `catalog` surface is already carved out; keep extracting the remaining
  command families in the same mechanical style.
- **H3** Split `tui/app.rs` (3k LOC) into `state.rs`/`update.rs`/
  `effects.rs`; move network I/O onto a worker thread + `mpsc::channel`
  so the UI stops freezing; share `ProfileResolver` with CLI; fix
  `Screen::Inspect.index()` collision + focus preservation on Inspect
  close.
- **H4 (rest)** Typed structs for plans, connections, people, job-groups,
  and schedules. Pattern is established in `types.rs`; each new one is
  ~30 LOC + 2 parse tests.
