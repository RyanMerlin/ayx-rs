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

### Prepared, independently tested changes

These are not automatically approved for merge; each needs a conflict review
against the current integration base.

- `b25b1b7 fix(cli): wrap help descriptions` sets a 120-column clap help
  width, improves `--no-input` wording, and adds a help-wrap smoke test.
  `cargo test -p ayx-rs --test cli_smoke` passed (65 tests).
- `1a91ab6 fix(one): remove retired person count` removes `ayx one person
  count` across CLI, inventory, catalog, docs, and tests. The CLI smoke suite
  (65 tests), One API inventory tests (3 tests), formatting, and generated-doc
  checks passed.
- The bare-GID onboarding fix and human-output/redaction work exist as local,
  uncommitted experiments. Re-review their scope against phases 2 and 3; do
  not merge them wholesale merely because they passed an earlier local test.

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

- [ ] Remove `--error-format` with the `json-full` consolidation. `--output
  json` already renders both success and failure as JSON envelopes; the flag
  only enables the confusing asymmetric case of human success output with a
  JSON error. Output format must be one coherent invocation-level contract.
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

- [ ] Remove `ayx one person count` now, with no compatibility alias. The
  vendor endpoint `/v4/people/count` is already retired (HTTP 410 / the
  `IAM_SCREAM_PEOPLE` removal); it must disappear from the CLI, inventory,
  catalog, generated command surface, README, tests, and current docs.
- [ ] Consolidate the duplicate people surfaces. `ayx one person list` and
  `ayx one workspace people` both issue workspace-scoped `GET /v4/people` and
  returned the same 18 ids in Windows validation.
  - Decide whether the remaining resource is `workspace people` (best matches
    the actual membership scope) or `workspace users` (more conventional
    operator language); retain a narrowly scoped self-identity command rather
    than a second membership list.
  - Do not rename to `users` merely as an alias while two overlapping command
    trees and inconsistent payload terms remain.
- [ ] Replace the `job-groups` user-facing namespace with a clear Jobs
  surface only after migrating to the supported Job Library contract.
  - The live OpenAPI marks `GET /v4/jobGroups` deprecated and directs callers
    to `/v4/jobLibrary`; the current list/count already use Job Library, but
    detail/run/status operations retain job-group endpoints.
  - Design the hierarchy so a parent `jobs` command does not create the absurd
    `jobs jobs` child. Preserve backend `jobGroup` only in transport metadata
    and documentation of legacy endpoints.
- [ ] Research whether `/v4/outputObjects` is a current One resource or a
  Designer Cloud/Trifacta-derived surface before renaming it. The endpoint
  worked in Windows validation but returned zero items; it is not a credible
  quick-start example. If retained, use an operator-facing plural name such
  as `outputs` only after its lifecycle and relationship to workflows are
  documented.
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

- [ ] Make root `doctor` report One, Server, and Mongo as independent domains.
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

- [ ] Rewire these commands to use the One configuration and endpoints, or
  move/rename them under the actual Server API surface if their intent is
  Server-only. Do not retain an ambiguous hybrid command.
- [ ] Add a One-only integration test proving every `ayx one api ...` command
  either succeeds against One or fails with a clearly One-owned error.
- [ ] Update help, catalog, README, and testing documents to name the owning
  product explicitly.

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

- [ ] Correct all onboarding, login, doctor, help, and README wording so it
  never suggests an OTP-created credential rotates or lasts indefinitely.
  Secure storage protects the time-limited credential; it does not make it
  durable.
- [ ] Make an explicit product decision for onboarding: either offer/guide the
  durable OAuth API-token path, or label email OTP as a time-limited login and
  state that the user will repeat OTP after expiry. Do not invent a durable OTP
  claim without upstream evidence that the API now returns a refresh token.
- [ ] Test the two paths separately: OTP persistence and expiry messaging;
  OAuth refresh persistence and silent renewal. `doctor` must describe the
  OTP state accurately rather than treating its intentional lack of a refresh
  token as a malformed configuration.

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

Hidden rather than deleted — the implementation is correct OAuth and becomes
usable the moment those grants are enabled upstream. It stays reachable for
re-testing by typing the flag.

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

### Live sweep result, 2026-09-09

Run on Windows against the `windows-otp` profile, `--output json`, release
binary built from this branch:

**74 invocations: 72 passed, 2 expected-unprivileged, 0 failed. Exit 0.**

That clears the sweep half of the Phase 2 exit gate. The prior baseline was
71 passed with three classified for follow up; the two `ayx one api` commands
that previously failed before the network call now pass, and the two denials
are classified rather than counted as undifferentiated failures.

The two expected-unprivileged rows are `ayx one workspace detail 91946` and
`ayx one connections connector-metadata publish-info gsheetsuser`. Both exit 5
(`permission_denied`). Neither is counted as a pass.

Four rows pass while carrying a declared non-zero exit code of 2
(`job-groups inputs`, `profile`, `profile-results`, `pdf-results`). See the
open finding below before reading `72 passed` as 72 clean successes.

- [x] `ayx one workspace detail 91946` received a live 403
  `AccessControlException`. This is a real permission boundary, not evidence
  that the endpoint path is wrong. **Done (2026-09-09):** the sweep now
  classifies it rather than counting it as an undifferentiated failure. Both
  halves of the original item are available:
  - `-PermissionBoundary` marks the command. A denial there is recorded as
    `expected_unprivileged` — a third outcome, deliberately **not** folded
    into the pass count, because the sweep must never report success for a
    request the tenant refused. It does not fail the run.
  - The classification applies only when the output is genuinely a denial
    (`403`, `AccessControlException`, `forbidden`, `permission denied`,
    `not authorized`). Any other failure at the same command is still a
    failure, and says so: a network outage there cannot hide behind the flag.
  - `-AdministratorFixture` asserts the profile is an administrator and
    switches the leniency off, so a denial anywhere counts as a real failure.
    That is the "re-run with an administrator fixture" half.

  The summary log is now `ayx.one-read-sweep.v2`, adding `status` and
  `permission_boundary` per result and `total` / `expected_unprivileged` /
  `administrator_fixture` to the header. Verified against a stub binary in
  three cases — expected denial, administrator assertion, and a non-denial
  failure at the same command — since a live tenant was not available.
- [ ] `ayx one api status` and `ayx one api diagnose` failed before the
  network call because they require `api/server_api`; this is the separate
  One-versus-Server defect recorded below.
- [ ] Keep the sweep as the Windows release gate after every change touching
  onboarding, One dispatch, profiles/credentials, output, or command help.
  Pair it with the interactive OTP scenario in the preceding authentication
  section; neither test alone covers the other.

- [x] **A 400 that means "no such data" is reported as `validation`, which
  sends the operator to the wrong place.** Fixed 2026-09-10:
  `ErrorCode::from_http_status_with_body` refines a 400 to `NotFound` when the
  upstream body names an absent resource, and `one_http_envelope` passes the
  parsed body it already held. The refinement is deliberately narrow -- only
  400, only towards `NotFound`, only on an exception name ending in
  `NotFoundException` or a human-readable field saying "not found" -- so a
  genuine 400 stays `validation`. Live proof of both directions on the same
  fixture: `job-groups profile|profile-results|pdf-results` now report
  `not_found` with the remediation "list the family to find a valid one",
  while `job-groups inputs` correctly stays `validation`, because its body
  says "Only Jdbc sources have connect String" -- a real caller-side problem.
  The sweep's declared exit codes moved from 2 to 6 for the three that
  changed. Original detail: Found while classifying the sweep,
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

- [ ] Ship the tested fix for a bare One workspace GID.
  - A GID alone cannot identify its regional endpoint. The wizard must request
    an explicit regional base URL before offering OTP login, with a visible US1
    default rather than silently assuming a region.
  - An existing incomplete profile must be repairable without being replaced by
    a new default profile.
- [ ] Fix bare relative `ayx onboard --profile <file>` paths on Windows.
  - The sensitive-file writer must treat an empty parent path as the current
    directory, not call `create_dir_all(\"\")`.
- [ ] Keep live Windows coverage for:
  - Server-only onboarding;
  - One onboarding with a full workspace URL and with a bare GID;
  - an email-OTP login into an isolated `AYX_CONFIG_HOME` profile;
  - OAuth refresh credential rotation via `ayx one auth diagnose`.

The current local implementation and regression coverage are on branch
`fix/onboard-bare-workspace-base-url`; it needs normal PR/release handling.

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
    outputs` returned successful responses that the human renderer labeled an
    "unrecognized collection shape." Add typed renderers or an explicit
    human-readable fallback before these become promoted examples.
- [ ] Standardize timestamps in human output at second precision.
  - Preserve the original RFC 3339 value, including fractional seconds, in
    JSON and persisted/audit data; trim only display-only fractional seconds.
  - Cover UTC and offset timestamps, values with no fraction, and malformed
    strings, which must pass through without a rendering failure. Apply this
    consistently to connections and every other human text/table view.

## Output contract consolidation

Priority: high — one machine contract, one human experience

Current behavior has two JSON dialects: `--output json` emits the versioned
`ayx.output.v1` compact projection (selected fields, summarized nested values,
and a default 20-row cap), while `--output json-full` emits the complete
recursively redacted envelope. The original rationale was bounded agent
payloads and insulation from upstream transport-wrapper drift, but the split
makes `json` unexpectedly incomplete for an agent and creates a second
machine-facing contract to maintain.

- [ ] Make `--output json` the single canonical machine output: the full,
  recursively redacted envelope with stable outer fields (`ok`, `message`,
  `timestamp_utc`, `data`, and `error_code` on failure).
- [ ] Remove `--output json-full` from the public CLI in the next
  pre-announcement release rather than carrying a deprecation alias. There is
  no announced external automation compatibility commitment; update internal
  tests, docs, skills, and the generator in the same change.
- [ ] Keep default `text`/`table` as the human output path: readable,
  formatted/colorized where a terminal supports it, with safe concise views
  for large diagnostics.
- [ ] Replace format-level truncation/projection with explicit command-level
  summary/detail controls and normal list pagination. An agent must never need
  a second output format merely to retrieve fields omitted by the first.
- [ ] Update CLI spec, README, schema/tests, shell completions, generated help,
  and migration/release notes together; add compatibility tests for the alias
  only if an external compatibility commitment appears before the change; add
  full-data-preservation tests in all cases.

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

- [ ] Stop promoting `ayx one flows` in README quick-start examples.
- [ ] Decide support policy with product owners: remove the surface, or retain
  it behind a non-default `legacy-flows` Cargo feature.
  - A real feature gate must cover the Clap command and dispatch, catalog and
    generated command surface, API inventory/helpers/types, tests, and feature
    propagation from `ayx-rs` to `ayx-one-api`.
  - Do not create a misleading one-file gate that leaves legacy routes or
    commands compiled and documented by default.
