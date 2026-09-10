# Phase 2 Close + Intake Handoff

**Date:** 2026-09-10
**Branch:** `integration/phase-1` @ `db3b882`, 33 commits ahead of `origin/main`
**Worktree:** `C:/code/worktrees/ayx-rs/integration-phase-1`
**Open PRs:** [#186](https://github.com/RyanMerlin/ayx-rs/pull/186) (this branch), [#187](https://github.com/RyanMerlin/ayx-rs/pull/187) (site advisories)
**Predecessor:** `docs/superpowers/handoffs/2026-09-09-phase-2-handoff.md`

---

## Status

**Phase 2 is complete.** Both exit gates are met and evidenced. The branch is
pushed and in review as #186. Nothing is blocked on a gate.

**The roadmap pass was requested and NOT done.** That is the first task below.

Verified on Windows at `db3b882`, run rather than reported:

| Check | Result |
|---|---|
| `cargo fmt --all --check` | clean |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | clean |
| `cargo nextest run --workspace --locked` | **1094 passed, 22 skipped** (session start: 1084) |
| `cargo run -p xtask -- refresh-command-surface --check` | fresh |
| `cd site && npm run build` | 83 pages |
| Live read sweep (Gate V2) | 74 invocations — 72 passed, 2 expected-unprivileged, **0 failed**, exit 0 |
| Interactive OTP onboarding (Gate V2.4) | `doctor` overall **ok** on a from-scratch profile |

---

## What landed this session

| Commit | What |
|---|---|
| `1ae7dcd` | Task 7B — `--browser` / `--device` hidden from help; recorded as **never validated**, not as an observed failure |
| `51cf8fc` | Sweep gained `expected_unprivileged` as a third outcome, plus `-AdministratorFixture` |
| `23d46c9` | Denial detection moved off text onto the exit code; `publish-info`'s denial stopped counting as a pass |
| `680d308` | Absent-data 400s classified `not_found`; each job-group leaf names itself in the envelope |
| `dcaf1a4` | **Review round 1 fixes** — see below |
| `493d0ed` | Gate V2.4 recorded |
| `84cd10f` | Docs site stopped advertising the hidden auth flows |
| `47984be` | Tracker status corrected against what actually shipped |
| `db3b882` | **Review round 2 fixes** — see below |

### Gate V2 — live read sweep

74 read-only invocations, `--output json`, release binary, `windows-otp` profile:
**72 passed, 2 expected-unprivileged, 0 failed, exit 0.**

Four of the 72 are an *expected error*, not a clean result, and the sweep now
prints that itself: `job-groups inputs` (`validation` — fixture is not JDBC),
and `profile` / `profile-results` / `pdf-results` (`not_found` — fixture has no
profiling data). The two expected-unprivileged rows are `workspace detail` and
`connector publish-info`, both `permission_denied`, both deliberately **not**
counted as passes.

### Gate V2.4 — interactive OTP onboarding

Run by hand into an isolated `AYX_CONFIG_HOME`. All six `doctor` rows on a
profile created from nothing: config ok, auth ok ("One auth configured
(time-limited login)"), network ok, one ok, server **skip**, mongo **skip**,
overall **ok**. The wizard did not offer `--secret-policy session`, confirming
its removal.

---

## Review history — read this before writing similar code

Five hostile reviews ran against this branch: four Fable subagents and one
Codex (`gpt-5.6-terra`, high reasoning). They found **real defects**, including
two cases where a commit message claimed a fix that did not work.

**The single root cause, three times over: classifying on lossy proxies when
the CLI already publishes a typed value.**

| Round | The proxy | What it actually matched |
|---|---|---|
| 1 | exit code 6 | also Gone, Conflict, RateLimited, Network, Upstream — three sweep rows accepted connection resets as passes |
| 1 | `\b403\b` in output | any three digits — a timeout carrying `elapsed_ms: 403` read as an expected denial |
| 1 | prose containing "not found" | `"Required parameter 'flowId' not found in request body"` — a genuine input error |
| 2 | `*NotFoundException` suffix | `RouteNotFoundException` — a defect in *this binary*, reported to the user as their missing data |
| 2 | `error_code` substring | the suffix of `provider_error_code`, and `body_preview` decoys in text mode |

Two further consequences the reviews surfaced:

- **`data.error_code` was still status-derived**, so one envelope carried
  `not_found` at the top and `validation` under `data` — and `data` is the copy
  the compact renderer shows. Fixed by restating the classification inside
  `one_http_envelope` so call sites cannot disagree with it.
- **`NotFound` remediation advised "list the family to find a valid one"** for a
  job group that exists. That moved the misdirection rather than removing it.

**The text-mode trap is worth remembering.** `render_text` prints `data` as
alphabetically sorted lines and does not escape strings, so an upstream body
echoed under `body_preview` sorts *before* the envelope's own `error_code`. A
substring search finds the decoy first. An anchored lookbehind does not help
(the decoy is preceded by a quote) — only line anchoring works.

Current rule in `ayx-core/src/envelope.rs`: an **allowlist** of exception types
observed live (`ABSENT_DATA_EXCEPTIONS`, currently just
`ProfilingDataNotFoundException`), with a test that fails if anyone loosens it
back to a suffix or a `contains` check.

---

## Decisions taken without the author

1. **Skipped Gate V1** (live `--browser` / `--device` test) at the author's
   direction and went straight to Task 7B. Consequence: the tracker records the
   flags as *never validated* rather than as an observed failure, and no IdP
   error code exists. Fabricating one was refused.
2. **`expected_unprivileged` is a third outcome, not a pass.** A sweep must
   never report success for a request the tenant refused.
3. **Allowlist over suffix rule** for absent-data exceptions — after the suffix
   rule was shown to sweep in route errors. The cost is that an unobserved
   type stays `Validation`, which is the safe direction to err.
4. **Sub-resource verbs named explicitly** (`SUB_RESOURCE_VERBS` in `main.rs`)
   rather than inverted. An earlier inverted list told `count`, `list` and
   `current` — which take no id at all — that "the requested data does not
   exist for that id".
5. **Site advisories split into their own PR** (#187) rather than riding along
   in #186.

---

## Errors made, so they are not repeated

- **`ayx profile delete` was asserted to exist. It does not.** `ayx profile` is
  `list | current | show | use | path | migrate`. Verify a command before
  recommending it.
- **The docs site was not swept** after hiding the auth flags. `connecting.md`
  went on listing them under "you usually won't need these, but they're there"
  — a stronger endorsement than the help text ever gave. Caught by the author.
- **A `[x]` tracker item described behaviour a later commit had changed.** A
  finished item whose body is wrong is worse than an open one.
- **A test config home was created in `C:\code\`** rather than the session
  scratchpad.

---

## The work that remains

### 1. The roadmap pass — REQUESTED, NOT DONE. Do this first.

`docs/roadmap/operator-followups.md` currently reads **48 open, 9 done**. It
needs three things added:

**(a) The five issues surfaced this session.** None is a one-liner; each needs a
design decision, which is why they were not rushed:

| Issue | Detail |
|---|---|
| No profile deletion | `onboard` creates profiles; nothing removes them. Asymmetric lifecycle. Decide what happens to keyring entries, the active-profile pointer, and audit artifacts. |
| `secret prune` cannot clean binding-scoped orphans | Its help says it only handles the pre-v0.11.0 `profile_name`-scoped scheme — the only kind current versions *don't* create. |
| `AYX_CONFIG_HOME` implies isolation it does not give | Keyring accounts are derived from the credential binding, not the folder, so two "isolated" config homes for the same identity + workspace **share one secret**. Proven: `ayx-otp-v24` and `ayx-win-otp-test` carry byte-identical `access_token_ref` values. Doc fix or a warning — decide which. |
| `render_text` ignores `remediation` entirely | `ayx-rs/src/render.rs` has no mention of it, so remediation reaches only `--output json` users. Phase 3 work; collides with `fix/human-output-rendering`. |
| Sweep exits 0 on a boundary denial | A permission regression scoped to one boundary command reads as expected indefinitely. A *global* scope loss is still caught. Arguably the correct default; CI should run `-AdministratorFixture`. Documented as a known limit. |

**(b) The author's intake document**, now preserved in the repo at
`docs/roadmap/intake/2026-09-10-merlin-issue-intake.md` (verbatim copy of
`C:\code\worktrees\merlin_issue_intake.md`) — roughly 15 issues, not yet
transcribed into `operator-followups.md`. Several are functional breakage, not
polish:

- `ayx one job-groups outputs` — *"The service returned a collection shape this
  CLI version does not recognize."*
- `ayx one agent-assets agents list` — fails with "oauth_client_id is required
  to refresh access tokens" on an OTP profile. A real auth-path gap.
- `ayx one connections permissions list` — NAME column blank, duplicate IDs in
  the output (`646` twice).
- `ayx one job-groups status` — returns nothing at all.
- `ayx one job-groups list` — NAME is `job-?` for every row.
- `ayx one connections list|detail` — missing owner, connection type, share
  count, last-accessed. Governance data absent.
- Onboarding wizard — should *prompt* for the auth type (default OTP, offer API
  token) rather than describe the command; wants colorized, well-formatted
  output; the author cited `openclaw` as the model.
- Naming — `job-groups` → `jobs` (already an open tracker item, **not started**,
  Phase 4); `agent-assets` → `agent-studio`.
- Timestamps — drop the unused microseconds.
- Structural question — is there a better hierarchy for the `job-groups` family,
  and should `publications` invert to list-all-then-associate?

One note when transcribing: the author's `job-groups profile` paste shows
`error_code: validation`, which is the **old binary**. On this branch it is
`not_found`.

**(c) The phase framing.** The tracker's own sequence puts globals *last*
(Phase 5) on purpose: `-o` touches every example in every doc, so doing it
before command names settle means rewriting all of it twice.

### 2. Phase 3 — the next body of work

Output/redaction consolidation: one redaction model, readable human rendering,
and a single canonical JSON contract, delivered as **one** change. Sections:
`Human-facing output and redaction`, `Output contract consolidation` (removing
`--output json-full` and `--error-format`), `Redaction model simplification`.
`fix/human-output-rendering` @ `2a1b0be` is a head start (passes 1054/1054).

---

## Known limits — state these in release notes, do not overclaim

- **`doctor config` warns "inline secrets found"** for any profile with an
  inline `access_token`. "A One-only profile reaches a clean `doctor`" holds for
  **keyring-backed** profiles only; the warning is correct otherwise. Gate V2.4
  confirmed the wizard's default is keyring, so the caveat did not fire there.
- **`--browser` / `--device` are unvalidated**, not broken-and-proven. The device
  authorization endpoint is derived by string substitution on the token endpoint
  (`ayx-rs/src/cmd/one_platform/auth.rs:776-777`) with no discovery lookup.
- **Exit codes are many-to-one.** Six error codes share exit 6. Match
  `error_code`, never the exit code alone. Now documented in `docs/cli-spec.md`
  with the full map.

---

## Environment notes

- **`gh` is authenticated in WSL2 only**, not on Windows. The worktree `.git`
  file holds a Windows path WSL cannot follow, so run `gh` from a neutral
  directory with `--repo` / `--head`:
  `wsl -e bash -lc "cd ~ && gh pr create --repo RyanMerlin/ayx-rs --base main --head <branch> ..."`
- **CodeGraph is working** (MCP tool + CLI). Run `codegraph sync .` first — a
  stale index omits source for drifted files rather than showing wrong lines.
- **Two binaries both report `ayx 0.20.5`.** `~/.local/bin/ayx` on PATH has none
  of this branch's work. Always use
  `C:\code\worktrees\ayx-rs\integration-phase-1\target\release\ayx.exe`.
- **Worktrees are consistent**, all under `C:\code\worktrees\ayx-rs\`:

  | Worktree | Branch | State |
  |---|---|---|
  | `C:/code/ayx-rs` (main checkout) | `codex/dependency-updates-review` | 5 commits, unmerged; carries a deps commit **plus** unrelated features (durable api-token auth, MCP demo clients) — review on its own merits |
  | `integration-phase-1` | `integration/phase-1` | PR #186 |
  | `site-dependency-advisories` | `fix/site-dependency-advisories` | PR #187 |
  | `human-output-rendering` | `fix/human-output-rendering` | Phase 3 head start |
  | `legacy-flows-gate` | `feat/legacy-flows-gate` | **does not pass** — 5 `cli_smoke` failures; the gate compiles out `one flows` and those tests were never feature-gated. Phase 4. |
  | `onboard-bare-workspace-base-url` | `fix/onboard-bare-workspace-base-url` | already merged into `integration/phase-1` |

- **Config homes in `C:\code\` are `AYX_CONFIG_HOME` overrides, not worktrees:**
  `ayx-win-otp-test` (**in active use by the sweep — do not delete**),
  `ayx-otp-v24` (Gate V2.4, deletable), and three stale
  `ayx-win-onboard-*-check` directories that completed no login and hold no
  token refs — plain `rm -rf` is the whole cleanup for those.
- **Parked branches** not needed for either PR: `feat/one-workflow-download`
  @ `433e9fe` (complete but unverified — the version-download route 401s on the
  current OAuth profile), `docs/readme-product-sections` @ `088e1e4` (WIP,
  partly superseded).

---

## Recommended order for the next session

1. **The roadmap pass** — (a), (b) and (c) above. Nothing else should start
   until the intake is captured; it is captured only in the intake file and
   this handoff, not in the roadmap itself.
2. **Triage the intake's functional breakage** — `job-groups outputs`,
   `agent-assets agents list`, `connections permissions list`, `job-groups
   status`. These are broken commands, not polish, and they are not yet on the
   roadmap at all.
3. **Phase 3** once the roadmap reflects reality.

Do not re-run completed Phase 2 work. Git history is the authoritative record;
the SDD ledger at
`.superpowers/sdd/2026-09-09-phase-2-one-only-release-blockers/progress.md` is
retained (gitignored) as the detailed audit trail.
