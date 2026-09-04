# Wave 1: `ayx one access` governance primitives

**Date:** 2026-09-04
**Status:** Design approved in principle; live re-verification checklist must
be run before the implementation plan is written
**Roadmap:** `docs/roadmap/agent-first-substrate.md`, Wave 1
**Product scope:** Alteryx One only. Alteryx Server gets `ayx server access …`
in a later wave and never shares a command with this surface.

## Goal

Answer the three governance questions a One administrator actually asks, from
the CLI, with evidence, in a form a human, a CI job, or an agent consumes the
same way:

1. **Who has access to what?** (`access matrix`, `access review`)
2. **Why does this person have access to this thing?** (`access explain`)
3. **What changed since <date>?** (`snapshot take|diff`, `access diff`)

Plus the hygiene view that falls out of the same data: `token audit`.

This is the first killer capability on the agent-first substrate. The June
2026 governance research (recovered at `48567cc`) and the endpoint matrix
establish that most of the raw material is already wired; this wave adds the
normalization, the indexes, the evidence model, and the commands.

## Principles

- **Read-only first.** Every command in this wave is `read-only` except the
  plan emission in `access review --plan-out`, which writes a local file and
  contacts no server. Actual revocation is Wave 3 (`ayx apply`).
- **Derived views say so.** Every grant row carries `source` and `evidence`.
  A view assembled from three endpoints never presents itself as one.
- **Scope walls are results, not failures.** Where the tenant, tier, or token
  cannot answer (flow permissions under a PAT, role assignments), the row says
  `blocked_by_scope` and the envelope says `complete: false`. This reuses the
  `incomplete` error code and the `blocked_by_scope` classification the live
  smoke suite already uses.
- **Bounded fan-out.** Per-resource permission lookups are O(N) requests.
  `--all` and `--max-pages` govern them exactly as `telemetry permissions
  connections --deep` does today; the default is one page per source.
- **Snapshots are the audit trail.** One has no audit-log API. A snapshot is
  the whole normalized inventory at a point in time; a diff of two snapshots is
  the change record.

## Data model

```text
Subject   { kind: person|group, id, email?, name?, is_admin?, is_suspended? }
Resource  { kind: workspace|connection|workflow|flow|plan|token|group|role, id, name? }
Grant     { subject, resource, privileges: [string],
            source: direct|group_member|workspace_member|workspace_admin|owner|role,
            via: Option<Subject|Resource>,     // the group or role that carried it
            evidence: { endpoint, method, status, fetched_at } }
Inventory { workspace: { id, gid, name }, taken_at, subjects[], resources[], grants[],
            sources: [{ name, endpoint, status: live|blocked_by_scope|not_applicable|error, count }],
            complete: bool }
```

`privileges` are the product's own strings where the product supplies them
(`/svc-workflow` share body uses `create|delete|execute|read|share|update`;
connection shares are presence-only and normalize to `["use"]`). Never invent a
privilege vocabulary the product does not return.

## Sources

Status is from `docs/one-endpoint-matrix.md` at `v0.19.1`. Every row marked
*verify* is on the checklist below.

| Source | Endpoint | Command today | Status | Notes |
| --- | --- | --- | --- | --- |
| People / membership | `GET /v4/people` (header-scoped to the workspace) | `one person list`, `one workspace people` | live 200 | `/v4/people/count` is `410 gone`; count client-side |
| Admins | `GET /v4/workspaces/{numericId}/admins` | `one workspace admins` | declared; *verify* (`dc739a3`, `290ae51` repointed it) | fallback: filter `isAdmin` client-side, labeled as such |
| Groups | groups list | `one workspace groups`, `groups-global` | wired; *verify* live status and whether a members list exists (`add-group-users` exists; a `GET` of members is not in the matrix) | without a members list, `group_member` grants cannot be expanded |
| Roles | `GET /v4/authorization/roles`, `/{id}` | `one role list`, `one role detail` | live 200 | Viewer policy `25704008` confirmed |
| Role → people | `GET /v4/authorization/roles/{id}/people` | `one role list-assignments` | live **403** `permission_denied` | `blocked_by_scope` under the PAT; report it, do not retry |
| Group → roles | `PUT /v4/workspaces/{id}/groups/{groupId}/roles` | `one workspace set-group-roles` | live 2xx (write) | *verify* whether a `GET` counterpart exists; without it, role grants are visible only through group detail |
| Connection shares | `GET /v4/connections/{id}/permissions/sharedSubjects` | `one connections permissions list` | live 200 | already indexed by `telemetry permissions connections --deep` |
| Plan shares | `GET /v4/plans/{id}/permissions` | `one plans permissions` | live 200 | tier-gated surface; 404 on `platform_packaging` is `not_applicable`, not an error |
| Flow permissions | `GET /v4/flows/{id}/permissions` | `one flows permissions-get` | live **403** under PAT | `blocked_by_scope` |
| Workflow shares (cloud-native) | `POST /svc-workflow/api/v2/workflows/{id}/share` | `one workflows share` | write only | *verify* whether any `GET` exposes current shares (`/svc-workflow/api/v1/assets/{id}`? ). If none, workflow grants are `not_available` and the matrix says so. Do **not** reach for the DataHub GraphQL the web UI uses; it is observed-and-unsupported. |
| Tokens | `GET /v4/apiAccessTokens` | `one token list` | live 200 | *verify* whether an admin sees all tokens or only their own; `token audit` scope follows the answer |
| Workspace | `GET /v4/workspaces/{id}` | `one workspace detail` (Wave 0) | live after Wave 0 | anchors `Inventory.workspace` |

## Commands

All under `ayx one access` unless noted. Every command accepts the global
flags (`--workspace`, `--output`, `--jq`, `--all`, `--max-pages`).

### `access matrix [--by subject|resource] [--kind <list>] [--subject <email|id>]`

The rakkess-style grid. JSON is the full `Inventory` projection
`{ subjects[], resources[], grants[], sources[], complete }`; `--output table`
renders subjects × resources with privilege abbreviations in cells and a legend;
`--by resource` transposes. `--kind connection,plan` limits fan-out. The
default (no `--all`) fetches one page per source and marks `complete: false`
when any source paginated.

### `access explain --subject <email|id> --resource <kind>/<id>`

The GCP Policy Troubleshooter model. Output:

```json
{ "effective": true,
  "paths": [
    { "source": "workspace_admin", "evidence": { "endpoint": "/v4/workspaces/91946/admins", "status": 200 } },
    { "source": "group_member", "via": { "kind": "group", "id": "…", "name": "Analysts" },
      "privileges": ["use"], "evidence": { "endpoint": "/v4/connections/…/permissions/sharedSubjects", "status": 200 } }
  ],
  "not_evaluated": [ { "source": "role", "reason": "blocked_by_scope", "endpoint": "/v4/authorization/roles/{id}/people" } ] }
```

`effective` is `true` when at least one path is live-verified. `not_evaluated`
lists every source the token or tier could not read, so the answer is honest
about its own blind spots.

### `access review --subject <email|id> [--plan-out <file>]`

Everything one person can touch: membership and admin flag, groups, roles
(through groups where readable), connection grants, plan grants, workflow
shares (if a read path exists), tokens (if visible). This is deprovisioning
prep. `--plan-out` writes a Wave 3 plan artifact (`{ schema_version:
"ayx.plan.v1", steps: [{ command: [...argv], mutating: true, idempotency_key }]
}`) plus a `commands` array of the equivalent `ayx … --apply` invocations so the
plan is usable before `ayx apply` exists. Writing the file contacts no server.

### `access graph [--format json|dot|mermaid]`

Subjects, resources, and grants as a graph. `dot` renders with Graphviz;
`mermaid` pastes into the docs site or Confluence. Edge labels are privileges;
edge style distinguishes `direct` from carried (`group_member`, `role`).

### `access diff --since <duration|date>` and `snapshot take|list|diff|export`

- `snapshot take [--out <path>]` writes the `Inventory` as content-addressed
  JSON under `${AYX_CONFIG_HOME}/snapshots/<workspace-id>/<taken_at>-<sha256[..12]>.json`,
  redacted with the standard rules, and prints its path and hash.
- `snapshot list` enumerates snapshots for the current workspace.
- `snapshot diff <a> <b>` emits `{ added_grants[], removed_grants[],
  changed_grants[], added_subjects[], removed_subjects[], … }`. Identity of a
  grant is `(subject.id, resource.kind, resource.id, source, via.id)`.
- `snapshot export --format parquet|csv [--out <dir>]` writes one file per
  entity so users can run DuckDB or a spreadsheet over it. This is the
  deliberate substitute for embedding SQL (see the roadmap's deferred list).
- `access diff --since 30d` is sugar: take a fresh snapshot, find the newest
  snapshot older than the cutoff, diff them.

### `one token audit [--stale-days N]`

Lists tokens with age and, if the API returns it, last-used; flags tokens older
than `--stale-days` (default 90). Scope (own vs all) follows the checklist
answer and is stated in the envelope.

### `telemetry permissions` → alias

`telemetry permissions connections|workflows|summary --source one` become thin
aliases over `access` for one release cycle, then are removed. The `--source
server` paths in `telemetry permissions` are untouched and stay in `telemetry`
until `ayx server access` exists.

## Implementation shape

New module `ayx-rs/src/cmd/one_access/`:

- `collector.rs` — one function per source, each returning
  `SourceResult { grants, subjects, resources, status, evidence }`. Reuses
  `one_api_list_request` and `one_api_live_request`. The connection collector
  is lifted from `telemetry/permissions.rs:connections_one` and fixes the
  review lead in `api-surface-and-observability.md` that
  `extract_shared_subject_ids` collapses the person and group buckets.
- `inventory.rs` — assembles `Inventory`, computes `complete`, and holds the
  content-addressing and redaction for snapshots.
- `index.rs` — `by_subject`, `by_resource`, and the `explain` path walk.
- `render.rs` — matrix table, graph formats.
- `snapshot.rs` — store, list, diff, export (parquet via `arrow`/`parquet`
  crates is a size cost; evaluate against `csv` + `jsonl` first and record the
  choice in the plan).

`catalog.rs` metadata: every `access` and `snapshot` command `read-only`,
`mutating: no`, `prerequisites: "One PAT for the target workspace"`.

## Live re-verification checklist (before the plan)

Run against a real tenant with `AYX_ONE_API_ACCESS_TOKEN`; record each result
in `docs/one-endpoint-matrix.md`.

1. `GET /v4/workspaces/{numericId}/admins` — confirm 200 and shape.
2. Groups: does a members list exist for a group? Which endpoint?
3. Groups: does a `GET` of a group's roles exist (counterpart to
   `set-group-roles`)?
4. Cloud-native workflow shares: is there any read path for current shares?
5. `GET /v4/apiAccessTokens`: does a workspace admin see all tokens or only
   their own? Does the record carry a last-used timestamp?
6. Plan permissions on the `platform_packaging` tier: confirm `not_applicable`
   handling.
7. `GET /v4/flows/{id}/permissions` and `/v4/authorization/roles/{id}/people`:
   re-confirm 403 so `blocked_by_scope` is stated from evidence, not memory.

## Testing

- Fixture-based unit tests for every collector's normalization (redacted
  fixtures under `docs/fixtures/one-access/`), the `explain` path walk, the
  grant identity function, and `snapshot diff`.
- `matrix` and `graph` rendering tests on a small fixture inventory.
- Live-smoke tests (gated exactly like `one_live_smoke.rs`) for each source,
  asserting the documented status, including the 403s.
- `ayx actions validate` and the command-surface gate stay green.

## Out of scope

- Any mutation. Revocation runs through Wave 3's `ayx apply`.
- Alteryx Server. Separate product, separate later wave.
- Role CRUD (issue #133) and custom roles.
- An audit-log API. It does not exist; snapshots are the substitute.
- The DataHub GraphQL the web UI uses for browsing. Observed and unsupported.

## Sequencing inside the wave

1. Collector + `Inventory` + `access matrix` for people, admins, groups,
   roles, connections (the four live-200 sources).
2. `access explain`.
3. `access review` with `--plan-out`.
4. `snapshot take|list|diff` and `access diff --since`.
5. `access graph`.
6. `one token audit`, gated on checklist item 5.
7. `telemetry permissions` aliases.
