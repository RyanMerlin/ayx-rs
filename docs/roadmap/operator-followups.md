# Operator Follow-ups

Status: active

This is the durable intake for issues found during operator use, release
validation, and cross-platform testing. Keep the evidence and desired outcome
here while a topic is being understood; promote a bounded, reproducible unit
of work to a GitHub Issue before implementation. Add the issue link here, then
remove or reduce the completed item to its decision/outcome.

## Triage rules

- Record the CLI version, platform, command, and safe/redacted evidence.
- State the affected product explicitly: Alteryx One, Alteryx Server, Mongo,
  Designer, or shared CLI. Do not use a combined label when products are
  independent.
- Use a GitHub Issue as the source of truth once an item has a clear owner,
  scope, and acceptance criteria. Link the issue from this file.
- Keep longer-term sequencing or cross-cutting decisions in the themed
  roadmap documents; keep release evidence in release notes or testing docs.

## Fresh-session execution sequence

Do not treat this intake order as implementation order. The next session
should use these phases, stopping after each release-risk phase for a Windows
re-test before widening the change set.

1. **Establish a clean integration base and re-run evidence.** Preserve the
   existing dirty root worktree; inspect and intentionally integrate only the
   prepared changes below. Run the One read sweep against the resulting binary
   and record its platform, binary version, active profile type, and outcome.
2. **Remove release blockers on One-only Windows use.** Fix onboarding with a
   bare workspace GID and relative profile path; decide and accurately expose
   the email-OTP credential lifecycle; fix or relocate `ayx one api` commands
   that incorrectly require Server configuration. Re-test OTP onboarding and
   all One reads before a release candidate.
3. **Make output trustworthy before expanding examples.** Deliver one
   redaction model, readable human rendering, and the single canonical JSON
   contract as one coherent change. This includes the query-transform contract
   and removal of output-format/error-format dead ends. Verify text, JSON,
   `-q`, audit output, and PowerShell behavior with secrets and safe expiry
   metadata.
4. **Make One discoverable and product boundaries unambiguous.** Set the
   product taxonomy, remove legacy/duplicate/retired One names, decide the
   `flows` policy, and then redesign the workspace hierarchy. Rewrite README
   examples only after the retained commands and names are decided; otherwise
   documentation will immediately churn again.
5. **Finish ergonomics and deferred configuration work.** Add short flags
   (`-o`, `-y`, `-q`) with docs/completions, scope globals, then resolve or
   remove the incomplete environment model. Do the richer connection
   classification/governance work after the stable public surface is settled.

**Status, 2026-09-10:** phases 1 and 2 are complete on `integration/phase-1`
([#186](https://github.com/RyanMerlin/ayx-rs/pull/186)). Phase 3 is implemented
on the follow-up branch and has passed the local suite plus redacted live reads
using the keyring-backed `local-dev` profile. Review/merge and a non-empty
job-output fixture remain external dependencies.

**Why globals are deliberately last.** `-o` and the rest of the global-scope
work touch every example in every document, skill, runbook and generated page.
Doing them before Phase 4 settles command names means rewriting all of those
examples twice -- once for the flag, again for the rename. The order is a
cost decision, not a priority ranking: a global item is not less important for
sitting in Phase 5.

**Where the 2026-09-10 intake lands.** The operator intake of that date
(`docs/roadmap/intake/2026-09-10-merlin-issue-intake.md`, preserved verbatim)
is transcribed into this file. Its *functional defects* -- commands that are
broken today, not merely unpolished -- are collected in
[their own section](#operator-intake-2026-09-10-functional-defects) and are
not held for any phase: each is small, independently testable, and should be
fixed before Phase 3 widens the change set. Its UX and naming requests are
filed under the existing phase sections they belong to, each tagged
`(intake 2026-09-10)`.

### Prepared, independently tested changes

**Superseded as of 2026-09-10: every change listed below is now committed on
`integration/phase-1` and needs no further merge review.** The list is kept
because the notes on what each change was tested against remain useful. The
closing paragraph about "local, uncommitted experiments" is likewise stale --
the bare-GID onboarding fix is `33e9a74`, and the human-output/redaction work
lives on `fix/human-output-rendering` as Phase 3 material.

- `b25b1b7 fix(cli): wrap help descriptions` sets a 120-column clap help
  width, improves `--no-input` wording, and adds a help-wrap smoke test.
  `cargo test -p ayx-rs --test cli_smoke` passed (65 tests).
- `1a91ab6 fix(one): remove retired person count` removes `ayx one person
  count` across CLI, inventory, catalog, docs, and tests. The CLI smoke suite
  (65 tests), One API inventory tests (3 tests), formatting, and generated-doc
  checks passed.
- The bare-GID onboarding fix is `33e9a74`. The human-output/redaction work is
  Phase 3 material and is tracked by its implementation branch and review.

## Operator intake 2026-09-10: functional defects

Priority: fix before Phase 3 -- these commands are broken today

Product: Alteryx One throughout. Reported by the author against the
`integration/phase-1` release binary; **every item below was re-reproduced on
2026-09-10** on Windows with `ayx 0.20.5` built at `db3b882`, `windows-otp`
profile (email OTP), workspace `91946`. The root causes are from the captured
full redacted JSON payloads and the source, not from the human rendering the
intake quoted.

**Status:** the Phase-1 fixes are complete (`cc2e465`, `203e38f`, `528dced`).
Phase 3 completes connection-permission rendering and the output contract;
redacted live permission and workflow-tool reads pass. A non-empty job-output
fixture and PR review remain external dependencies.

- [x] **Job Library entries with upstream `name: null` are intelligible in
  human output.** Numeric IDs are handled. Phase 3 keeps upstream `name: null`
  truthful in canonical JSON and adds a presentation-only label for
  `ayx one jobs list` human output. As first shipped the label went into a
  separate `display_name` field that the command's declared list columns never
  projected, so it never appeared; since the RC output-contract review it
  fills the unnamed row's NAME cell in the terminal copy instead.
- [x] **A 401 on any One read is replaced by an unrelated configuration error
  when the profile has no refresh credential.** **Fixed** in `203e38f`: a 401
  is retried through a refresh only when the profile can renew (a refresh
  token, or a service principal); otherwise the 401 envelope reaches the user
  as `auth_failed`, exit 4, with the One "log in again" remediation. The
  applied-mutation preflight, which had the same flaw for a stale token, now
  fails with a typed `OneLoginExpired`. Original detail follows. Seen as
  `ayx one agent-assets agents list` failing with
  `alteryx_one.oauth_client_id is required to refresh access tokens`,
  `error_code: validation`, exit 2. `--debug` shows one attempt to
  `GET /ai-agents/backend/agents` and no second: on a non-mutating 401 the
  client unconditionally attempts a refresh-token exchange
  (`ayx-one-api/src/lib.rs`, the `StatusCode::UNAUTHORIZED` arm of the request
  loop), and on an email-OTP profile, which has no refresh token by design, the
  refresh fails before the request is retried and `refreshed?` returns *that*
  error instead of the 401. Two consequences, both wrong:
  - the upstream status never reaches the user, and the classification is
    `validation` -- "inspect the failed flag" -- for a request with no bad flag;
  - **by reading the code (not yet reproduced),** an expired OTP login would
    fail the same way on every One read, instead of saying the login expired
    and naming `ayx one login`. That would undercut the Phase 2 OTP-expiry
    messaging exactly when it matters.

- [x] **`ayx one agent-assets` does not work with any bearer-token
  credential.** Unmasking the 401 above revealed the upstream body: `"No
  Alteryx session cookies found"`. The `/ai-agents/backend` service
  authenticates with browser session cookies, not bearer tokens. Reproduced
  2026-09-10 on **both** credential types -- the `windows-otp` email-OTP
  profile and the `local-dev` OAuth API-token profile -- for `agents list`,
  `datasets list` and `workflows list`, which between them cover every
  `/ai-agents/backend` route this surface calls. So this is not OTP-specific,
  and "log in again", the remediation any 401 now gets, is **wrong advice
  here**: no `ayx one login` produces a session cookie.

  This is a product-surface decision, the same shape as the hidden
  `--browser` / `--device` flags: an advertised command that cannot succeed
  for any user of this CLI. Options: hide `agent-assets` until a bearer-token
  route is confirmed with the vendor; or keep it and give its 401 an honest
  remediation. Do not decide it by matching the cookie message in code -- that
  is prose-matching again. Settle it before the Phase 4 `agent-studio` rename,
  since renaming a surface nobody can use is wasted work. **Disposition:**
  hidden from help, catalog, and normal discovery until the vendor supplies a
  supported bearer-token contract; it is not an expired-login defect.
- [x] **`ayx one jobs status` prints nothing in text mode.** **Fixed**
  in `528dced`: a bare scalar body is labelled with the command's single
  declared field, so text prints `status: Complete`; canonical JSON retains
  the upstream scalar response.
- [x] **`ayx one jobs outputs` reports an "unrecognized collection
  shape".** Named collections explicitly render as `Files` and `Tables` in
  human output; canonical JSON preserves the complete response. The available
  25-job live scan still found only empty collections, so a non-empty provider
  fixture remains an external dependency.
- [x] **`ayx one connections permissions list|detail` shows blank names and an
  apparent duplicate.** Neither is data corruption:
  - `name` and `email` are **empty strings upstream** for every subject from
    `GET /v4/connections/{id}/permissions/sharedSubjects`. The CLI is not
    losing them; the endpoint does not populate them. Resolving the ID to a
    person (the IDs are workspace person IDs) is the only way to show a name.
  - person `646` appears twice because it genuinely holds **two entries**:
    `roleType: owner` and `roleType: collaborator`. The projection keeps only
    `id` and `name`, dropping `roleType`, `isCreatedBy`, `policyTag`,
    `subjectType` and `source`, so two distinct grants look like one row
    printed twice. The `groups` array is dropped as well.

  Phase 3 flattens both buckets into distinct rows, resolves people from one
  bounded `/v4/people` listing, reports lookup failures and unresolved names
  explicitly, and retains all provider rows. The governance fields the author
  asked for (when granted, last
  access, workflows attached) are **not** in this payload; they belong to the
  [connections governance](#alteryx-one-connections-classification-and-governance)
  work and must be labelled as heuristics where they are derived.

## CLI flag ergonomics

Priority: near-term UX improvement

- [ ] Add `-o` as the global short form of `--output`.
  - Rationale: output selection is a frequent interactive and automation
    operation; no current short flag uses `o`.
  - Acceptance: `ayx one auth status -o json` is equivalent to the long form,
    works at every command depth, appears in help and generated completions,
    and has smoke coverage.
- [ ] Add `-y` as the global short form of `--yes`.
  - Rationale: it is the conventional explicit-confirmation spelling.
  - Acceptance: it remains equivalent to `--yes` for every guarded mutation;
    dry-run and confirmation behavior are unchanged.
- [ ] Evaluate `-w` for `--workspace` and `-e` for `--env` after the first
  short-flag release.
  - Do not assign `-v` to version: it is already the global `--verbose` flag.
    `-V, --version` is the existing conventional version spelling.
  - Avoid short forms for `--apply`, `--debug`, TLS bypass, and pagination
    until a specific repetitive operator workflow justifies them.
- [ ] After `-o, --output` ships, update every user-facing example, help
  snippet, skill, runbook, README, and generated documentation to use `-o`
  rather than `--output`. Keep the long form in reference prose where it aids
  discoverability; do not leave mixed example conventions.

## Global option scope and error-output policy

Priority: high command-surface clarity

The global option list currently advertises controls on every command even
when they belong only to one product or operation class. Windows review also
found an internally inconsistent error-format option:

- [x] Remove `--error-format` with the JSON consolidation. `--output json`
  now renders one complete recursively redacted envelope for success and
  failure; terminal projection remains human-only.
- [ ] Keep `--no-input` global, but describe its actual process-wide contract:
  never read interactive input; fail closed when a credential, secret,
  ambiguous selector, or destructive confirmation would otherwise prompt.
  This is not login-only: it protects selector pickers, `secret set`, and
  every confirmation helper as well as interactive login.
- [ ] Remove `--no-verify-tls` support from the One client. Alteryx One is a
  SaaS endpoint and certificate validation is non-negotiable; a One command
  must never offer an insecure TLS bypass. Scope the option under `ayx server`
  only, where an operator may need a temporary lab/self-signed-certificate
  escape hatch. Retain the Server profile TLS setting as the durable policy.
- [ ] Audit every remaining global by actual scope:
  - keep root-global: `-o, --output`; `--no-input`; `--yes`; `--apply`;
    `--verbose`; and `--debug`. These are invocation-wide rendering, safety,
    or diagnostics policies. Correct `--apply` help so it never implies One
    and Server are the same product.
  - remove `--output-limit` with compact JSON projection. Output truncation is
    a format-level policy we have already decided to replace with normal
    pagination and explicit command summary/detail choices.
  - scope to `ayx one`: `--workspace` and `--page-size`; neither is read by
    Server, Mongo, Designer, or local tooling today.
  - normalize profile selection deliberately. Windows sweep work found that
    some One leaves accept `--profile` while others reject it, even though all
    resolve the active profile. Do not add a root-global `--profile` until we
    separate a profile *name* selector from onboarding's profile *file path*
    and eliminate leaf-by-leaf inconsistencies.
  - remove or hide `--env` from the public root help until the documented
    environment model actually works; restore it only if it selects a genuine
    cross-product runtime configuration.
  - retain the bundled query transform and add `-q` as the conventional short
    form of `--jq <FILTER>`; keep `-r, --raw-output` as its jq-compatible
    modifier, requiring `--jq`. Do not rename it to the generic `--query`:
    that spelling conventionally means JMESPath in other CLIs, while AYX
    accepts jq syntax. This is a useful Windows-friendly escape hatch for
    extracting fields from a large response without requiring a separate jq
    installation.
  - define one strict query contract: evaluate the filter against the complete,
    recursively redacted canonical JSON envelope after successful command
    rendering. `--jq` may internally select JSON rendering, but must not
    reintroduce compact versus full JSON modes or leak unredacted data. Its
    deliberately transformed scalar/line output is the one exception to the
    normal envelope contract; preserve the underlying command exit status and
    report invalid filters as structured command errors.
  - preserve existing query sandboxing (no environment, clock, or halt access)
    and add regression coverage for redaction-before-query, `-q`, raw strings,
    invalid filters, and nonzero command failures. Prefer server-side filters
    and pagination for genuinely large collections: a local jq filter runs
    only after the complete response has been downloaded.
  Do not use `global = true` merely to make a flag convenient at every parser
  depth.

## One resource naming, deprecation, and examples

Priority: high before public announcement

Windows live verification found that several current One example commands are
either deprecated, empty in the test workspace, or named after backend
implementation terms rather than an operator-facing resource.

- [x] Remove `ayx one person count` now, with no compatibility alias. The
  vendor endpoint `/v4/people/count` is already retired (HTTP 410 / the
  `IAM_SCREAM_PEOPLE` removal); it must disappear from the CLI, inventory,
  catalog, generated command surface, README, tests, and current docs.
  **Done** in `1a91ab6` / `ec52829` (the second removed the site page the
  first missed). `ayx one person --help` no longer exposes `count`.
- [ ] Consolidate the duplicate people surfaces. `ayx one person list` and
  `ayx one workspace people` both issue workspace-scoped `GET /v4/people` and
  returned the same 18 ids in Windows validation.
  - Decide whether the remaining resource is `workspace people` (best matches
    the actual membership scope) or `workspace users` (more conventional
    operator language); retain a narrowly scoped self-identity command rather
    than a second membership list.
  - Do not rename to `users` merely as an alias while two overlapping command
    trees and inconsistent payload terms remain.
- [x] Replace the `job-groups` user-facing namespace with the Jobs surface
  backed by the supported Job Library list/count contract. **Done 2026-09-11.**
  - `ayx one jobs <JOB-ID>` retrieves the aggregate Job Library entry;
    `ayx one jobs runs <JOB-ID>` returns the complete child-run collection;
    `ayx one jobs execute --body <FILE>` submits a Job Group. The old
    `job-groups` namespace is hidden but retained as a compatibility command,
    including its former mutating `run` verb.
  - The live OpenAPI marks `GET /v4/jobGroups` deprecated and directs callers
    to `/v4/jobLibrary`; the current list/count already use Job Library, but
    detail/run/status operations retain job-group endpoints.
  - The live OpenAPI defines a Job Group as a job executed from a flow node and
    `GET /v4/jobGroups/{JOB-ID}/jobs` as its batch-job collection. Its child
    records have their own numeric `RUN-ID` and a `jobGroup` parent reference.
    The only direct child route is status-only
    `GET /v4/jobs/{RUN-ID}/status`; no full child detail route exists. The
    `runs` command therefore deliberately presents every returned child record
    instead of inventing a false singular detail command.
  - **Run metadata the author needs, and where it already lives.** Who ran it,
    how long it took, whether it errored, which workspace, what triggered it,
    and which outputs it produced. The live payloads already carry most of it;
    the CLI projection drops it:
    `GET /v4/jobGroups/{id}` has `creator.id`, `ranfrom` (the trigger, e.g.
    `ui`), `ranfor`, `workspace.id`, `workloadError` and `status`;
    `GET /v4/jobGroups/{id}/jobs` has per-job `startedAt`/`finishedAt` (so
    runtime is computable), `errorMessage`, `hasWarnings` and `jobType`;
    `outputs` has the `files`/`tables` refs. Only the creator's *name* needs a
    second lookup. Conversely, each **list** item embeds the creator's entire
    person record -- email, `maximalCapabilities`, `maximalPrivileges` --
    about 14 KB for a single row, which the list view should stop requesting
    or stop carrying.
  - Human `runs` output is a vertical record per child run, including identity,
    parent, type, status/progress, timestamps, heartbeat, warning/error,
    execution context, sample size, and script references. Canonical JSON
    retains the full recursively redacted provider response.
  - **Inverted publications (intake 2026-09-10).** Asked: list all
    publications and show the job each belongs to, instead of drilling into
    one job group at a time. Needs research first: whether the API offers a
    workspace-wide publications collection, or only the per-job-group route
    this command uses. A client-side fan-out across every job group is not an
    acceptable substitute for a real collection.
- [ ] Research whether `/v4/outputObjects` is a current One resource or a
  Designer Cloud/Trifacta-derived surface before renaming it. The endpoint
  worked in Windows validation but returned zero items; it is not a credible
  quick-start example. If retained, use an operator-facing plural name such
  as `outputs` only after its lifecycle and relationship to workflows are
  documented.
- [ ] Decide whether `ayx one agent-assets` should be named after the product,
  `agent-studio` (intake 2026-09-10). The author asks why the command does not
  use the product name. Confirm the current product name and what the
  `/ai-agents/backend` surface actually covers before renaming; settle it in
  the same Phase 4 naming pass as `jobs`, so the two renames ship with one
  migration table rather than two. **Blocked on** the finding in
  [the intake defects](#operator-intake-2026-09-10-functional-defects) that
  the whole surface needs browser session cookies and fails for every
  bearer-token credential; decide whether it stays before renaming it.
- [ ] Rewrite README Quick Examples as a tested first-run path, not a grab-bag
  of easy list/count calls. Each example must be available in the reference
  One workspace, explain a meaningful capability, name its product clearly,
  and avoid deprecated, empty, or legacy-only surfaces.

## Root `doctor` must preserve product boundaries

Priority: high UX/diagnostic clarity

Observed on Windows with `ayx 0.20.5` and an Alteryx One-only profile:

```text
⚠ auth      warn   One incomplete; Server not configured
⚠ network   warn   One endpoints configured; no live probes run
⚠ one       warn   One refresh token and client id missing
– server    skip   Server not configured
```

The final `server` row correctly says Server is not configured, but the
earlier summaries conflate separate products. An email-OTP One profile is also
reported as incomplete merely because it intentionally has no OAuth refresh
credential.

- [x] Make root `doctor` report One, Server, and Mongo as independent domains.
  **Done and verified live in Gate V2.4** (see the authentication section):
  every row named one domain, an unconfigured Server was a Server-only `skip`,
  Mongo likewise, the OTP credential read as a time-limited login, and the
  rollup was `ok`. Sub-bullets below record the original requirement.
  - Each row must name one product/domain and include only that domain's
    evidence and remediation.
  - An unconfigured Server is a Server-only `skip`; it must not make One auth
    or network warnings appear incomplete.
  - A One email-OTP credential should be described as time-limited/non-rotating
    rather than incomplete. Guidance may recommend a durable OAuth refresh
    credential without classifying the configured OTP flow as invalid.
  - If `doctor` retains an overall rollup, it must be explicitly a rollup and
    retain per-product statuses in structured output.
  - Add snapshots or integration tests for One-only, Server-only, and combined
    profiles.
  - Structured output uses two distinct nodes and they must not be confused:
    `data.checks.one` is the One probe row (its own `status`, `summary`, and
    `workspace_probe`), while `data.checks.auth.one` is the auth row's One
    section (`credential_kind`, `renews_automatically`,
    `access_token_expires_at`, `guidance`). An agent reading guidance wants
    `data.checks.auth.one.guidance`; there is no `guidance` under
    `data.checks.one`. Under the scoped `ayx doctor auth` the same node is at
    `data.one`, because a scoped check returns its own payload at the data
    root.
  - `probes_run` in the `network` row is scoped to that row's own targets.
    A full `ayx doctor` run still probes: the `one` check calls
    `GET /v4/apiAccessTokens` when an access token is present.

## `ayx one api` must be a One surface, not a Server API dependency

Priority: high product-boundary defect

Windows read-only validation on 2026-09-09, using a working One-only OTP
profile, established that these commands never reach Alteryx One:

```text
ayx one api status
ayx one api diagnose
```

Both fail locally with `config missing api/server_api section`. That is an
incorrect dependency under an `ayx one` namespace, not an incomplete One
profile.

- [x] Rewire these commands to use the One configuration and endpoints, or
  move/rename them under the actual Server API surface if their intent is
  Server-only. Do not retain an ambiguous hybrid command. **Done** in
  `d28f295` / `9770be2`: `ayx one api status|diagnose` report
  `product: "Alteryx One"` and no longer require an `api:` section. Both pass
  in the live sweep.
- [ ] Add a One-only integration test proving every `ayx one api ...` command
  either succeeds against One or fails with a clearly One-owned error.
  **Partially done.** `one_api_commands_never_require_server_configuration`
  covers `status` and `diagnose`. `open-api-spec` and `coverage` are not
  covered, because both fetch the live OpenAPI document and adding them would
  put a network call inside a suite that is deliberately hermetic. Closing
  this needs a recorded spec fixture or an injected transport, not just two
  more loop entries.
- [ ] Update help, catalog, README, and testing documents to name the owning
  product explicitly. **Mostly done:** help, README and the site were updated
  in Task 6, and 122 false `server_api` prerequisites were removed from the
  `one/...` catalog in `5562991`. The testing documents under `docs/` have not
  been swept for product ownership, so this stays open on that narrower scope.

## Product boundary and documentation architecture

Priority: highest — terminology and navigation correctness

Alteryx One and Alteryx Server are separate products with no configuration,
compatibility, or health dependency. Designer/Desktop, local workflow XML,
and Cloud Execution for Desktop are distinct legacy/adjacent technology
surfaces. The CLI and its documentation must make those boundaries obvious,
not imply that configuring or diagnosing one establishes anything about
another.

- [ ] Restructure the README command examples into explicit product sections:
  **Alteryx One** as the primary section; a smaller **Alteryx Server and
  Mongo** section; and a separately labelled **Designer/Desktop and local
  workflow files** section where applicable. Do not mix commands in a single
  quick-start sequence.
- [ ] Establish a command/docs taxonomy and apply it everywhere: CLI help,
  command catalog, discover output, README, roadmap, endpoint inventory,
  diagnostics, and release notes.
  - `ayx one ...` is the Alteryx One namespace and must be named as One in
    prose, summaries, errors, and headings.
  - `ayx server ...` is Alteryx Server only; Mongo status is its own local
    data-store/runtime domain, never implicit One status.
  - `ayx designer ...`, XML, and desktop execution must carry explicit
    Designer/Desktop/local wording and must not be described as One workflow
    support.
  - Designer Cloud/Trifacta-derived `/v4/flows` is an opt-in legacy surface,
    never an example of cloud-native Alteryx One workflows.
- [ ] Audit ambiguous generic vocabulary — especially `workflow`, `flow`,
  `auth`, `connection`, `workspace`, `doctor`, `profile`, and `cloud` — and
  require an owning product in human-facing output whenever context is not
  already unambiguous from the command namespace.
- [ ] Add product-boundary regression coverage:
  - One-only, Server-only, Designer/Desktop-only, and combined profiles;
  - generated help/catalog snapshots proving One and Server commands remain
    separately grouped;
  - documentation checks that quick-start examples do not cross products;
  - diagnostics where an unconfigured product is a local `skip`, never a
    warning attached to a configured product.

## Alteryx One workspace hierarchy redesign

Priority: high information-architecture improvement before public promotion

`ayx one workspace --help` currently exposes roughly forty flat leaf commands.
It leaks implementation names (`configuration-v4`), mixes workspace lifecycle,
membership, group administration, cloud configuration, and transfers, and
forces users to scan verb-prefixed variants such as `add-group-users` and
`save-current-configuration`.

Windows validation and a review of mature CLI patterns point to a resource
hierarchy, not a blanket extra nesting level:

- GitHub CLI keeps the primary resource lifecycle shallow (`gh repo list`,
  `create`, `view`, `delete`) but introduces a child group when that child is a
  resource with its own lifecycle (`gh repo deploy-key list|add|delete`,
  `autolink create|list|delete`).
- Kubernetes groups stateful configuration operations under `kubectl config`
  instead of making each configuration operation a top-level verb.
- Gemini CLI uses namespaces only for coherent command families, including
  extension and agent management; it does not create a hierarchy merely to
  mirror source-file or transport layout.

Candidate operator-facing tree (proposal, not an implementation decision):

```text
ayx one workspace
  list | create | delete | current | detail | use
  config
    get | set | schema | reset
  members
    list | admins | invite | reinvite | remove | suspend | unsuspend | update | invitation-link
  groups
    list | create | update | delete
    members add | members remove
    roles set
  cloud-configs
    list | create | update
  transfer
    start | assets
```

Design constraints:

- `workspace` remains the active One context, and its primary
  list/current/detail/use lifecycle stays shallow. Prefer `use` over
  `switch` if we make the naming change; the active workspace selector remains
  `--workspace`.
- Use **members** for workspace-scoped people until the separate
  `one person` versus `workspace people` consolidation decision is made. Do
  not introduce a third overlapping `users` list first.
- Make the active workspace implicit for `config get`, `members list`, and
  similar commands. An explicit `--workspace` selects another workspace;
  eliminate paired `current-*` / `*-v4` command names from the human surface.
- Remove `groups-global` from the operator-facing surface unless a live,
  supported use case demonstrates a meaningful result that `groups` cannot
  return. It is a direct upstream `GET /v4/groups` call, not an AYX loop over
  workspaces, but the transport sends the active
  `x-alteryx-workspace-gid` context header to both it and
  `GET /v4/workspaces/{id}/groups`. Windows testing against workspace `91946`
  returned the identical group from both endpoints. Do not preserve a vague
  "global" name merely because the underlying API has a second route.
- Preserve API version and endpoint distinctions only in machine metadata,
  diagnostics, and backend docs—not as command names.
- Do not create a vague `assets` group: groups, members, configuration, and
  cloud configurations have distinct operator purposes. `transfer assets` is
  a verb-object operation, not a resource family.
- Treat this as a pre-announcement breaking hierarchy migration, with one
  compatibility decision made deliberately for every old path rather than
  accumulating permanent aliases.

Acceptance work before implementation:

- [ ] Verify each proposed child with the live One API and identify any route
  whose semantics do not fit the group above.
- [ ] Decide `use` versus `switch`, and `members` versus `users`, alongside
  the broader person-surface consolidation; write one naming glossary.
- [ ] Produce a command-to-command migration table, catalog aliases or hard
  removals, help snapshots, completions, docs, and the generated command
  surface in one change.
- [ ] Test a One-only profile and a workspace with both group and membership
  fixtures, including selector/default behavior and all mutation dry-runs.

## Authentication lifecycle and onboarding contract

Priority: release blocker — do not promise durable login where none exists

Windows live testing established that `ayx onboard` offers the same email-OTP
path as `ayx one login`; onboarding is not a distinct durable-auth flow. The
successful OTP result reported `credential_kind: email_otp`, secure storage,
and `has_refresh_token: false`. The code's OTP result contains an access token,
workspace GID, and expiry only; its current lifetime is 2,592,000 seconds (30
days). The OAuth API-token path, by contrast, persists the refresh credential
that can renew access silently.

- [x] Correct all onboarding, login, doctor, help, and README wording so it
  never suggests an OTP-created credential rotates or lasts indefinitely.
  Secure storage protects the time-limited credential; it does not make it
  durable. **Verified live 2026-09-10** -- see the interactive run below.
- [x] Make an explicit product decision for onboarding: either offer/guide the
  durable OAuth API-token path, or label email OTP as a time-limited login and
  state that the user will repeat OTP after expiry. Do not invent a durable OTP
  claim without upstream evidence that the API now returns a refresh token.
  **Decided and shipped:** email OTP stays the onboarding default and is
  labelled a time-limited login; `--oauth-api-token` is named in the same
  breath as the durable alternative.
- [x] Test the two paths separately: OTP persistence and expiry messaging;
  OAuth refresh persistence and silent renewal. `doctor` must describe the
  OTP state accurately rather than treating its intentional lack of a refresh
  token as a malformed configuration. **Both tested:** OAuth API-token
  persistence and silent renewal earlier in Phase 2; OTP below.

### Gate V2.4 — interactive OTP onboarding, 2026-09-10

Run by hand on Windows into an isolated `AYX_CONFIG_HOME`
(`C:\code\ayx-otp-v24`), profile `otp-test`, against the release binary built
from this branch. A real passcode was emailed and entered; the workspace
password was saved to the OS secure store.

`ayx doctor` on the resulting profile:

| Row | Status | Summary |
|---|---|---|
| config | ok | profile 'otp-test' resolved; no inline secrets |
| auth | ok | One auth configured (time-limited login) |
| network | ok | Alteryx One endpoints configured |
| one | ok | One workspace probe succeeded |
| server | skip | Alteryx Server not configured |
| mongo | skip | Mongo not configured; no Alteryx Server in this profile |
| **overall** | **ok** | |

This is the spec's requirement met end to end on a profile created from
nothing: `one_status` is `configured_time_limited`, not `incomplete`; the
credential is described as `email_otp` with `renews_automatically: false` and
guidance naming `--oauth-api-token`; `access_token_expires_at` is reported as
a plain epoch rather than redacted; and an unconfigured Server is a Server-only
`skip` that does not drag the rollup to `fail`.

Two details worth recording because they were open questions:

- **The "inline secrets" caveat did not apply.** `config` reports "no inline
  secrets" and `inline_secret_risks` is empty, because the wizard's default
  put the credential in the OS secure store. The caveat stands only for a
  profile carrying an inline `access_token`, where the warning is correct.
- **The secret-posture slot table is consistent with the auth row.** The
  legacy top-level `one.access-token` slot reads `missing`/`not_configured`
  while `one.workspace.91946.access-token` reads `keyring`/`passed`. That is
  right, not a repeat of the workspace-scoped-credential defect: the top-level
  field genuinely is unset, and the workspace-scoped slot is where a modern
  login writes.

The wizard did not offer `--secret-policy session`, confirming its removal.

### `--browser` and `--device` are hidden, not removed (2026-09-09)

Decision: both flags carry `hide = true` on `ayx one login` as of this date.
They no longer appear in `--help` or in the generated command surface.

Why: **neither flow has ever been run against a live Alteryx One tenant.** This
is an absence of evidence, not a recorded live failure — the live auth-flow
gate was deliberately skipped and no IdP error code was captured. What is known
from the code is concrete enough to act on:

- `ayx-rs/src/cmd/one_platform/auth.rs` derives the device authorization
  endpoint by **string substitution on the token endpoint**, with no OIDC
  discovery lookup and no verification that the endpoint exists. If the tenant
  does not happen to serve `/device_authorization` at the substituted path, the
  flow 404s before it reaches the identity provider.
- Both grants must additionally be enabled on the Alteryx OAuth client:
  `device_code` for `--device`, `authorization_code` plus a registered
  `http://localhost:<port>` redirect URI for `--browser`.

A flag advertised in help that fails at the identity provider is worse than no
flag: the operator burns an afternoon concluding the failure is theirs, and
every agent reading the help surface treats it as a supported capability.

Hidden rather than deleted. The code reads as a conventional OAuth
implementation and would become usable if those grants were enabled upstream,
but that is a reading of the source, not a tested claim -- by the same
paragraph, it has never been run. It stays reachable for re-testing by typing
the flag.

To reopen this decision:

1. Run `ayx one login --browser` and `ayx one login --device` against the
   reference tenant. Record the CLI version, tenant region, date, the exact
   error text, and any IdP error code (`unauthorized_client`,
   `invalid_redirect_uri`), plus whether the derived `/device_authorization`
   endpoint existed at all. Redact every credential value.
2. If either works: remove its `hide = true`, replace the string-substituted
   device endpoint with an OIDC discovery lookup (falling back to substitution
   only when discovery is unavailable), and document the flow as a durable
   convenience path.

Meanwhile `--oauth-api-token` remains the proven durable path: it persists a
refresh credential and renews access tokens silently, with Windows evidence.

Pinned by `unverified_auth_flows_are_not_advertised` in
`ayx-rs/tests/product_boundary.rs`.

## Windows One read-validation harness

Priority: release evidence and regression protection

`scripts/one-read-sweep.ps1` is the non-mutating Windows regression harness
for an already-authenticated One profile. It carries overridable safe fixture
identifiers, writes only labels/exit codes/timings to a JSON log, and must not
emit credentials. It intentionally does not attempt interactive OTP.

### Live sweep result, 2026-09-10

Run on Windows against the `windows-otp` profile, `--output json`, release
binary built from this branch:

**74 invocations: 72 passed, 2 expected-unprivileged, 0 failed. Exit 0.**

That clears the sweep half of the Phase 2 exit gate. The prior baseline was
71 passed with three classified for follow up; the two `ayx one api` commands
that previously failed before the network call now pass, and the two denials
are classified rather than counted as undifferentiated failures.

**What that number does and does not mean.** The sweep prints these caveats
itself at the end of every run, so this paragraph cannot go stale the way an
earlier version of it did:

- **68 are clean successes** (exit 0).
- **4 passes are an expected error, not a clean result:** `job-groups inputs`
  reports `validation` (the fixture is not a JDBC source), and `job-groups
  profile`, `profile-results` and `pdf-results` report `not_found` (the
  fixture has no profiling data). Each is accepted only against that exact
  `error_code`.
- **2 are expected-unprivileged, and are not counted in the 72:**
  `ayx one workspace detail 91946` and
  `ayx one connections connector-metadata publish-info gsheetsuser`, both
  `permission_denied`.

Those four rows are matched on `error_code`, never on an exit code. Exit codes
are lossy -- `exit_code_for_envelope` maps `NotFound`, `Gone`, `Conflict`,
`RateLimited`, `Network` and `Upstream` all to 6 -- so accepting "exit 6" to
allow an expected not-found would also silently accept a connection reset, a
502 and a 429 on those rows. An earlier revision of this harness did exactly
that; it is fixed and covered by a stub test.

- [x] `ayx one workspace detail 91946` received a live 403
  `AccessControlException`. This is a real permission boundary, not evidence
  that the endpoint path is wrong. **Done (2026-09-09):** the sweep now
  classifies it rather than counting it as an undifferentiated failure. Both
  halves of the original item are available:
  - `-PermissionBoundary` marks the command. A denial there is recorded as
    `expected_unprivileged` — a third outcome, deliberately **not** folded
    into the pass count, because the sweep must never report success for a
    request the tenant refused. It does not fail the run.
  - The classification applies only when the CLI itself reports the denial:
    `error_code: permission_denied`, or exit 5 when no envelope was produced
    at all. It does **not** read prose. An earlier revision also matched
    `\b403\b` and words like "forbidden" anywhere in the output, which meant a
    timed-out request whose body happened to carry `elapsed_ms: 403` was
    classified as an expected denial. Three digits are not a status code.
  - `-AdministratorFixture` asserts the profile is an administrator and
    switches the leniency off, so a denial anywhere counts as a real failure.
    That is the "re-run with an administrator fixture" half.

  **Known limit, by design.** Without `-AdministratorFixture`, a permission
  regression scoped to one of the two boundary commands -- a token scope
  silently lost for that endpoint, a tenant policy change, the wrong workspace
  selected -- reads as `expected_unprivileged` indefinitely. A *global* scope
  loss is still caught, because every other row fails. The sweep prints a
  reminder whenever it accepts a denial with the assertion unset; periodically
  running with `-AdministratorFixture` is the only way to detect the
  single-endpoint case.

  Note also that exit 5 covers `402` and `451` as well as `403`
  (`ayx-core/src/envelope.rs`): a plan-tier or policy refusal reads the same
  way. All three are entitlement boundaries, so the classification holds, but
  the console wording says "access denied" rather than naming the status.

  The summary log is `ayx.one-read-sweep.v2`, adding `status`,
  `permission_boundary`, `reported_error_code` and `expected_error_code` per
  result, and `total` / `expected_unprivileged` / `administrator_fixture` to
  the header. Verified both against stub binaries -- expected denial,
  administrator assertion, a non-denial failure at a boundary row, a network
  outage on an expected-error row, and a timeout carrying `403` in its body --
  and against the live tenant.
- [x] `ayx one api status` and `ayx one api diagnose` failed before the
  network call because they require `api/server_api`; this is the separate
  One-versus-Server defect recorded below. Fixed in `d28f295` / `9770be2`;
  both pass in the live sweep above.
- [ ] **Decide how the single-endpoint permission regression gets caught.**
  The known limit above means a sweep without `-AdministratorFixture` exits 0
  on a denial at a boundary command, indefinitely. That is arguably the right
  default for an unprivileged operator, but then *some* scheduled run must
  pass `-AdministratorFixture` against an administrator profile, or the
  single-endpoint case is never detected. Nothing runs the sweep on a schedule
  today, so this is a decision about the release checklist (an admin-fixture
  run required before each release) rather than a CI switch.
- [ ] Keep the sweep as the Windows release gate after every change touching
  onboarding, One dispatch, profiles/credentials, output, or command help.
  Pair it with the interactive OTP scenario in the preceding authentication
  section; neither test alone covers the other. (Ongoing by design. Both halves
  were run for this branch: the sweep result above, and Gate V2.4.)

- [x] **A 400 that means "no such data" is reported as `validation`, which
  sends the operator to the wrong place.** Fixed 2026-09-10:
  `ErrorCode::from_http_status_with_body` refines a 400 to `NotFound` when the
  upstream body names an absent resource, and `one_http_envelope` passes the
  parsed body it already held. The refinement is deliberately narrow -- only
  400, only towards `NotFound`, only on an exception name ending in
  `NotFoundException` -- the upstream exception *type* and nothing else -- so a
  genuine 400 stays `validation`. An adversarial review showed the first
  version also scanned prose for "not found", which reclassified real input
  errors such as "Required parameter 'flowId' not found in request body"; that
  scan is gone. Live proof of both directions on the same fixture:
  `job-groups profile|profile-results|pdf-results` now report `not_found`,
  while `job-groups inputs` correctly stays `validation`, because its body
  says "Only Jdbc sources have connect String" -- a real caller-side problem.
  The `NotFound` remediation no longer advises listing the family for
  sub-resource reads, where the id is usually valid. The sweep matches those
  rows on the exact `error_code`, not on an exit code, because exit 6 also
  covers Gone, Conflict, RateLimited, Network and Upstream. Original detail: Found while classifying the sweep,
  2026-09-09. `ayx one job-groups profile 4087561` exits 2 with
  `error_code: validation`. The upstream response is HTTP 400 carrying
  `ProfilingDataNotFoundException` / "Job group 4087561 does not have
  profiling data" — an absent-resource condition, not malformed input. The
  CLI maps 400 to `Validation` unconditionally, and `validation`'s remediation
  tells the user to check their flags and `--help`, which cannot help here.
  This is the same misdirection `mapped_client_errors_land_in_an_actionable_bucket`
  in `ayx-core/src/envelope.rs` already guards against for 408/423/428.
  Consider consulting the upstream exception name, or the response body, when
  a 400 is classified. Affects `job-groups inputs`, `profile`,
  `profile-results`, and `pdf-results` on a fixture with no profiling data.

- [x] **Two `job-groups` subcommands report the wrong `command` in their
  envelope.** Fixed 2026-09-10. The cause was broader than the two commands
  that exposed it: `job_groups_descriptor` shared one name across whole groups
  of leaves, so `inputs`, `outputs`, `jobs` and `publications` all reported
  `one.job-groups.list`, and `status`, `profile`, `profile-results` and
  `pdf-results` all reported `one.job-groups.detail`. Eight commands were
  misnaming themselves, not two. The same pattern in
  `output_objects_descriptor` (`inputs` reporting `one.output-objects.list`)
  is fixed with it. The view *kind* is still legitimately shared; only the
  name is now per-leaf. Pinned by
  `every_job_group_leaf_reports_its_own_command_name`, which fails on any
  duplicate rather than only on the two that were found. Original detail: Found the same way. `ayx one job-groups profile <id>` emits
  `"command": "one.job-groups.detail"`, and `ayx one job-groups inputs <id>`
  emits `"command": "one.job-groups.list"`. The envelope's `command` field is
  part of the machine-readable contract, so an agent correlating a failure
  back to the invocation that caused it is told the wrong one.

## Windows onboarding and credential setup

Priority: release follow-up

Windows validation of `v0.20.5` found these onboarding paths:

- [x] Ship the tested fix for a bare One workspace GID. **Done** in `33e9a74`,
  merged into `integration/phase-1`; covered by
  `bare_workspace_gid_prompts_for_base_url_before_offering_login` and
  `onboarding_repairs_an_existing_bare_gid_profile_without_overwriting_it`
  (`ayx-rs/tests/onboard_login_offer.rs`).
  - A GID alone cannot identify its regional endpoint. The wizard must request
    an explicit regional base URL before offering OTP login, with a visible US1
    default rather than silently assuming a region.
  - An existing incomplete profile must be repairable without being replaced by
    a new default profile.
- [x] Fix bare relative `ayx onboard --profile <file>` paths on Windows.
  **Done** in the same commit (`ayx-core/src/sensitive.rs`); covered by
  `onboard_accepts_a_bare_profile_filename`.
  - The sensitive-file writer must treat an empty parent path as the current
    directory, not call `create_dir_all(\"\")`.
- [ ] Keep live Windows coverage for the following. Ongoing by design; what has
  live evidence as of 2026-09-10 is marked:
  - Server-only onboarding -- **not run live**;
  - One onboarding with a full workspace URL (**run**, Gate V2.4) and with a
    bare GID (**not run live**; unit-tested only);
  - an email-OTP login into an isolated `AYX_CONFIG_HOME` profile (**run**,
    Gate V2.4);
  - OAuth refresh credential rotation via `ayx one auth diagnose` (OAuth
    persistence and silent renewal were **run** in Phase 2 -- see the
    authentication section -- but no record says `auth diagnose` was the
    command used, so treat that specific check as **not run live**).

### Onboarding wizard redesign (intake 2026-09-10)

The author's verdict on the current wizard: it *describes* the API-token path
in a paragraph of prose instead of walking the user through it, which defeats
the purpose of a wizard. The reference model cited is `openclaw`: a sequence of
guided routines that ask the user to choose, collect values, and run checks
(`openclaw doctor`).

- [ ] **Ask for the auth method.** Prompt for the One credential type, with
  email OTP as the default and the OAuth API token offered as the durable
  alternative, instead of printing the `ayx one login --oauth-api-token`
  paragraph and continuing down the OTP path.
- [ ] **Walk through the API-token path in the wizard.** Prompt for the Client
  ID and the refresh token, say where in the Alteryx One UI each is found, and
  store them the same way `ayx one login --oauth-api-token` does. One code path
  for both entry points, not a second implementation.
- [ ] **Finish with a check.** End the wizard by running the relevant `doctor`
  rows and showing them, rather than printing commands the user may run
  later.
- [ ] **Readable output throughout**, per the palette item under
  [Human-facing output](#human-facing-output-and-redaction). The completion
  block (`onboarding completed` followed by raw fields) is the least readable
  screen in the flow and should become a short, human summary.

Settle the auth-method prompt before the palette: the first changes what the
wizard does, the second only how it looks.

## Profile and credential lifecycle

Priority: medium -- asymmetric lifecycle and a misleading isolation claim

Surfaced during Phase 2 validation, 2026-09-10. Each needs a design decision
rather than a quick fix, which is why none was rushed. Product: shared CLI.

- [ ] **No way to delete a profile.** `ayx onboard` creates profiles and
  `ayx profile` offers `list | current | show | use | path | migrate`; nothing
  removes one. Decide what deletion does to the profile's keyring entries (a
  profile whose secrets outlive it is a leak), to the active-profile pointer
  when the active profile is the one deleted, and to audit artifacts that name
  it.
- [ ] **`ayx secret prune` cannot clean the orphans current versions create.**
  Its help says it targets keyring accounts written by `ayx < v0.11.0`, keyed
  by `profile_name`. Current versions key accounts by credential binding, so
  the orphans a modern user accumulates -- from a deleted or reset config home,
  say -- are exactly the kind it does not handle. Extend it to binding-scoped
  accounts, or document the manual cleanup; this interacts with profile
  deletion above.
- [ ] **`AYX_CONFIG_HOME` implies an isolation it does not provide.** Keyring
  account names derive from the credential binding (identity + workspace), not
  from the config folder, so two separate config homes logged in as the same
  identity to the same workspace **share one secret**: re-logging in one
  silently replaces the other's token. Proven on 2026-09-10 -- `ayx-otp-v24`
  and `ayx-win-otp-test` carry byte-identical `access_token_ref` values.
  Decide between documenting the scope of the isolation, warning when a login
  would overwrite a secret another config home references, or folding the
  config home into the binding (a migration). The first is the minimum and
  should not wait for the others.

## Human-facing output and redaction

Priority: high UX correctness

- [ ] Do not redact operational expiry metadata merely because its key contains
  `token`.
  - `access_token_expires_at` and `token_expires_at` are safe status metadata;
    the credential value itself, refresh credentials, passwords, and client
    secrets remain redacted.
- [ ] Render nested response objects and arrays as indented human output for
  text/table modes rather than embedding compact JSON in a terminal row.
  - The canonical machine JSON result must remain lossless after the output
    contract consolidation below; human rendering must never alter it.
  - Table cells holding a nested value should provide a compact count/summary,
    not a serialized JSON blob.
  - Review large diagnostic response bodies after the recursive renderer ships:
    show a concise, safe summary in the default human view and expose the full
    response through an explicit detail/full mode where a probe has many rows.
  - Windows evidence: `ayx one auth status` and `ayx one auth diagnose` return
    success but print the full API-access-token inventory inside
    `workspace_probe` in the default human view. The summary should show probe
    status, endpoint, elapsed time, and count; the inventory belongs behind an
    intentional detail query.
  - Windows evidence: `ayx one workflows tools` and `ayx one job-groups
    outputs` returned sibling collections rather than an `items` wrapper.
    Both now opt in to named collection rendering; a redacted live workflow
    tools read confirmed the two sections render correctly.
- [x] Standardize timestamps in human output at second precision.
  - Preserve the original RFC 3339 value, including fractional seconds, in
    JSON and persisted/audit data; trim only display-only fractional seconds.
  - Cover UTC and offset timestamps, values with no fraction, and malformed
    strings, which must pass through without a rendering failure. Apply this
    consistently to connections and every other human text/table view.
  - Intake 2026-09-10 asks for the same thing ("remove the useless
    microseconds"). The fraction the One API returns is **milliseconds**, and
    it was `.000` on every row in the intake and in re-reproduction. Human
    output should also show a date rather than a raw epoch: the login flow
    prints `Token expires: 1791640201` (`one_platform/auth.rs`).
- [x] **Show `remediation` in human output.** Text now prints the actionable
  summary and suggested commands beneath the result.
- [x] **Colour and structure for human output (intake 2026-09-10).** The
  author asks for a consistent palette: commands highlighted in one colour
  (suggested: cobalt blue), keys in `key: value` blocks in another (suggested:
  gold), and emphasis on IDs, links and key terms; plus pretty-printed JSON
  wherever a nested value is shown to a human. The onboarding completion block
  is the author's worked example of what not to do: `summary:` and `login:`
  print single-line JSON, and `inline_secret_fields:`, `secret_refs:` and
  `warnings:` print as empty labels. Colour must respect `NO_COLOR` and a
  non-terminal stdout, and must never reach `--output json`. The author also
  asked what is needed to pin down "visually clean and human readable": agree
  a reference -- a mock-up of three representative screens (a list, a detail,
  an error), approved before implementation -- rather than iterating on
  adjectives. The delivered terminal palette is TTY-only and respects
  `NO_COLOR`; JSON and non-TTY output remain unstyled.

## Output contract consolidation

Priority: high — one machine contract, one human experience

**Completed in Phase 3.** `--output json` is the single canonical,
recursively redacted envelope. `json-full` and `--error-format` are retired;
text/table remain the bounded human projection. Regression coverage pins
redaction, upstream payload truth, schema validity, remediation, timestamps,
and named collections.

## Redaction model simplification

Priority: security correctness and operator trust

The current recursive key-substring heuristic treats labels containing words
such as `token` as secret-bearing by default. It has already redacted harmless
operational metadata (`access_token_expires_at`) and requires growing exception
lists as APIs evolve. Secret handling and presentation projection must be
separate concerns.

- [ ] Replace substring-key heuristics with typed/explicit sensitivity at the
  point a value enters the CLI envelope. Credentials, passwords, client
  secrets, authorization headers/cookies, credential-bearing URIs, and secret
  parameter values must be non-displayable by construction.
- [ ] Maintain a small exact-key/value detector only as defense in depth for
  untyped upstream JSON (for example `Bearer `, JWT-shaped values, and known
  credential URI syntax). It must never redact a field merely because its name
  contains `token`, `secret`, or another broad substring.
- [ ] Treat expiry, token type/source, credential health, workspace identity,
  request IDs, and safely selected claims as operational metadata. Test both
  positive metadata cases and negative secret-leak cases in text and JSON.
- [ ] Apply one redaction policy before every output renderer and audit writer;
  no output format may be an escape hatch for real credentials.

## Multi-environment configuration is undocumented/incomplete

Priority: correctness before promotion workflows

Observed on Windows: `C:\\Users\\ryan.merlin\\AppData\\Roaming\\ayx` contains
`profiles\\` and `state.yaml`, but no `environments.yaml` or `workspaces\\`
directory. The README says `environments.yaml` is canonical and that
`--environment` selects it for a normal run, but the runtime loader resolves a
central profile from `profiles\\` and only applies an environment selection if
the loaded file is already a workspace-shaped configuration.

- [ ] Choose and implement one authoritative model:
  - central config-home workspaces (`workspaces\\<name>.yaml`, selected in
    state or `AYX_WORKSPACE`), or
  - an explicit project-local workspace file passed to every relevant command.
  Do not imply both are automatic.
- [ ] Make `--environment` either load the selected workspace model for normal
  runtime commands or fail clearly when no workspace is selected; it must not
  silently load an ordinary profile and ignore the requested environment.
- [ ] Align `ayx onboard --environments`, `tools workspace init`, `tools
  workspace resolve`, profile resolution, README, CLI spec, and generated
  help. Until then, remove the README claim that the user should find a
  canonical `environments.yaml` in config home.
## Alteryx One connections: classification and governance

Priority: discovery followed by product-led UX work

The current connection surface exposes raw `/v4/connections` payloads and has
create-template fields such as `type`, `vendor`, `credentialType`, `isGlobal`,
and `ssl`. Make the list and detail views useful for managing a real estate of
connections without guessing from a connector name.

- [ ] Live-inventory the fields reliably returned by connection list and detail
  endpoints, redacting parameter values and all credentials.
  **Started 2026-09-10** on one connection (`44865`, BigQuery), `detail` only.
  Present upstream and dropped by the human view: `type` (`jdbc`), `vendor` /
  `vendorName` (`bigquery`), `credentialType` (`apiKey`), `creator.id`,
  `updater.id`, `associatedPeople`, `isGlobal`, `credentialsShared`,
  `hasCredentials`, `ssl`, `sshTunneling`, `workspace.id`. Absent upstream:
  any owner *name* (IDs only), a share count (derivable from the permissions
  endpoint), and any last-accessed or last-used time. Still to do: the `list`
  payload, and more than one connection type.
- [ ] **What the author needs on screen (intake 2026-09-10).** `connections
  list`: owner ID and owner name (populated, not blank), and the connection
  type -- what it connects to (BigQuery, Snowflake, GCS, ...). `connections
  detail`: source type, owner, how many subjects it is shared with, and when
  it was last accessed or run. The first three are available today: the type
  and vendor fields above, the owner by resolving `creator.id` to a person,
  and the share count from `permissions`. "Last used" is not reported by the
  connection API at all; the only route is a join across job runs, whose
  `location` embeds a `connectionId`. That is a heuristic, must be labelled as
  one, and belongs in the governance view rather than `detail`. The same
  applies to "how many workflows use it", which the author asked for on
  `permissions list`.
- [ ] Define a stable human-facing connection classification. Start with:
  connection type (for example JDBC, file/storage, SaaS), connector/vendor,
  credential method, owner/scope, shared/global state, health/status, and
  lifecycle timestamps where supplied.
- [ ] Add a governance view after the field inventory: ownership, people/groups
  with access, share policy, unused/stale/failed candidates, and connector or
  secret rotation signals where the API supports them. It must distinguish
  facts reported by the service from heuristic findings.
- [ ] Keep a concise connection list useful at a glance, and put expansive
  metadata in `detail` or an explicit governance/inventory command. Add
  fixtures for missing or partially populated fields.

## Designer Cloud / Trifacta-derived flow surface

Priority: product-boundary decision

`ayx one flows` is the integer-id `/v4/flows` Designer Cloud surface, not the
ULID-keyed cloud-native Alteryx One `/svc-workflow` surface exposed as
`ayx one workflows`.

- [ ] Stop promoting `ayx one flows` in README quick-start examples. The
  global `--output` help text does it too: its placement example is
  `ayx one flows list --output json`, shown under every command's `--help`.
- [ ] Decide support policy with product owners: remove the surface, or retain
  it behind a non-default `legacy-flows` Cargo feature.
  - A real feature gate must cover the Clap command and dispatch, catalog and
    generated command surface, API inventory/helpers/types, tests, and feature
    propagation from `ayx-rs` to `ayx-one-api`.
  - Do not create a misleading one-file gate that leaves legacy routes or
    commands compiled and documented by default.
