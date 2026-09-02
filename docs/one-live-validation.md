# One Live Validation

Per-endpoint, dated live evidence lives in `docs/one-endpoint-matrix.md`; this document is the
release-testing procedure.

This document tracks the live validation strategy for the wired Alteryx One surface.

> **Output-contract note:** the historical command snippets below inspect raw
> `.data.*` fields with `jq`. Run those snippets as `<full command> --output
> json-full` (trailing placement), not as `ayx --output json <command>`.
> `--output json` now returns the compact `ayx.output.v1` presentation envelope.

## Coverage Model

- `validated_live`: a real request returned from the One API host and the response was asserted.
- `validated_shape`: request construction, dry-run behavior, or envelope formatting was asserted without a live mutation.
- `blocked_by_auth`: the environment could not acquire usable live credentials.
- `blocked_by_scope`: the endpoint exists, but the current workspace/role does not have permission to exercise it.

## Surface Inventory

Test the currently wired One families in the CLI and API layers:

- platform / auth / workspace / person / token / role
- plans
- flows
- workflows (cloud-native, ULID-keyed, `/svc-workflow`)
- datasets
- connections
  - detail
  - permissions list
  - connector-metadata defaults
  - connector-metadata publish-info
- job-group
  - list
  - detail
  - status
  - inputs
  - outputs
  - jobs
  - publications
  - profile
  - profile-results
  - pdf-results
- output-object
- webhook-flow-task
- write-setting
- scheduling
- doctor / inventory / status helpers

## Validation Criteria

- One representative live read or discovery call per family.
- One edge case per family when the API supports it.
- For list endpoints: verify pagination or empty-result handling where possible.
- For mutating endpoints: prefer dry-run or a reversible safe case before any real mutation.
- Every result must record the command, endpoint family, status bucket, and whether it was truly live.
- Current smoke coverage includes invalid-id failures for representative detail commands and pagination-boundary checks for the major list families.

## Pressure Test Level

Use the default "happy path + one edge" matrix:

- happy path: prove the live endpoint is reachable and returning an expected envelope
- edge path: exercise invalid id, empty page, pagination boundary, or permission failure

Escalate to broader matrices only for families that are known to be flaky or stateful.

## Live Validation Hygiene

- Use `cargo nextest run` for all repo and smoke validation going forward.
- Keep One-only live tests on a minimal profile that still satisfies the config model, but avoid mixing in unrelated Server storage assumptions when validating the One cloud API.
- If auth fails, classify it as an environment blocker first. Only treat the surface as broken after a confirmed live request reaches the One host and returns a backend error.

## Current Harness

The current smoke harness lives in `ayx-rs/tests/one_live_smoke.rs` and already:

- uses the live CLI binary
- short-circuits cleanly when auth acquisition is unavailable
- validates the most important read paths across the One surface
- reports the surface and operation names in the envelope assertions
- contains 75 generated live tests as of this repo state. The v0.17.0-rc.3
  validation used a fresh Wizard login and a release-binary read sweep; tests
  requiring a live token remain explicitly gated by `AYX_ONE_LIVE_SMOKE`.

## Methodology traps

**GET-probing cannot detect POST-only routes.** `GET /v4/connections/share` returns the exact same
`RouteNotFoundException` shape as a genuinely nonexistent path — a GET probe cannot distinguish
"this route doesn't exist" from "this route exists but only accepts POST." The reliable existence test is
**POST with an intentionally invalid body**:

- `400 ApiValidationFailed` → the route exists; the API parsed the request far enough to validate
  the body and reject it.
- `404 RouteNotFoundException` → the route does not exist at this path.

**Two different classes of "404."**

- **Route-level 404** — the path itself is not registered on the server. On the `/v4` gateway and
  the `/billing/v1` managed service, this comes back as JSON with a `RouteNotFoundException`. The
  plans, scheduling, and workspace suspend/unsuspend commands in the current CLI now point at
  `/v4`; the old `/plans/v1`, `/scheduling/v1`, and `/iam/v1` routes were path bugs, not tier
  gates. On `/svc-workflow`, an unrouted path can come back as an Express default HTML 404 page.
- **Application-level 404** — the route exists and parsed the request, but the specific resource id
  does not. On `/svc-workflow`, a well-formed but nonexistent ULID can return clean JSON
  `NotFoundError`. Do not conflate this with a route-level 404.

**`one X list` reporting `"ok": true` does not prove the underlying route returned 200.** The shared
`one_api_list_request` helper extracts items without first checking whether each page's HTTP call
succeeded. A page that 404s can therefore report `"ok": true` with 0 items. When re-verifying a list
row, always check `data.page_envelopes[].status_code`, or cross-check against the matching count
row or `one doctor <surface>`, rather than trusting `ok: true` alone.

## Safety boundaries (apply throughout)

Classification source of truth: the `Safety`/`Mutating` columns in `docs/command-surface.md`.

| Tier | Rule |
|---|---|
| **GREEN** | Any read-only leaf, or any mutating leaf **without** `--apply`. Safe by construction — `ayx-one-api/src/lib.rs:883` returns the dry-run envelope *before* any network call. Run freely. |
| **YELLOW** | `one login --profile <name>` (rewrites local credential state, not `--apply`-gated). Prefer a named profile (`rc-check`), never `default`; use secret-safe OAuth env/stdin input for unattended runs. |
| **ORANGE** | `one flows create/update/delete` — per-command go, but pre-approved in this plan (Phase 5). Tenant baseline is zero flows/folders, so cleanup is verifiable. |
| **RED** | `workflows copy`/`share`, `person password-reset-request`, `workspace invite-users`, `webhook-flow-tasks test`, `plans share`, `connections permissions create`, `token create`. Default skip. **`workflows copy` is pre-approved for exactly one deliberate demo-asset creation (Phase 5b)** — nothing else in this tier runs without naming the specific command first. |
| **BLACK** | `workspace delete-configuration`, `workspace delete-current-configuration`. Never, no exceptions. |

Inherited hard rule: **no `--apply` in Phases 0–4.** It appears only in Phase 5, on the specific
pre-approved commands below.

## Phase 0 — Offline pre-flight (~5 min, zero live calls)

```bash
cd /path/to/ayx-rs
git status --porcelain && git log --oneline -1
which ayx && ayx --version                       # must be ~/.local/bin/ayx, 0.16.0
cargo nextest run --workspace -E 'not binary(one_live_smoke)'
cargo nextest run -p ayx-rs -E 'binary(one_inventory_drift)'
cargo test -p ayx-rs --test one_endpoint_matrix_doc
cargo nextest list -p ayx-rs -E 'binary(one_live_smoke)'   # record real test count (macro-generated, grep undercounts)
```

Gate: all green, tree clean, version confirmed. Record the listed live-suite test count — Phase 1
must match it.

## Phase 1 — Full automated live suite (~10 min)

Run the full suite, not a filtered subset — a partial run understates what's actually covered.

```bash
cd /path/to/ayx-rs
set -a && source .env && set +a
AYX_ONE_LIVE_SMOKE=1 cargo nextest run -p ayx-rs -E 'binary(one_live_smoke)'
```

For an interactive workstation validation, do not copy a keyring-backed token
into `.env`. Instead, explicitly opt in to the already-authenticated central
profile (for example `local-dev`):

```powershell
$env:AYX_ONE_LIVE_SMOKE = '1'
$env:AYX_ONE_LIVE_PROFILE = 'local-dev'
cargo nextest run -p ayx-rs -E 'binary(one_live_smoke)' --locked
Remove-Item Env:AYX_ONE_LIVE_PROFILE
```

Without `AYX_ONE_LIVE_PROFILE`, the suite retains its CI behavior and builds a
temporary `live` profile from the repository `.env` values.

Note: this spawns a **debug** build via `CARGO_BIN_EXE_ayx`, not the release binary on PATH —
Phase 1 and Phase 2 test different artifacts (coverage, not a gap, as long as it's stated).

**Expected, not bugs to fix now:**

- `one_connections_dry_run_shape_live` — documented pre-existing failure.

The current entitled disposable validation probe returned HTTP 200 for `one plans count`, so its live-smoke
allowlist is intentionally narrow: only `permission_denied` remains an accepted backend result.
Unexpected `not_found` or transport failures are findings, not expected noise.

**Silent-skip audit** — capture which `*_real_object` tests self-skip (no fixture: flows, folders,
output-objects, write-settings, token detail) versus actually ran:

```bash
AYX_ONE_LIVE_SMOKE=1 cargo nextest run -p ayx-rs -E 'binary(one_live_smoke)' 2>&1 | tee /tmp/live-smoke.log
grep -i "no .* found\|returning None\|skip" /tmp/live-smoke.log
```

Anything that skipped is `unverified` in the matrix, not `live 200` — do not let a green run imply
coverage it didn't have.

If `live_smoke_requires_a_live_token` panics: stop. For an OAuth profile,
re-import a newly issued access/refresh pair through the secret-safe env/stdin
path; for an OTP profile, use the interactive `ayx one login` flow. Do not put
token values in the command line or captured logs.

## Phase 2 — Read-only re-verify sweep, release binary (~15 min)

Run against `~/.local/bin/ayx` this time.

```bash
cd /path/to/ayx-rs
set -a && source .env && set +a

# resolve ids the sweep needs
ayx one connections list --output json | jq -r '.data.items[0].id'
ayx one job-groups list --output json  | jq -r '.data.items[0].id'
ayx one workflows list --output json   | jq -r '.data.items[0].id'   # reused in Phase 4/5b
```

Run the matrix's read-only command block:

```bash
ayx one workspace current --output json
ayx one workspace list --output json
ayx one person current --output json
ayx one person list --output json
ayx one token --output json
ayx one doctor discover --output json
ayx one doctor plans --output json
ayx one doctor scheduling --output json
ayx one plans list --output json
ayx one plans count --output json
ayx one flows list --output json
ayx one flows folders list --output json
ayx one datasets list --output json
ayx one datasets wrangled list --output json
ayx one connections list --output json
ayx one connections detail <connection_id> --output json
ayx one workflows list --output json
ayx one workflows count --output json
ayx one workflows tools --output json
ayx one job-groups list --output json
ayx one job-groups detail <job_group_id> --output json
ayx one output-objects list --output json
ayx one write-settings list --output json
ayx one scheduling list --output json
ayx one api open-api-spec --output json
ayx one api coverage --output json
```

**Critical trap:** `"ok": true` on a `list` command does **not** prove a 200. Check status
explicitly:

```bash
ayx one <cmd> --output json | jq -c '{ok, code: (.data.page_envelopes[0].status_code // .data.status_code), n: (.data.items | length?)}'
```

For `plans`/`scheduling`, cross-check against `one doctor <surface>` rather than
trusting the `list` leaf alone. Also re-check whether the tenant still has zero
flows/folders/wrangled-or-imported-datasets/output-objects/write-settings/API-access-tokens; this
determines how many Phase 1 skips were legitimate.

**Delete-route existence check (settles the workflow-delete question before Phase 5b).**

```bash
set -a && source .env && set +a
# a well-formed but certainly-nonexistent ULID, both plausible route shapes:
curl -s -o /dev/stderr -w "\nHTTP %{http_code}\n" -X DELETE \
  -H "Authorization: Bearer ${AYX_ONE_API_ACCESS_TOKEN}" \
  "${AYX_ONE_BASE_URL:-https://us1.alteryxcloud.com}/svc-workflow/api/v1/assets/01AAAAAAAAAAAAAAAAAAAAAAAA"
curl -s -o /dev/stderr -w "\nHTTP %{http_code}\n" -X DELETE \
  -H "Authorization: Bearer ${AYX_ONE_API_ACCESS_TOKEN}" \
  "${AYX_ONE_BASE_URL:-https://us1.alteryxcloud.com}/svc-workflow/api/v2/workflows/01AAAAAAAAAAAAAAAAAAAAAAAA"
```

An Express HTML 404 or JSON `RouteNotFoundException` means the route genuinely does not exist.
A clean JSON `NotFoundError`/`400`/`403` means the route exists and rejected the fake id — delete
is possible but unwired in the CLI. Record the result in the workflow-family notes in the matrix.

## Phase 3 — Targeted 0.16.0 regression pass (~15 min)

One check per CHANGELOG-flagged fix, honestly scoped to what is live-testable with `.env`'s
`AYX_ONE_*` credentials. The Server-API error-code fix is Server-side and should be recorded as
unit-test-validated only.

1. **Transport failures not masked as empty success**:

   Forcing this live is harder than it looks — `.env`'s `AYX_ONE_BASE_URL` gets copied into
   `config.alteryx_one.base_url` before host resolution ever checks an env var, so a plain
   `AYX_ONE_BASE_URL=...`/`AYX_ONE_API_BASE_URL=...` shell prefix is silently ignored (confirmed
   live in the 2026-08-14 pass — real tenant data came back both times). There is also a second
   `.env` lookup beside the resolved central profile that overrides a `.env` edited in an isolated
   cwd, so even editing a scratch copy doesn't reliably work. A genuine live repro needs the real
   `.env`'s `AYX_ONE_BASE_URL` line stripped entirely (not overridden) plus the shell-level vars
   unset, e.g.:

   ```bash
   probe_dir=$(mktemp -d)
   awk '!/^[[:space:]]*(export[[:space:]]+)?AYX_ONE_BASE_URL[[:space:]]*=/' .env > "$probe_dir/.env"
   ( cd "$probe_dir" && env -u AYX_ONE_BASE_URL -u AYX_ONE_API_BASE_URL \
       AYX_ONE_API_BASE_URL=https://ayx-rc-check.invalid \
       ayx one workflows list --output json ); echo "exit=$?"
   rm -rf "$probe_dir"
   ```

   PASS: non-zero exit, `error_code: "network"`. FAIL: `ok: true` with `items: []`.

   Given the setup cost, the pragmatic default is to skip the live repro and instead confirm the
   three dedicated non-live regression tests are passing (already covered by Phase 0's offline
   suite): `list_request_json_404_is_reported_as_failure_not_an_empty_success`,
   `list_request_html_404_is_reported_as_failure_not_an_empty_success`, and the paginated-failure-
   stays-failed case, all in `ayx-one-api/src/lib.rs`. Only do the live repro above if those tests
   themselves are in question.

2. **`one api coverage` breaking shape + false-green fix**:

   ```bash
   ayx one api coverage --output json | jq '{coverage_pct, inventory_total, spec_operations, outside_spec_namespace_len: (.data.outside_spec_namespace | length?), stale_commands_is_array: (.data.stale[0].commands | type)}'
   ```

   PASS: `stale[].commands` is an array, `coverage_pct` is `null` if `spec_operations` is still 0
   (not falsely `100.0`), and `outside_spec_namespace` is present (7 rows in the current baseline).

3. **`one connections permissions` route fix**:

   ```bash
   ayx one connections permissions list <CONNECTION_ID> --output json | jq -c '{ok, code: (.data.page_envelopes[0].status_code // .data.status_code)}'
   ```

   PASS: not a `RouteNotFoundException`.

4. **`output-objects wrangle-to-python` `--apply` gate** (fake id is fine; the gate short-circuits
   before any network call):

   ```bash
   ayx one output-objects wrangle-to-python 999999 --output json | jq '.data | {dry_run, mutating, would_send}'
   ```

   PASS: dry-run envelope.

5. **410 → `not_found`** — deferred to Phase 5's flows delete→read-back cycle; nothing else in
   this pass deletes anything.

## Phase 4 — Demo-quality UX pass on `ayx one workflows` (~20 min)

```bash
ayx one workflows --help
for c in list count detail dependencies assets engines tools copy share; do
  echo "--- $c ---"; ayx one workflows $c --help
done

ayx one workflows list
ayx one workflows list --output table
diff <(ayx one workflows list) <(ayx one workflows list --output table) && echo "table == text"
ayx one workflows list --output json | head -40
ayx one workflows list --output yaml | head -40

ayx one workflows list --limit 5
ayx one workflows list --all | tail -5
ayx one workflows count

ayx one workflows detail <ULID>
ayx one workflows detail <ULID> --include-dependencies
ayx one workflows dependencies <ULID>
ayx one workflows engines <ULID>
ayx one workflows assets --limit 5
ayx one workflows tools | head -30

# dry-run mutations — safe, no --apply, no network
ayx one workflows copy <ULID> --name "rc-check-copy" --output json | jq '.data'
ayx one workflows share <ULID> --to-person <YOUR_EMAIL> --output json | jq '.data'

# error UX
ayx one workflows detail 01AAAAAAAAAAAAAAAAAAAAAAAA   # well-formed ULID, no such asset
ayx one workflows detail not-a-ulid                    # malformed
```

What to look for: useful text-table columns, ULID legibility, byte-identical `text`/`table`
output, accurate pagination hints, visible `detail_source`/`count_source`, and clean not-found and
malformed-id behavior. Deliver a short papercut list tagged **blocks-demo** / **fix-after** /
**accept**.

## Phase 5 — Live mutations (approved) + deliberate demo asset

### 5a. Reversible `one flows` cycle — closes the 410 regression check

```bash
ayx one flows list --output json | jq '.data.items | length'      # baseline, expect 0
ayx one flows create --body <payload.json> --output json | jq '.data.would_send'   # dry-run first
ayx one flows create --body <payload.json> --apply --output json   # TTY confirm fires — do not pass --yes
ayx one flows detail <NEW_ID> --output json
ayx one flows update <NEW_ID> --body <patch.json> --apply --output json
ayx one flows delete <NEW_ID> --apply --output json
ayx one flows detail <NEW_ID> --output json | jq '{ok, error_code: .error.code}'   # PASS: not_found
ayx one flows list --output json | jq '.data.items | length'      # must be back to 0 — hard requirement
```

If the final count isn't 0, the pass isn't complete — record the residue, don't leave it silent.

### 5b. Deliberate demo asset — `workflows copy`

This is demo prep, not a test. Before running it:

- Resolve Phase 2's delete-route check first. If both paths are route-level 404s, the copy is
  permanent and requires sign-off on that basis. If the route exists but is unwired, say so plainly:
  the copy is cleanable via a raw API call, which changes the risk calculus.
- Name the real workflow ULID from the Phase 2/4 list; do not guess.
- Choose the copy name.
- Get one explicit confirmation immediately before running, restating the actual cleanup story.

```bash
ayx one workflows copy <CHOSEN_ULID> --name "<CHOSEN_NAME>" --apply --output json
```

(`--send-email` is intentionally omitted from `share` in this pass — no real share, only the copy,
unless a real share is separately requested.)

### 5c. Completed Phase 1 — groups, schedules, connection permissions, and plans

This phase was executed against a disposable validation workspace on
2026-08-20 using an authenticated isolated profile. Every mutation was dry-run reviewed first.

- Created a disposable validation group, added two disposable users,
  verified both memberships, removed both, and deleted the group. The workspace returned to its
  original single-group baseline.
- Added and live-verified schedule lifecycle commands for `POST /v4/schedules`,
  `PUT /v4/schedules/{id}`, and `DELETE /v4/schedules/{id}`. Created a disposable schedule,
  renamed it, disabled it, deleted it, and verified the
  schedule list returned to the original one-schedule baseline. The list endpoint reflected the
  deletion immediately; the detail endpoint remained eventually consistent and served the deleted
  record afterward.
- Shared a disposable connection with both people as viewers, verified access,
  revoked both shares, and verified the original permission list. Connection credentials and
  configuration were not changed. The request builder was corrected to omit empty subject buckets
  because the live API rejects an empty `group` array when sharing with people.
- Created plan `156184`, inspected its empty node/edge graph, deleted it, and verified the plan list
  returned to its original four entries.

No workflow execution, plan execution, connection update/delete, invitations, role changes, or
other deferred API families were run in this phase.

### Canonical reversible CRUD gate

The disposable group and plan fixtures provide the minimum write-validation
protocol for a release candidate. Run this only against the named disposable
validation workspace, after recording the baseline counts; do not substitute
production assets or omit cleanup:

```powershell
$group = 'ayx-rs/tests/fixtures/one-group-canary.json'
$groupUpdate = 'ayx-rs/tests/fixtures/one-group-canary-update.json'
$plan = 'ayx-rs/tests/fixtures/one-plan-canary.json'
$planUpdate = 'ayx-rs/tests/fixtures/one-plan-canary-update.json'

ayx one workspace current --output json                         # capture <WORKSPACE_ID>
ayx one workspace groups <WORKSPACE_ID> --output json
ayx one plans list --output json
ayx one workspace create-group <WORKSPACE_ID> --body $group --output json # dry-run
ayx one plans create --body $plan --output json                      # dry-run
# Apply create, capture each returned id, then update and verify each object:
   ayx one workspace create-group <WORKSPACE_ID> --body $group --apply --yes --output json
   ayx one workspace update-group <WORKSPACE_ID> <GROUP_ID> --body $groupUpdate --apply --yes --output json
   ayx one workspace groups <WORKSPACE_ID> --output json                # assert updated group
   ayx one plans create --body $plan --apply --yes --output json
   ayx one plans update <PLAN_ID> --body $planUpdate --apply --yes --output json
   ayx one plans detail <PLAN_ID> --output json
# Always delete in a finally block, verify not_found/detail behavior, and re-list:
   ayx one plans delete <PLAN_ID> --apply --yes --output json
   ayx one workspace delete-group <WORKSPACE_ID> <GROUP_ID> --apply --yes --output json
   ayx one plans list --output json
   ayx one workspace groups <WORKSPACE_ID> --output json
```

The gate passes only when create, update, detail, and delete all succeed and
the final group/plan listings match the recorded baselines. If cleanup fails,
stop the release and remove only the captured canary IDs manually; never use a
name-based bulk delete. The checked-in fixture names must be unique before the
run, and every mutation must have a reviewed dry-run envelope first.
