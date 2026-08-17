# Changelog

## Unreleased

### Added

- **`WorkspaceCredential` gains `sp_client_secret` / `sp_client_secret_ref`, so a profile holding several One workspaces can carry a distinct service-principal secret per workspace instead of sharing one profile-level value.** Resolution order: workspace-level dedicated field → profile-level dedicated field → shared `client_secret`. The fallback to the shared field is deliberate, not incidental — profiles written before this change carry only `client_secret`, and without the fallback they would stop authenticating on upgrade. As with the existing `*_ref` fields, `sp_client_secret_ref` may hold an inline secret when no scheme prefix is present, so callers must not print it blind.

### Changed

- **BREAKING (agent-facing JSON)** — HTTP `410` now classifies as `error_code: "gone"`, not `"not_found"`. v0.15.0 deliberately collapsed `410` into `not_found`, reasoning that "for a caller the outcome is identical to 404." Live evidence now shows that reasoning was wrong: `GET /v4/people/count` returns `410 GoneException` with `code: IAM_ENDPOINT_SCREAM_TEST`, `flagName: IAM_SCREAM_PEOPLE` — a deliberate, in-progress vendor withdrawal, not a missing resource. Collapsing the two let that withdrawal sit misfiled as a permissions/not-found gap for weeks. Anything branching on `error_code` to distinguish "never existed" from "existed and was deliberately removed" must add a `gone` arm; an exhaustive match on the prior value set will now fail to compile or fail a runtime `_ => unreachable!()`-style arm.

### Removed

- **`ayx one billing current-account`, `ayx one billing usage-export`, and `ayx one doctor billing`** (plus the billing probe inside `ayx one doctor discover`). All `/billing/v1/*` paths return live `404 RouteNotFoundException`, and `GET /v4/open-api-spec` (172 paths) contains no billing, usage, credit, license, or quota route at all. Unlike the plans/scheduling repoint below, this isn't a wrong-path bug with a fix available — there is no `/v4` equivalent to point at. The spec is not entitlement-filtered (this tenant lacks the Plans entitlement, yet 22 `/v4/plan*` paths still appear in its spec), so billing's absence is a genuine absence, not a tier gate the old `--help` text's "requires an enterprise-tier workspace" guess implied. Same reasoning as the 0.13.0 removal of `one auto-insights` / `one desktop-exec`: removed rather than shipped as commands that can only fail, and it can return when the underlying surface exists. The research is preserved in `docs/one-endpoint-matrix.md` and `docs/one-api-surface-audit.md`.

### Fixed

- **`ayx one datasets list` never worked.** `GET /v4/datasetLibrary` declares the query parameter `datasetsFilter` as `required: true`, and the CLI never sent it, so every call failed with `400 ApiValidationFailed`. Adds `--datasets-filter`, defaulting to `all` so the bare command works, accepting the spec's enum (`all`/`imported`/`reference`/`recipe`) as either a single value or a list. The parallel `/v4/datasetLibrary/count` declares the same parameter `required: false`, so it stays optional there and is omitted rather than silently defaulted.

- **Seventeen `one` endpoints pointed at base paths that exist in no API spec and returned live `404 RouteNotFoundException`.** They had been recorded as "tier-gated" — a tenant-entitlement gap the CLI correctly tolerated. That reading was wrong: the paths were simply incorrect. `GET /v4/open-api-spec` (172 live paths) documents the real routes, and they are what the Alteryx One web UI actually calls (confirmed against browser HAR capture). `/plans/v1/plans*` → `/v4/plans*` (10 rows), `/scheduling/v1/schedules*` → `/v4/schedules*` (5 rows), and the `/iam/v1/workspaces/{id}/people/{,un}suspend` pair → their `/v4/workspaces/...` equivalents (2 rows). One target had no direct equivalent: `GET /plans/v1/plans/{id}` (`one plans detail`) is merged onto the already-wired `GET /v4/plans/{id}/full` (`one plans full`), since the spec defines `/v4/plans/{id}` with `DELETE`/`PATCH` only. The "tier-gated surface" reading that had been recorded for these paths — and for `/billing/v1` alongside them — was a misdiagnosis, not tenant entitlement.

- **`ayx telemetry permissions workflows`/`summary` dispatched a dead `/iam/v1/workspaces/{id}/people` path since inception.** Found while live-verifying the billing removal above — the identical class of bug the 17-endpoint repoint fixed, missed there because this command is deliberately carved out of the `one`-surface inventory (it's a `telemetry` command, not a `one` command) and every path-correctness gate in the repo is built from that inventory. Repointed to the real, spec-documented `/v4/workspaces/{id}/people`. Live-verified: `workflows` went from a hard `error_code: not_found` failure to returning real data; `summary`'s `workspace_member_count` went from a silent `null` (the "failed lookup is unknown, not zero" fallback masking the dead path) to a real count.

### Deprecated

- **`ayx one person count` is being withdrawn by the vendor, not failing on our side.** Verified live: `410 GoneException`, `code IAM_ENDPOINT_SCREAM_TEST`, `flagName IAM_SCREAM_PEOPLE`. It is also absent from the live `/v4/open-api-spec`'s 172 paths — de-documented, scream-tested, and `410` all point the same direction. The command now warns on use and points at `one person list`; it still functions and is not removed.

## 0.15.0 — 2026-08-13

### Added

- **`ayx one workflows` — a real CLI surface for Alteryx One cloud-native (canvas) workflows.** These are not `one flows`: `flows` is the Designer Cloud `/v4/flows` family keyed by integer ids, while cloud-native workflows are the Alteryx One canvas product, keyed by ULIDs and served by a separate `/svc-workflow/api/vN` service — a workspace can hold dozens of cloud-native workflows while `GET /v4/flows` returns zero items. `list`, `count`, `assets`, `detail`, `dependencies`, `engines`, `tools`, `copy`, and `share` are wired. `detail` and `count` are synthesized client-side (the API exposes no per-id or count route) and say so via `detail_source`/`count_source`, so a caller can tell client-side assembly from a server lookup. `share` resolves `--to-person` email addresses to person ids before building its body; its request shape is not in any published spec and was recovered from the service's own schema-validation errors. Authoring is still out of scope — no public endpoint accepts arbitrary visual workflow logic.
- **`ayx one api coverage` now reports wired endpoints that lie outside the spec's namespace.** The One gateway spec documents `/v4` only, but the CLI also speaks `/svc-workflow`, `/plans/v1`, `/scheduling/v1`, `/billing/v1`, and `/iam/v1`. Those 25 rows (a sixth of the wired surface) were previously dropped from the report entirely — never covered, never missing, never stale — while `inventory_operations` reported 123 for a 150-row inventory. They now appear under `outside_spec_namespace` alongside the commands that dispatch them, `inventory_total` counts every distinct wired row, and `coverage_pct` is documented as scoped to the comparable namespace rather than reading as "percent of the One surface covered". `--check` still gates on `missing` only: these endpoints are unverified by the diff, not drift.
- **`docs/one-endpoint-matrix.md`** — a per-endpoint matrix recording live status, verification date, dispatching command, response shape, and error-body flavor for every wired One endpoint, gated by a test that fails when a newly wired endpoint lands without a matrix row.

### Fixed

- **A deleted resource no longer reports as an internal error.** Alteryx One answers `410 GoneException` when you read back an object that existed and was removed (found during a live flow create/read/update/delete cycle). `410` was not in the status-classification table, so it fell through to the catch-all and surfaced as `error_code: "internal"` — telling an agent branching on the code that `ayx` had malfunctioned and the call was worth retrying, when the object is gone and the answer will never change. `410` now classifies as `not_found`, matching `404`. Also mapped, each to a bucket a caller can act on: `402`/`451` to `permission_denied` (entitlement and policy denials, not malformed input), `412`/`423`/`428` to `conflict` (precondition and lock failures are state conflicts, like `409`), and `408` to `network` (a request timeout may succeed on retry; `validation` would send the caller to check their flags). `405`, `415`, `426`, and `431` deliberately stay `internal`: for a CLI those almost always mean `ayx` built the request wrong — wrong method, wrong content type, oversized header — which is what `internal` means.
- **Server API failures now carry the classification they already computed.** `ayx-server-api` bails with `error_code=<code>` embedded in its message, derived from the same status table, and its comment claimed the outer dispatcher picked that up. It did not: the dispatcher scanned the message prose for `"not found"` with a space while the embedded token is `not_found` with an underscore, so a Server-side `404` was classified correctly only when the response body prose happened to say "not found", and a `410` matched nothing and fell through to `internal`. The dispatcher now parses the structured token first and only falls back to prose matching when there isn't one.
- **Transport failures are no longer masked as empty successes.** A failed One list request could read back as a successful, empty one, making a network, auth, or gateway error indistinguishable from "this workspace has no items".
- **`one connections permissions` called a route that does not exist.** All four leaves were wired to `/v4/connections/{id}/permissions`, which the live API answers with `RouteNotFoundException`; the hand-maintained inventory mirrored the same wrong path, so neither `one inventory` nor `one api coverage` could surface it. They now use `/v4/connections/{id}/permissions/sharedSubjects` and `/v4/connections/share`.
- **`one output-objects wrangle-to-python` ran for real without `--apply`**, bypassing the dry-run gate every other mutating One command honors.
- **Release builds no longer ship unsigned macOS binaries under a passing check.** With no `AYX_MACOS_*` secrets configured, the sign and notarize steps skipped silently while the job reported success. The signing posture is now stated in the job summary and raised as a warning annotation; setting the repo variable `AYX_REQUIRE_MACOS_SIGNING=true` makes a missing certificate a hard failure. README and the getting-started page document the Gatekeeper quarantine workaround in the meantime.
- **`ayx one workflows list --all` no longer silently under-delivers.** `/v4/workflows` is a limit-only endpoint with no real cursor — `next_page_token` is always `null` — so the shared paginator fetched exactly one page and reported it as complete. `--all` now requests a generous limit by default (respecting an explicit `--limit` if given), compares the fetched count against the endpoint's own `count` field, and adds `complete: true`/`false` to the envelope plus a stderr warning when the result is short, rather than reporting a partial result as done.
- **`ayx actions export --output json` produced invalid, unparseable output.** A bare `print!` dumped raw YAML to stdout before the JSON envelope was also written, breaking the one-structured-envelope-per-command contract every other command honors. The YAML now lives in the envelope's `data.yaml` field; `save_hint` documents the working `--output json | jq -r '.data.yaml'` extraction path for the fork-and-edit workflow.

### Changed

- **BREAKING (agent-facing JSON)** — `ayx one api coverage --output json` changed shape. `stale[].command` (a string) is now `stale[].commands` (an array): one endpoint can legitimately back several commands (`GET /v4/people/current` serves both `one person current` and `one whoami`), and a single-string field forced those to be either duplicated or misattributed. `coverage_pct` is now nullable and is `null` when the spec contributes nothing comparable, rather than reporting `100.0` for an empty or malformed spec. New fields: `inventory_total` and `outside_spec_namespace`. Anything parsing this envelope must be updated; the underlying `ayx_one_api::EndpointSpec::command` field changed with it.
- The One endpoint drift gate now scans `src/main.rs` in addition to `src/cmd/**`. The `one_doctor_*` and `one_platform_auth_*` dispatchers live in `main.rs` and issue real transport calls that the gate previously could not see.
- All workspace crates declare `repository` and `homepage` metadata.

## 0.14.0 — 2026-07-17

### Added

- **Every bundled action and workflow now carries a machine-readable, validated I/O contract.** `input_schema` and `output_schema` (a JSON-Schema-subset grammar covering `object`/`string`/`array`/`boolean`/`integer`/`number`/`null` types, `required`, `additionalProperties`, `enum`, `const`, `minLength`, `minItems`) can be declared on any `*.action.yaml`/`*.workflow.yaml` entry and are checked at load time — grammar errors, cross-action property disagreements, and missing composed-child placeholders are all rejected before the registry finalizes, not discovered at run time. At run time, a declared input contract is enforced against the caller's `--param` map *before any step or subprocess runs* (unknown parameter, missing required parameter, and enum/const mismatches all fail as a single reported violation set), and a declared output contract is checked against the finished `ActionRun`/`WorkflowRun` record on every run, plan or `--apply`. A workflow validates its own contract once, then filters the caller's params down to each composed action's own declared property set before calling it, so a strict child action never sees (or rejects on) a sibling's key. `ayx actions describe` and `ayx actions workflows explain` expose the effective schema plus `input_schema_source` (`declared` vs. `inferred`) so an agent can fetch a validated contract before constructing parameters. This is additive and v2-compatible — no `schema_version` bump. All 12 bundled actions/workflows (10 actions, 2 workflows) now declare real, load-bearing contracts; a legacy custom action or workflow with no declared schema still runs exactly as before, via a permissive contract inferred from its `<placeholder>` usage.
- **`ayx mongo mutate --apply` and `ayx mongo undo --apply` now execute live** against named, bounded templates from the mutation registry (`knowledge/mongo/mutations.yaml`) — no free-form filter/update, and no template runs until an owner deliberately promotes it from `preview_only` to `executable`. Applying requires `--accept-mutation-risk`, `--backup-audit-artifact` (a current, successful `mongo backup` audit artifact), `--approval-artifact` (the artifact a prior preview run wrote), and `--approve <sha256:digest>` (the digest that preview printed, re-derived and checked against the artifact's stored snapshot at apply time) — all four together, with `--apply` itself; missing pieces are reported all at once, not one at a time. `--yes` is required outside an interactive TTY. Every preview and every apply, success or failure, writes a JSON audit artifact with Mongo connection details (URI, password-file path) always redacted. `mongo undo` reverses an applied mutation via its recorded pre-mutation `$set` values (`guarded_set_inverse`, the only rollback strategy supported); it is guarded, not automatic — it live-checks that every affected field on every candidate document still holds its recorded post-mutation value immediately before restoring, and refuses the entire batch if even one document has drifted. Undo is not a substitute for backup/restore and does not repair a mutation with an unknown transaction outcome.

### Changed

- **`ayx catalog` is now derived from the live `clap` command tree instead of a hand-maintained `COMMAND_SPECS` registry.** `ayx catalog list` defaults to `--scope all` — every visible command, not just the previously curated subset — with `name`, `path`, and `summary` sourced live from clap so a new command can never be silently missing from the catalog. Existing curated entries keep their full `output`/`safety`/`mutating`/`prerequisites`/`notes` classification unchanged; commands with no `CATALOG_METADATA` row show up honestly as `metadata_status: unclassified` (blank `mutating`, `safety: unclassified`) rather than borrowing another command's classification. `ayx catalog list --scope curated` is preserved as the compatibility view for consumers that need only the former, fully annotated projection. `docs/command-surface.md` is regenerated from the all-scope projection and now documents the complete command index (345 commands, up from the prior curated-only ~208).
- **BREAKING** — `ayx workflows` moved to `ayx actions workflows`. No back-compat alias; update scripts and CI that reference the old path. Subcommands (`list`, `explain`, `run`) are unchanged.
- **BREAKING** — `ayx workflow` moved to `ayx designer workflow`. No back-compat alias; update scripts and CI that reference the old path.
- **BREAKING** — `ayx tactics` is now `ayx actions`, and the `tactic` concept is renamed to `action` throughout. This is a full rename of the noun, not just the CLI word, so it breaks three contracts at once:
  - **Agent-facing JSON.** Envelope keys change: `tactics` → `actions`, `tactic_id` → `action_id`, `tactic_count` → `action_count`, `tactics_resolved` / `tactics_missing` → `actions_resolved` / `actions_missing`. Anything parsing `ayx actions ... --output json` must be updated.
  - **On-disk registry files.** The recognized extension is now `*.action.yaml` / `*.action.yml` (was `*.tactic.yaml` / `*.tactic.yml`), and the bundled directory moved from `tactics/` to `actions/`. Custom files under `$AYX_REGISTRY_DIR` or `${AYX_CONFIG_HOME}/registry/` must be renamed to be found.
  - **YAML wire format.** Composition steps change from `- kind: tactic` to `- kind: action`, and a workflow's `tactics:` list key is now `actions:`. The registry schema version is bumped `1` → `2` to mark the break.

  No back-compat alias and no dual-read. A pre-0.14.0 `*.tactic.yaml` in your registry search path is **not loaded** — it is skipped with a warning naming the file, because staying silent would let a bundled action quietly reclaim an id you had overridden (and if your override tightened `safety`, the gate would silently relax). Rename the file and update any `kind: tactic` step to `kind: action`. Subcommands (`list`, `describe`, `resolve`, `run`, `validate`, `export`) are unchanged.

### Fixed

- **`ayx one login`'s email-OTP flow no longer treats a transient network blip or a typo as a full restart.** Every HTTP call in the flow (sendPasscode, validatePasscode, the OIDC redirect chain, the workspace-password submission, the PAT mint) previously had zero retry logic, and a wrong OTP code or wrong workspace password was an immediate, unrecoverable failure — any of these forced starting over from `sendPasscode`, which means a brand-new OTP email every time. Calls with no duplication risk (validatePasscode, the workspace-password POST, read-only lookups) now retry transient network failures and 429/5xx responses; calls with a real side effect (sendPasscode, the PAT mint) retry only when we're confident the request never reached the server, never on an ambiguous timeout or 5xx, so a retry can't send a second OTP email or mint an orphaned second PAT. A wrong OTP gets up to 3 local re-prompts against the same passcode reference before one fresh passcode is sent automatically (capped at 2 sends total); a wrong interactively-typed workspace password gets up to 3 re-prompts (a password sourced from `AYX_ONE_WS_PASSWORD` fails fast instead, since retrying a fixed value that's wrong just wastes requests).

## 0.13.2 — 2026-07-15

### Fixed

- **Windows: successful live commands no longer crash on exit.** `ayx one flows list/count` (and any other command that made a live One API call) printed correct output on Windows and then aborted with `thread local panicked on drop, aborting`, corrupting the process exit code even though the command succeeded. Root cause: `reqwest::blocking::Client` is a thin handle over a background thread running its own tokio runtime; caching it in a thread-local meant its `Drop` (which joins that thread) ran from inside an OS-invoked thread-local destructor callback during process exit (on Windows, via FLS) — fragile, and the actual cause of the abort. Fixed by wrapping the cached client in `ManuallyDrop`, so no destructor is ever registered for it; the client (and its background thread) is intentionally leaked for the life of the process, which is harmless for a short-lived CLI invocation. Added a subprocess-based regression test that exercises this exact path — the only prior test that made a real network round-trip through the compiled binary was gated to a live-credentials-only nightly job that never ran on Windows, which is why this shipped in 0.13.1 undetected.
- **Sensitive-file writes (profiles, onboard config, audit logs) are now atomic and lock-protected.** The shared write helper previously truncated files in place with no temp file, no fsync, and no lock — a crash mid-write could destroy an entire credential profile, and two concurrent writers (e.g. two `ayx one login` runs) could tear the same file. Writes now go through a same-directory temp file, fsync, atomic rename, and an advisory lock on a stable sibling path, so a crash or a concurrent writer can no longer corrupt the target file.
- **The interactive workspace-password prompt no longer echoes to the terminal.** `ayx one login`'s workspace-password step used a plain stdin read, so the password appeared in plain text as it was typed — visible in terminal scrollback, screen recordings, or over-the-shoulder. It's now read via a masked, no-echo terminal read; the `AYX_ONE_WS_PASSWORD` non-interactive path is unchanged.
- **`one flows` vs. `one flows library` help text no longer reads as redundant.** Both had near-identical descriptions ("List One flows." vs. "List the One flow library.") that gave no indication `flows library` is a folder-aware combined view (flows and folders together — its count breaks down by `all`/`flow`/`folder`) while plain `flows` is a flat, folder-less collection. `--help` and the agent-facing command catalog (`ayx catalog list --format full`, `docs/command-surface.md`) now state the distinction explicitly.

## 0.13.1 — 2026-07-14

### Fixed

- **Bare command groups now print real help, not a flat string.** Running any group without a subcommand (`ayx one` and every subgroup, `ayx server`, `ayx sqlserver`, `ayx workflow`, ~35 groups) previously emitted a hand-rolled `"… commands available: a, b, c"` line; it now renders clap's styled Usage/Commands/Options help (Alteryx-blue on a terminal) via `arg_required_else_help`. The only groups that still act on bare invocation are the three with a real default: `ayx doctor` (runs the full suite) and `ayx one token` / `ayx one person` (list).
- **Backfilled `one` subcommand descriptions.** The `one` subtree (deferred from the #103 help backfill pending its redesign) now carries a one-line `about` on every command, sourced from the command catalog, so `ayx one <group> --help` shows an informative Commands table instead of blank rows.

## 0.13.0 — 2026-07-14

### Added

- **Command help backfill across the stable surface** (`#103`): every stable-family command (`server`, `sqlserver`, `mongo`, `doctor`, `tools`, `license`, ...) now carries a one-line `about`, so `--help` no longer renders blank description columns. The `one` subtree is excluded, reserved for its in-flight primitive-first redesign.
- **Affiliation and trademark disclaimer**: the README, a new `NOTICE` file, and the docs site footer now state that `ayx` is an independent, open-source project — not affiliated with, authorized, maintained, sponsored, or endorsed by Alteryx, Inc. — and attribute the project to Ryan Merlin. The `LICENSE` copyright placeholder is filled in (`Copyright 2026 Ryan Merlin`).

### Changed

- **BREAKING — `ayx one` hierarchy dissolved `platform`, primitive-first tree.** Pre-release, no back-compat aliases. `one platform {workspace,role,person,token}` → `one {workspace,role,person,token}`; `one platform auth login` → `one login` (plus a new `one logout` that clears stored Alteryx One credentials from the active profile); `one platform auth {status,diagnose}` → `one auth {status,diagnose}`. The redundant `one platform user` is dropped in favor of a new `one whoami` (equivalent to `one person current`, `GET /v4/people/current`). `one status` and `one platform status` are removed — they were Alteryx Server `api` views, still available at `ayx server api status` — and `one inventory` is now the Alteryx One command-surface inventory (previously `one platform inventory`). `one doctor platform` → `one doctor identity`, and the deprecated hidden `one platform api` alias is removed (`one api` stays).
- **BREAKING — `ayx one` resource identifiers are now positional.** Pre-release, no back-compat. Commands that took a required `--<noun>-id` flag now take the id positionally: e.g. `one flows detail <id>`, `one plans run <id>`, `one job-groups status <id>`, `one connections detail <id>`, `one token delete <id>`, `one person update <id>`, `one scheduling enable <id>`. Two-id commands take ordered positionals (`one connections permissions detail <connection-id> <subject-id>`, `one role assign <role-id> <subject-id>`); connector-metadata commands take a positional `<connector>` slug; `one datasets {wrangled,imported} detail <id>` (the old `--wrangled-id`/`--imported-id` are gone); `one flows import <input>`. Optional context selectors stay flags (`--workspace-id`, `one login`'s `--client-id`/`--workspace-id`/`--workspace-gid`, `plans permissions --subject-id`), as do payload/output controls (`--body`, `--output-file`, filters, pagination).
- **Alteryx-blue help, human-readable doctor, top-level command reorder** (`#95`): clap help/usage/errors are colorized with the Alteryx palette; `ayx doctor` renders a glyph/color status table instead of a raw JSON dump; the top-level `Command` enum is reordered task-first (profile, one, tools, secret, workflow, server, mongo, ...).
- **CI and release builds now pin `--locked`** on the workspace `cargo build`, `cargo clippy`, and `cargo nextest run` invocations in `ci.yml` and `build-release.yml`. `Cargo.lock` is already committed, so this closes a gap where the dependency resolution used in CI/release could silently drift from what a contributor last verified locally.
- **README accuracy fixes**: documented the Windows release archive (`ayx-x86_64-pc-windows-msvc.zip`) and Windows as a built target alongside Linux and macOS; corrected the docs-site live-reload command from the nonexistent `npm start` to `npm run dev`; reworded the `tools workspace` promotion-workflow guidance so it no longer points users at the `compare`/migration-helper scaffolds as if they were implemented, and annotated `tools` (`compare`, migration helpers) and `mongo` (`mutate`) as preview / not yet implemented in the top-level command list.
- **`docs/cli-spec.md`**: dropped the stale `(v0.11.0)` version stamp from the title so the published spec doesn't read as version-locked.

### Removed

- **`one auto-insights` and `one desktop-exec` commands**: both were config-posture stubs that performed no Alteryx One work and, worse, demanded a Server config section — so they errored on a One-only profile. They are removed from the `one` command tree and the machine-readable `catalog` rather than shipped as commands that mislead. They can return when the underlying surfaces are implemented.

### Fixed

- **Error-contract and OTP-prompt DX fixes** (`#100`): CLI errors now exit non-zero via `process::exit` instead of returning `Err` from `main` (previously appended a bare non-JSON `Error: ...` line that broke `--output json` parsing and triple-printed in text mode); "is required" validation errors are classified as `Validation` instead of `Internal`; `--output` is constrained to `text`/`json`/`yaml`/`table` via a clap value parser; the OTP login prompt now prints and flushes before reading stdin instead of leaving a frozen cursor.
- **`scripts/install.sh` checksum-tool selection**: `require_cmd sha256sum 2>/dev/null || require_cmd shasum` was dead code — `require_cmd` calls `exit` on a miss, so the `||` fallback never got a chance to run — and a stock macOS host (which has only `shasum`, not `sha256sum`) aborted silently before ever downloading. The installer now picks whichever tool is present once, up front, and reuses that choice for the checksum comparison.
- **`scripts/install.sh` Windows branch**: the download URL was hard-coded to `.tar.gz`, but the only Windows release asset is a `.zip`, so running the bash installer under Git Bash/MSYS/Cygwin 404'd. Windows now gets a clear message pointing at the PowerShell installer (`scripts/install.ps1`) instead of attempting a download that can't succeed.
- **Alteryx One API-coverage metadata drift**: `one api coverage` and `catalog describe` now report the endpoints the CLI actually calls. `one flows update` is recorded as `PATCH /v4/flows/{id}` (was `PUT`); `one platform workspace people`/`admins` map to `GET /v4/people` and `GET /v4/people?role=admin` (the `/v4/workspaces/{id}/people` and `/admins` routes 404 — workspace context is carried by the `x-alteryx-workspace-gid` header); the never-called `/v4/workspaces/{id}/people/{personId}/suspended` PUT/DELETE mappings are dropped in favor of the live `/iam/v1/.../suspend`|`unsuspend` pair; and the `workspace people`/`admins` prerequisites are corrected from `server_api` to `alteryx_one.access_token`. The inventory dedup guard now pins each shared endpoint to its exact expected command set, so future accidental drift fails the test.

### Docs

- Roadmap, hygiene, and pruning passes (`#96`, `#97`, `#98`, `#99`, `#101`, `#102`): reconciled the active roadmap against the shipped v0.12.2 surface, scrubbed a leaked internal vault-path reference and personal/internal identifiers (emails, workspace GIDs, tier names) from source, tests, docs, and the site, removed the dead Docusaurus `docs-site/` and stale internal planning docs, and fixed command-tree drift plus envelope-contract contradictions across the README, `cli-spec`, and site docs.
- **Docs-site release notes**: the Releases page no longer links to ungenerated `v0.9.12`/`v0.9.13`/`v0.9.14` pages (they are now generated from `docs/releases/`), and it carries `v0.12.1`/`v0.12.2` entries instead of stopping at `v0.12.0`.

### Dependencies

- Bump `taiki-e/install-action` 2.82.9 → 2.83.2 (`#104`), `regex` 1.12.4 → 1.13.0 (`#106`), `apple-native-keyring-store` 1.0.0 → 1.0.1 and `getrandom` 0.4.2 → 0.4.3 (`#105`).

## 0.12.2 — 2026-07-07

### Fixed

- **`ayx update` still failed after v0.12.1 on Linux/macOS** with
  `Could not find the required path in the archive: "ayx"`. The `.tar.gz`
  packaging used `tar -C "$root" .`, naming every member `./ayx`; self_update
  matches the archive entry against `ayx` with an exact `Path` compare, and
  `./ayx != ayx`. Package members explicitly so the binary is at `ayx`. (Windows
  `.zip` stores `ayx.exe` at the root already, so it was fixed by v0.12.1.)

## 0.12.1 — 2026-07-07

### Fixed

- **`ayx update` failed to extract release archives on every platform.**
  `self_update` was pulled with default features only, which include no archive
  backend, so self-update aborted with `ArchiveNotEnabled` — `.zip` on Windows
  (`Archive extension 'zip' not supported`) and equally `.tar.gz` on
  Linux/macOS. Enable `archive-tar` + `compression-flate2` (for the `.tar.gz`
  assets) and `archive-zip` + `compression-zip-deflate` (for the Windows
  `Compress-Archive` `.zip`). Note: upgrading *into* the first build that
  carries this fix still needs a one-time manual download, since the currently
  installed binary is the one that can't extract.

### Dependencies

- **`ayx-one-api`: `getrandom` 0.2 → 0.4.** The two CSPRNG helpers
  (`generate_pkce_challenge`, `generate_random_state`) move from the
  0.3-removed `getrandom::getrandom` free function to `getrandom::fill`.
  Behavior is unchanged — same OS entropy source, same fail-on-no-entropy
  contract — and new characterization tests lock the 256-bit verifier, the S256
  challenge relationship, and non-repetition. `ayx-one-api` now shares the
  `getrandom` 0.4.2 already pulled by `tempfile`; `getrandom` 0.2 stays in the
  tree only as a transitive dep of `secret-service`. Supersedes dependabot #57.

## 0.12.0 — 2026-07-07

### Added

- **Seamless Alteryx One first-run onboarding** (`#74`): `ayx onboard` parses a
  pasted workspace URL for its region and workspace gid and offers to run the
  email-OTP login immediately, so the wizard ends with you connected. Includes
  profile-split fixes so the onboarded profile is the one `auth login` writes its
  token into.
- **`ayx one datasets`** (`#82`): read the Alteryx One dataset library — `list`,
  `count`, plus `wrangled` (list/count/detail) and `imported` (detail).
- **`ayx one api`** (`#86`): One OpenAPI-spec introspection. `coverage` diffs the
  live spec against the wired-command inventory to surface gaps; plus `status`,
  `diagnose`, and `open-api-spec`.
- **Visual interface browser (TUI v2)** (`#68`, `#69`): a k9s-style resource
  browser (all five asset kinds, drill/filter/switch), a `Ctrl+K` command
  palette, `?` help, and inline editing, behind `AYX_TUI_V2=1 ayx tui`.
- **Install shadow warning** (`#67`): the installer warns when a different `ayx`
  earlier on `PATH` would shadow the freshly installed binary (Windows).

### Fixed

- **Onboarding yes/no defaults** (`#87`): prompts now honor the `[Y/n]` / `[y/N]`
  default they display. Pressing Enter at "Configure Alteryx Server" (shown as
  `[y/N]` on a fresh One onboard) correctly skips Server configuration instead of
  silently entering it and writing an empty server section.
- **Windows** (`#84`, `#85`): reserve a 16 MiB main-thread stack and enable the
  Windows `cli_smoke` job; remove the redundant command-dispatch worker thread.
- **Keyring test isolation** (`#81`): keyring tests use an in-memory mock store,
  so they no longer read or write the host OS keyring.

### Changed

- **`one ui` is gated behind a default-off cargo feature** (`#80`): the
  experimental visual-interface subtree is absent from the shipped binary.
- **Docs**: onboarding getting-started/connecting/configuration rewritten to the
  OTP flow (`#75`); One command descriptions backfilled (`#83`); command-surface
  coverage gaps captured (`#76`); README command tree reconciled with the shipped
  surface — `one ui` removed, `one api` and `one datasets` added (`#87`).

### Dependencies

- Bump `cmov` 0.5.3 → 0.5.4 (`#79`), `tui-input` 0.11.1 → 0.15.3 (`#73`), and
  `clap_complete`, `anyhow`, and `taiki-e/install-action` (`#71`, `#72`, `#77`,
  `#78`).

## 0.11.2 — 2026-06-27

### Fixed

- **Windows release asset** (`#63`): `scripts/install.ps1` downloads
  `ayx-x86_64-pc-windows-msvc.zip`, but the release workflow had no Windows build
  job, so every prior release was missing that asset and the PowerShell
  quick-start failed with a 404. Added a hardened `build-windows` job and wired
  it into the release pipeline (SHA256SUMS, Sigstore signing, SLSA attestation).
  This is the first release to publish a Windows binary.

### Added

- **Visual interface preview (TUI v2)** (`#62`): a new resource-browser TUI spine
  (The Elm Architecture + a `ResourceKind` registry) is available behind
  `AYX_TUI_V2=1 ayx tui`, currently browsing Alteryx One flows end-to-end. The
  existing `ayx tui` is unchanged without the flag. Foundations for a forthcoming
  workspace/asset browser.

## 0.11.1 — 2026-06-23

### Added

- `ayx secret prune` — removes keyring accounts orphaned by the v0.11.0
  profile_name to file-stem scope migration.  Dry-run by default; `--apply`
  to delete.  Targets the deterministic set of accounts writable by
  `secretize_config`; never enumerates the full keyring.  See
  [docs/releases/v0.11.1.md](docs/releases/v0.11.1.md).

## 0.11.0 — 2026-06-23

### Breaking changes

- **On-disk format** (`#50`, `#51`): the canonical config format now uses `client_secret_ref` /
  `curator_api_secret_ref` to store secrets indirectly (keyring or env references).
  Config files written by v0.11.0 are not readable by older binaries that lack
  the `_ref` fields. Existing plaintext configs load fine on upgrade; the ref is
  written on the next save (lazy migration).

### Features

- **Server-API secret consolidation** (`#51`): a single canonical source
  (`server_api.client_secret`) is now the authoritative secret for Alteryx Server
  connectivity. The legacy `api.auth.client_secret` and `server.curator_api_secret`
  fields are synthesized (derived, read-only) views of the same secret; writing to
  them is a no-op when they carry the same value as `server_api`. A mixed-state
  conflict (two representations resolving to different values) is detected at the
  write boundary and reported with field names and ref forms — never the resolved
  secret value itself.

### Migration notes

- **Keyring accounts re-key on next save** (lazy migration): the keyring account
  name now uses the on-disk file stem (standalone profiles) or `workspace.env`
  (workspace environments) as the stable scope, rather than the mutable
  `profile_name` field. After the first save, the old account (if any) may remain
  in your keyring; it is harmless and can be pruned with `ayx secret prune`
  (tracked in issue #4).

## 0.10.3 — 2026-06-22

### Security (dependencies)

- Bumped `quinn-proto` 0.11.14 → 0.11.15 to clear **RUSTSEC-2026-0185** (remote memory exhaustion / DoS via unbounded out-of-order stream reassembly), published the same day. `quinn-proto` is a transitive HTTP/3 QUIC dependency not on the CLI's HTTP/1.1 request path, so this is a `Cargo.lock`-only change with no behavior impact — but it restores a green `cargo audit` gate.

## 0.10.2 — 2026-06-22

### Security

- **Redirect-host allowlist** on the auth flow. The OTP→OIDC redirect follower now refuses to follow a `Location` to any host outside the base domain and its subdomains (e.g. `us1.alteryxcloud.com` allows `pingauth.alteryxcloud.com` but rejects `evil.com` and `alteryxcloud.com.evil.com`). An off-domain redirect is never requested, so no cookies are sent off-domain. (red-team M2)
- **Interaction-id shape validation.** The OIDC interaction id pulled from the redirect chain is now bounds-checked (6–128 chars, restricted charset) before use. (red-team M3)
- **Redacted two more raw response bodies** (`validatePasscode`, `/v4/auth/accounts` error paths) that previously interpolated unredacted bodies into errors — same leak class fixed earlier in the preflight path.

### Robustness

- Removed a latent `unwrap()` in the `auth diagnose` envelope builder (safe-by-construction today, but a footgun if the control flow changed).

### Tests

- 18 new unit tests for the redirect-host allowlist and interaction-id validation (306 total).

## 0.10.1 — 2026-06-22

### Removed

- Dropped the Playwright/headless-Chromium fallback from the Alteryx One first-login flow. The pure-HTTP reqwest flow (proven through v0.10.0) is now the only path — no `python3`, `playwright`, or `chromium` dependency, and no `AYX_ONE_AUTH_FORCE_BROWSER` / `AYX_ONE_AUTH_NO_FALLBACK` env vars. This removes ~505 lines (including an embedded Python script), drops the unused `tempfile` dependency, and resolves the red-team M4 finding (the workspace password was passed to the subprocess via an env var). The full browser implementation remains in git history if Alteryx ever changes their OIDC flow.
- Removed dead helpers orphaned by the earlier pure-HTTP refactor (`random_hex`, `wait_for_file`).

## 0.10.0 — 2026-06-22

Alteryx One authentication GA. This milestone follows a security and correctness
red-team of the auth flow and the API surface work; all blocking findings are fixed
and covered by tests (288 total, up from 255).

### Security

- `auth login` now warns when a 30-day PAT is stored inline (plaintext YAML) because the OS keyring is unavailable — previously silent on headless hosts. The inline-secret warning is shared with the onboarding path.
- Workspace preflight errors now redact the response body preview, matching the sibling parse-failure branch. No more raw response bodies (which can echo tokens/cookies) in error chains.
- The secret redactor now masks the field names this auth flow actually produces (`tokenValue`, `local-auth-workspace`, `x-csrf-token`, `passcode`, `passcodeReferenceId`, `secret`) plus bare JWT-shaped tokens (`eyJ…`).

### Workspace model

- The Alteryx One PAT is workspace-bound — the `x-alteryx-workspace-gid` header is ignored server-side; the token alone determines the workspace. The CLI now reflects this:
  - `workspace people` and `workspace admins` are argless (the old required `--workspace-id` was silently ignored and could imply the wrong workspace).
  - New `workspace switch --workspace-id <id>` selects an already-authenticated workspace credential instantly; if you have not logged into that workspace, it tells you to `auth login`.
  - `workspace invite-users` and the other membership mutations now reject an explicit `--workspace-id` that does not match the active workspace, instead of letting the path and the token diverge on a destructive operation.

### Correctness

- `connections connector-metadata template`: the connection-`type` heuristic now emits a `<jdbc|remotefile|…>` placeholder (with a `_note`) when it cannot confidently infer the type, instead of silently defaulting to `remotefile` for every non-relational connector.
- `job-groups list`: synthesized names now disambiguate multiple runs of one flow (`flow-{flowId} ({id})` / `flow-{flowId} @ {createdAt}`) instead of collapsing to a single `flow-{flowId}`.
- `apply_env_fallbacks`: restored uniform gap-fill precedence (env fills only an absent profile value) for `base_url`, `oauth_client_id`, `client_secret`, `token_endpoint_url`, matching the documented "last-resort fallback" contract.

### Tests

- 33 new deterministic tests: panic-regression guards for the four `--output-file` commands, plus unit coverage for the job-group name synthesizer, the connection-template builder, `resolve_workspace_id`, and the One-only-profile guard.

## 0.9.14 — 2026-06-22

### Bug fixes

- Fixed runtime panics in `flows export`, `server system-info`, `server runtime-settings`, and `tools workspace init`. Each defined a local `--output` (file path) arg that collided with the global `--output <text|json>` format flag, panicking on every invocation. The file arg is now `--output-file` on all four. `flows export` now exports a `.yxzp` package end-to-end.

### One API additions

- `connections connector-metadata template <slug>`: generates a fillable JSON create-body template from connector metadata (derives `type`, `vendor`, `credentialType`, and a `params` skeleton). Unblocks `connections create` body construction.

### Documentation

- `docs/one-live-validation.md`: full per-endpoint live-verified status table — working surfaces, PAT-scope-blocked surfaces, absent routes, and tier-gated surfaces.

## 0.9.13 — 2026-06-22

### One API additions

- `flows permissions-get <ID>`: read command for `GET /v4/flows/{id}/permissions`. Returns a clean `permission_denied` error (the endpoint is 403 under the current PAT scope) rather than a missing-command error. The existing `flows permissions` (POST, set permissions) is unchanged.
- `job-groups list`: synthesizes a display name (`flow-{flowId}`, falling back to `job-{id}`) when the API returns a null name, so flow-run job-groups are intelligible in text output.

## 0.9.12 — 2026-06-22

### One API endpoint fixes

- `flows update`: switched from `PUT /v4/flows/{id}` (returned 403) to `PATCH /v4/flows/{id}` (returns 200). Full CRUD on flows now works.
- `workspace people`: switched from `GET /v4/workspaces/{id}/people` (404) to `GET /v4/people` (200).
- `workspace admins`: switched from `GET /v4/workspaces/{id}/admins` (404) to `GET /v4/people?role=admin` (200).

### CLI ergonomics

- `--body <FILE>`: all 32 body mutation args now accept a path to a JSON file (previously the help text was ambiguous). Pass a file path or use `-` for stdin.
- `ayx one status` and `ayx one inventory` on One-only profiles: clean message instead of an internal config error.
- `platform workspace invite-users` and related membership commands: `--workspace-id` is now optional and defaults from the profile's `workspace_gid`.

### Documentation

- Billing, plans, and scheduling help text notes enterprise-tier requirement; commands return 404 on non-enterprise workspace tiers.
- `connector-metadata`: help text documents that connector enumeration (`list`) is not available via the v4 API; no `/v4/connectors` endpoint exists.

## 0.9.10 — 2026-06-20

### Docs and release cleanup

- Move the public docs site to Astro/Starlight under `site/`.
- Remove stale dashboard and legacy docs references from the public docs surface.
- Rehome the runtime fixture under `docs/fixtures/RuntimeSettings.xml`.
- Keep the CLI spec and command-surface docs aligned with the live 0.9.10 binary.

### Verification

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets --locked -- -D warnings`
- `cargo test --workspace --locked`
- `cargo run -q -p xtask -- refresh-command-surface --check`

## 0.9.8 — 2026-06-16

### One hardening and release prep

- Tighten the One transport envelope so live requests, dry-runs, auth failures, and backend errors report a stable shape with request metadata.
- Add a table-driven live validation matrix for the wired One surface so the smoke suite proves real API reachability instead of just envelope construction.
- Standardize auth failure, permission failure, and transport failure classification so blocked environments are reported explicitly.
- Update the contributor and release docs to prefer `cargo nextest run` and align the public release checklist with the current CI matrix.

### Verification

- `cargo nextest run -p ayx-one-api --lib`
- `cargo nextest run -p ayx-rs --test one_live_smoke`

## 0.9.7 — 2026-06-15

### Progressive discovery

- Add a first-class `ayx discover` entry point for the live `clap` tree.
- Keep `catalog` as the machine-readable registry view while discovery becomes the progressive agent substrate.
- Regenerate the command-surface docs and smoke tests so the live binary, docs, and catalog stay aligned.

## 0.9.6 — 2026-06-15

### Workspace credential mapping

- Add workspace-scoped One API credentials and prefer them over legacy top-level tokens when a workspace is known.
- Resolve One refresh and auth status paths through the active workspace credential when present.
- Keep the release docs and smoke tests aligned with the `us1` API host and auth issuer split.

## 0.9.5 — 2026-06-15

### One API host and auth fixes

- Require an explicit `AYX_ONE_BASE_URL` for One API requests instead of inferring the API host from the token endpoint.
- Keep `AYX_ONE_TOKEN_ENDPOINT_URL` pointed at the auth issuer and normalize `/as` to `/as/token` when refreshing access tokens.
- Align the One platform workspace and role routes with the published v4 OpenAPI surface.
- Refresh the user and agent guidance in the sample config and docs so the API host and auth host are clearly separated.

### Verification

- `cargo nextest run -p ayx-core one_token_endpoint`
- `cargo nextest run -p ayx-one-api refresh_token_uses_refresh_token_only`
- `cargo nextest run -p ayx-rs`

## 0.9.4 — 2026-06-02

Completes the two breaking dependency upgrades that 0.9.3 deliberately deferred.
Both landed as isolated, reviewed changes and are green on Linux and macOS CI.

### Dependencies

- Migrate `keyring` 3.6 → 4.0. keyring 4.x moved the `Entry` API to
  `keyring-core` and split the platform credential stores into separate crates
  that are registered at runtime. ayx-core now depends on `keyring-core` plus a
  per-OS store (zbus Secret Service on Linux, native Keychain on macOS, native
  Credential Manager on Windows) and registers it once before first use. Also
  replaces a fragile error-string match with `Error::NoEntry` so not-found
  handling is correct under the new error type.
- Upgrade `axum` 0.7 → 0.8 and convert the dashboard router to the new
  path-capture syntax (`:id` → `{id}`, `*path` → `{*path}`); the old form is a
  router-build panic under 0.8, not a compile error.
- Drop the `keyring` / `axum` major-version ignore rules from `dependabot.yml`
  now that the deferred migrations are done, so future updates are tracked again.

## 0.9.3 — 2026-06-01

First complete release since 0.9.1. The 0.9.2 tag never published artifacts
because its release build failed on the Windows job; this release drops Windows
to ship cleanly on Linux and macOS.

### Platform support

- Drop Windows from CI and the release pipeline. Tests run on Linux and macOS;
  release artifacts are `x86_64-unknown-linux-gnu` and the two macOS targets.
- Fix the Windows-only `cli_smoke` build break that triggered this (the
  `std::fs` import is now gated to match its `#[cfg(not(windows))]` usage),
  kept for correctness even though Windows is no longer built.

### Dependencies

- Defer the breaking `keyring` 4.x and `axum` 0.8.x upgrades; stay on the
  latest 3.x / 0.7.x (both `cargo-audit` clean) and add `dependabot.yml` ignore
  rules so the breaking majors stop being re-proposed.

## 0.9.1 — 2026-05-29

### CI and release fixes

- Pull in the current `cargo-audit` ignore set and lockfile refresh so CI matches the upstream passing dependency state.
- Switch GitHub Actions test jobs to `cargo nextest run` for faster, more consistent workspace validation.
- Replace the broken GitHub Actions lint action, opt workflows into the Node.js 24 runtime early, and keep shell globs actionlint-safe.
- Fix release signing secret scoping so Windows/macOS signing and notarization steps can actually run when secrets are present.
- Make SBOM collection deterministic for the current `cargo-cyclonedx` output layout and fail the SBOM job if no JSON files are produced.

### CLI maintenance

- Preserve catalog coverage after the `main.rs` refactor by restoring the stronger catalog describe/tag-filter unit tests.
- Add workspace summary parsing coverage in `ayx-one-api` for list responses that use `workspaceName`, `workspace_id`, and related aliases.
- Correct the local source install command in the README to point at the real binary crate path (`ayx-rs/`).

## 0.9.0 — 2026-05-27

### Dependency modernization

- Bump workspace dependencies to current patch releases: `clap_complete`, `openssl`, `openssl-sys`, `reqwest`, `rustls-webpki`, `serde_json`, and `tower-http`.
- Add the direct `base64` dependency in `ayx-rs` so the dashboard/server code uses the workspace-managed crate version.

### Workflow and runtime hardening

- Keep YXDB handling flexible while making workflow parsing safer and more explicit about malformed inputs.
- Preserve structured failure handling in `ayx-rs` and keep dashboard password handling from mutating opaque secrets.

### Release and docs cleanup

- Refresh the public release docs, install scripts, CI release workflow, and release checklist to match the current repository shape.
- Update the changelog and package version so tag-based CI can publish the next release line cleanly.

### Dependency upgrades

- `reqwest` 0.12 → 0.13 (workspace). Feature set updated to `rustls` (was `rustls-tls`) and adds `form`.
- `zip` 0.6 → 8 (workspace). `ayx-server` and `ayx-workflow` now consume the workspace pin instead of pinning locally.
- `sha2` 0.10 → 0.11 (workspace).
- `self_update` 0.43.1 → 0.44.0 (`ayx-rs`).

### Code changes required by the upgrades

- `zip`: `FileOptions` → `SimpleFileOptions` in `ayx-server/src/upgrade/service.rs` and `ayx-workflow/src/lib.rs`.
- `sha2`: `format!("{:x}", hasher.finalize())` no longer compiles because the new `Array<u8, ...>` return type does not implement `LowerHex`. Switched `ayx-workflow/src/cloud_convert.rs::checksum` to the same byte-iter idiom already used in `ayx-server/src/upgrade/manifest.rs::compute_sha256`.

### `ayx onboard` fixes

- Skip the storage backend section entirely when "Configure Alteryx Server" is N. Previously the RuntimeSettings.xml, AlteryxService.exe, and Mongo restore-target prompts ran regardless of the server answer.
- Drop the "Designer user install" yes/no prompt. The service detector now always probes `%LOCALAPPDATA%\Alteryx\bin` in addition to `C:\Program Files\Alteryx\bin`, so per-user Designer installs are picked up without asking.
- Drop the "Embedded Mongo restore target path" prompt. The value is resolved at restore time from `RuntimeSettings.xml` (`ayx-server/src/mongo.rs::resolve_embedded_restore_target_path`); existing profile values are preserved.

### Verification

- `cargo build --workspace` clean.
- `cargo nextest run -p ayx-workflow -p ayx-server -p ayx-rs` passes (including `workflow_canary` and `one_live_smoke` integration tests).

## 0.7.0

See commit `162fb05`.
