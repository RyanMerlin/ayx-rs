# Alteryx One Admin / Governance / Access Research

Date: 2026-06-27

Scope: repo-grounded inventory of the admin, governance, and access-control surface already wrapped by `ayx`, plus a separate platform-model/use-case section based on prior knowledge rather than repo evidence.

## Part A. Repo Inventory

This section is grounded in the repo only. Citations are file:line.

### A1. Important transport and safety behavior

- Mutating One API calls are dry-run by default and only execute when the thread-local `--apply` gate is enabled. That is a strong safety default for admin/governance actions. `ayx-one-api/src/lib.rs:154-166`, `ayx-one-api/src/lib.rs:726-754`
- Mutating requests also perform a workspace-identity preflight when `expected_workspace_id` is pinned, and refuse to mutate if the token is authenticated to the wrong workspace. `ayx-one-api/src/lib.rs:756-766`, `ayx-one-api/src/lib.rs:1591-1655`
- The transport attaches workspace context headers (`x-alteryx-workspace-gid` and `x-trifacta-person-workspace-id`) on live requests when available. This matters because several IAM/admin endpoints are workspace-scoped by header, not only by URL path. `ayx-one-api/src/lib.rs:93-113`, `ayx-one-api/src/lib.rs:794-810`
- Workspace credentials are explicitly modeled as workspace-bound in the CLI; switching workspaces is a first-class profile operation, and passing a mismatched workspace id fails closed. `ayx-rs/src/cmd/one_platform/workspace.rs:14-45`, `ayx-rs/src/cmd/one_platform/workspace.rs:242-291`

### A2. Users / people / workspace membership / roles / auth

| Surface | Endpoint / operation | What it does | R/W | Evidence |
|---|---|---|---|---|
| Current user | `GET /v4/people/current` via `one platform user` and `one platform person current` | Returns the current authenticated person. | Read | `ayx-rs/src/cmd/one_platform/mod.rs:39-49`, `ayx-rs/src/cmd/one_platform/person.rs:65-76` |
| People list | `GET /v4/people` via `one platform person list` | Lists people; paginated helper supports `limit`, `pageToken`, `--all`. | Read | `ayx-rs/src/main.rs:1226-1238`, `ayx-rs/src/cmd/one_platform/person.rs:32-52` |
| People count | `GET /v4/people/count` | Counts people. | Read | `ayx-rs/src/cmd/one_platform/person.rs:53-64` |
| Person detail | `GET /v4/people/{id}` | Fetches one person. | Read | `ayx-rs/src/cmd/one_platform/person.rs:77-88` |
| Person create | `POST /v4/people` | Creates a person record. | Write | `ayx-rs/src/cmd/one_platform/person.rs:147-159` |
| Person update / patch | `PUT /v4/people/{id}`, `PATCH /v4/people/{id}` | Updates person attributes. | Write | `ayx-rs/src/cmd/one_platform/person.rs:89-124` |
| Person delete | `DELETE /v4/people/{id}` | Deletes a person. Confirmation is access-change gated. | Write | `ayx-rs/src/cmd/one_platform/person.rs:125-146` |
| Password flows | `PATCH /v4/people/current/updatePassword`, `POST /v4/passwordresetrequest` | Current-user password update and password reset request. | Write | `ayx-rs/src/cmd/one_platform/person.rs:161-188` |
| Workspace list / current | `GET /v4/workspaces`, `GET /v4/workspaces/current` | Lists accessible workspaces and fetches current workspace. | Read | `ayx-rs/src/cmd/one_platform/workspace.rs:58-78`, `ayx-rs/src/cmd/one_platform/workspace.rs:135-152` |
| Workspace people | Actual wired call is `GET /v4/people` under `one platform workspace people` | Lists workspace people using workspace header context, not the documented path. Code comment says `/v4/workspaces/{id}/people` returns 404. | Read | `ayx-rs/src/cmd/one_platform/workspace.rs:213-227` |
| Workspace admins | Actual wired call is `GET /v4/people?role=admin` under `one platform workspace admins` | Lists admins using workspace header context. Code comment says `/v4/workspaces/{id}/admins` returns 404. | Read | `ayx-rs/src/cmd/one_platform/workspace.rs:228-240` |
| Invite users | `POST /v4/workspaces/{id}/people/batch` | Invites/adds users to a workspace. | Write | `ayx-rs/src/cmd/one_platform/workspace.rs:293-305` |
| Remove user | `DELETE /v4/workspaces/{workspaceId}/people/{id}` | Removes a person from a workspace. Confirmation is access-change gated. | Write | `ayx-rs/src/cmd/one_platform/workspace.rs:306-330` |
| Suspend / unsuspend users | `POST /iam/v1/workspaces/{id}/people/suspend`, `POST /iam/v1/workspaces/{id}/people/unsuspend` | Bulk-suspends or bulk-unsuspends workspace users. | Write | `ayx-rs/src/cmd/one_platform/workspace.rs:332-377` |
| Role assignments | `GET /v4/authorization/roles/{id}/people` | Lists assignments for a known role id. | Read | `ayx-rs/src/main.rs:1380-1396`, `ayx-rs/src/cmd/one_platform/role.rs:17-28` |
| Role assign / unassign | `POST` / `DELETE /v4/authorization/roles/{id}/people/{subjectId}` | Grants or revokes a role for a known subject and role. | Write | `ayx-rs/src/cmd/one_platform/role.rs:29-67` |
| Auth status / diagnose | Probe uses `GET /v4/apiAccessTokens` as validation target | Shows token posture and validates that the token can reach a safe One endpoint. | Read | `ayx-rs/src/main.rs:5693-5765` |
| Interactive login | Email OTP flow ends with `POST /v4/apiAccessTokens` | Default login flow mints a 30-day PAT after OIDC/workspace auth completes. | Write | `ayx-one-api/src/email_otp.rs:56-67`, `ayx-one-api/src/email_otp.rs:177-208` |

What the typed API layer already understands here:

- `PersonSummary` captures `id`, `email`, `full_name`, `is_admin`, `is_suspended`, `created_at`, with tolerant parsing for camel/snake case. `ayx-one-api/src/types.rs:202-252`
- `WorkspaceSummary` captures workspace ids, name, status, owner email, and timestamps. `ayx-one-api/src/types.rs:254-311`
- `RoleListPage` exists as a typed model, but there is no wired `role list` or `role detail` command that uses it. `ayx-one-api/src/types.rs:437-480`, `ayx-rs/src/main.rs:1380-1396`

### A3. Resource sharing / ACL / entitlement surfaces

| Surface | Endpoint / operation | What it does | R/W | Evidence |
|---|---|---|---|---|
| Flow permissions read | `GET /v4/flows/{id}/permissions` via `one flows permissions-get` | Reads sharing/permission state for a flow. | Read | `ayx-rs/src/cmd/one_flows.rs:440-451` |
| Flow permissions write | `POST /v4/flows/{id}/permissions` via `one flows permissions` | Writes sharing/permission changes for a flow. | Write | `ayx-rs/src/cmd/one_flows.rs:453-470` |
| Plan permissions read | `GET /plans/v1/plans/{id}/permissions` | Reads plan permissions. | Read | `ayx-rs/src/cmd/one_plans.rs:219-237` |
| Plan share | `POST /v4/plans/{id}/permissions` | Grants/shares access to a plan. | Write | `ayx-rs/src/cmd/one_plans.rs:188-206` |
| Plan permission remove | `DELETE /plans/v1/plans/{id}/permissions/{subjectId}` | Removes a plan subject grant. | Write | `ayx-rs/src/cmd/one_plans.rs:219-247` |
| Connection permissions list | `GET /v4/connections/{id}/permissions` | Lists connection grantees/permissions. | Read | `ayx-rs/src/cmd/one_connections.rs:398-418` |
| Connection permission create | `POST /v4/connections/{id}/permissions` | Grants connection access. | Write | `ayx-rs/src/cmd/one_connections.rs:419-438` |
| Connection permission detail | `GET /v4/connections/{id}/permissions/{aid}` | Inspects a single subject grant on a connection. | Read | `ayx-rs/src/cmd/one_connections.rs:439-456` |
| Connection permission delete | `DELETE /v4/connections/{id}/permissions/{aid}` | Revokes a connection grant for one subject. | Write | `ayx-rs/src/cmd/one_connections.rs:457-486` |
| Workspace asset transfer | `POST /v4/workspaces/{id}/transfer`, `PATCH /v4/workspaces/current/transfer` | Workspace-level transfer operations, including an explicit `transfer-assets` body-driven path. | Write | `ayx-rs/src/cmd/one_platform/workspace.rs:378-413` |

Access-adjacent connection governance:

| Surface | Endpoint / operation | What it does | R/W | Evidence |
|---|---|---|---|---|
| Connection list / detail / status | `GET /v4/connections`, `/v4/connections/{id}`, `/v4/connections/{id}/status` | Inventory and inspect governed/shared connections. | Read | `ayx-rs/src/cmd/one_connections.rs:120-152`, `ayx-rs/src/cmd/one_connections.rs:181-214` |
| Connection create / update / delete | `POST /v4/connections`, `PATCH /v4/connections/{id}`, `DELETE /v4/connections/{id}` | Lifecycle control over governed connections. | Write | `ayx-rs/src/cmd/one_connections.rs:153-166`, `ayx-rs/src/cmd/one_connections.rs:215-260` |
| Connector metadata defaults/detail/publish info | `GET /v4/connectorMetadata/{connector}...` | Inspects connector schema/publish behavior used to govern connection definitions. | Read | `ayx-rs/src/cmd/one_connections.rs:262-337` |
| Connector metadata overrides list/create/delete | `/v4/connectorMetadata/{connector}/overrides` | Reads/writes connector-level overrides, which is a governance lever for connection behavior. | Read/Write | `ayx-rs/src/cmd/one_connections.rs:338-395` |

### A4. Derived "who has access to what" helpers already in the repo

These are not new API endpoints; they are higher-level governance views built from the wrapped endpoints.

| Helper | Built from | What it gives | Support | Evidence |
|---|---|---|---|---|
| `telemetry permissions connections --deep` | `/v4/connections` plus per-id `/v4/connections/{id}/permissions` | Builds both per-connection grantee lists and a reverse `by_subject` index. This is the closest thing in-repo to an access matrix today. | Derived read | `ayx-rs/src/cmd/telemetry/mod.rs:164-196`, `ayx-rs/src/cmd/telemetry/permissions.rs:81-197` |
| `telemetry permissions workflows` | `/iam/v1/workspaces/{id}/people` | Treats workspace membership as workflow access authority on One. | Derived read | `ayx-rs/src/cmd/telemetry/mod.rs:175-183`, `ayx-rs/src/cmd/telemetry/permissions.rs:199-246` |
| `telemetry permissions summary` | connections list plus workspace people list | Emits summary counts for governed connections and workspace members. | Derived read | `ayx-rs/src/cmd/telemetry/permissions.rs:248-307` |

### A5. Usage, token, and audit-adjacent surfaces

| Surface | Endpoint / operation | What it does | R/W | Evidence |
|---|---|---|---|---|
| API access tokens list/detail/create/delete | `/v4/apiAccessTokens`, `/v4/apiAccessTokens/{tokenId}` | Lists, creates, inspects, and deletes personal API access tokens. Good for token hygiene and access review. | Read/Write | `ayx-rs/src/main.rs:1202-1223`, `ayx-rs/src/cmd/one_platform/token.rs:17-77` |
| Billing usage export | `GET /billing/v1/usage/export` | Exports usage/billing data. Useful for usage governance, not ACL. | Read | `ayx-rs/src/cmd/one_billing.rs:12-39` |
| Workspace / API auth posture | status/diagnose surfaces and login flows | Exposes token presence, source, claims summary, and validation target; important for admin operability. | Read/Write | `ayx-rs/src/main.rs:5693-5798`, `ayx-rs/src/cmd/one_platform/auth.rs:22-43` |

### A6. Notable gaps and contradictions in the current repo

1. Role administration is incomplete.
   There is support for listing assignments of a known role id and assigning/unassigning subjects, but no wired role enumeration, no role detail, and no role CRUD. The CLI enum only exposes `ListAssignments`, `Assign`, and `Unassign`, even though the typed API crate already has `RoleSummary`/`RoleListPage`. `ayx-rs/src/main.rs:1380-1396`, `ayx-rs/src/cmd/one_platform/role.rs:17-67`, `ayx-one-api/src/types.rs:437-480`

2. Workspace people/admin inventory metadata diverges from live wiring.
   The surface inventory still advertises `/v4/workspaces/{id}/people` and `/v4/workspaces/{workspaceId}/admins`, but the workspace command comments explicitly say those 404 and instead use `/v4/people` and `/v4/people?role=admin`. `ayx-one-api/src/inventory.rs:32-40`, `ayx-rs/src/cmd/one_platform/workspace.rs:215-240`

3. Workflow ACL support is inconsistent inside the repo.
   `one flows` wires `GET/POST /v4/flows/{id}/permissions`, but telemetry explicitly states "One has no per-flow ACL endpoint" and falls back to workspace membership as authoritative workflow access. That means "who can see/edit flow X?" is not yet a cleanly-settled model in the codebase. `ayx-rs/src/cmd/one_flows.rs:440-470`, `ayx-rs/src/cmd/telemetry/permissions.rs:9-14`, `ayx-rs/src/cmd/telemetry/permissions.rs:233-245`

4. No One audit-log surface is wired.
   The command catalog explicitly asserts there is no `one platform audit` command. The repo does have local CLI audit-artifact management, but that is not Alteryx One tenant audit logging. `ayx-rs/src/cmd/catalog.rs:242-253`, `ayx-rs/src/main.rs:372-411`

5. No group / SSO / session / OAuth-client / environment-parameter admin surfaces are wired.
   The catalog explicitly asserts that `one platform group`, `sso`, `session`, `oauth-client`, `env-param`, and `pdh` are absent. `ayx-rs/src/cmd/catalog.rs:248-254`

6. Connection governance is good, but still schema-heavy.
   The surface inventory notes that credential-backend specifics still live mostly in raw payloads instead of a richer local model, which limits higher-level governance analysis without more interpretation code. `ayx-one-api/src/inventory.rs:677-684`

7. Connector enumeration is a known API gap.
   The CLI help says connector listing is not available from the One v4 API; operators must already know a connector slug. That weakens discovery-oriented governance around allowed/used connector types. `ayx-rs/src/cmd/one_connections.rs:262-267`

8. Plans, scheduling, and billing are tier-gated.
   The repo warns these surfaces return 404 on `platform_packaging` workspaces, so some governance features are only available on enterprise-tier tenants. `ayx-rs/src/cmd/one_plans.rs:18-20`, `ayx-rs/src/cmd/one_billing.rs:12-14`, `ayx-rs/src/cmd/one_scheduling.rs:12-15`

## Part B. Platform Model And High-Value Use Cases

This section is based on prior knowledge and inference from the repo, not a fresh doc lookup. Confidence is medium. Where I am unsure, I say so explicitly.

### B1. Likely Alteryx One admin / governance model

- Primary boundary: workspace.
  My working model is that Alteryx One is fundamentally workspace-scoped. Identity, token scope, many APIs, and the repo's safety checks all reinforce that workspace is the main tenant/isolation boundary.

- Users and admins:
  Users belong to workspaces; there is at least an admin-vs-non-admin distinction on people records (`isAdmin`, `role=admin`) and explicit workspace membership management. I am less certain about the full global-vs-workspace admin hierarchy.

- RBAC and role assignment:
  The repo strongly suggests there are assignable roles (`/v4/authorization/roles/{id}/people/...`), but the current wrapper does not enumerate roles. That usually means there is a deeper RBAC model available in the platform, but the repo has not surfaced it yet.

- Asset-level sharing:
  Flows, plans, and connections appear to have share/permissions endpoints. My best inference is that direct subject grants exist for at least those assets, probably layered on top of workspace-level access and roles. Exact permission verbs and inheritance are uncertain.

- Connection governance:
  This looks like one of the stronger admin surfaces in One. The repo wraps connection CRUD, permission grants, status, connector metadata, and connector metadata overrides. That usually maps well to connection hygiene, credential policy, and "shared connection" governance use cases.

- Audit and usage:
  Billing usage export exists. I would expect some separate admin/audit/event capability in the platform, but the repo does not expose one. So I cannot claim tenant audit logs are currently available to `ayx`.

### B2. Highest-value admin / governance / access use cases

| Use case | Why it matters | Support from Part A | Notes |
|---|---|---|---|
| Connection access matrix: "who has access to connection X, and which connections does Alice have?" | High-value, concrete, frequent governance task; especially good for a TUI cross-reference view. | Yes | The repo already has connection permission list/detail/create/delete plus `telemetry permissions connections --deep`, which builds a reverse `by_subject` index. |
| Workspace roster and admin review | Basic tenant administration: who is in the workspace, who is admin, who is suspended. | Yes | `workspace people`, `workspace admins`, person detail, invite/remove, suspend/unsuspend are all wrapped. |
| User deprovision / access cleanup | Remove user from workspace, suspend access, revoke key grants. | Partial | Workspace removal/suspend is supported, and connection/plan grants can be removed. But there is no single "deprovision user everywhere" workflow, and flow grant removal is not normalized. |
| Plan access review: "who can see/edit plan X?" | Important for governed operational assets. | Partial-to-Yes | Plan permissions can be listed and revoked, and share can be posted. What is missing is an effective-permissions view that explains inherited vs direct access. |
| Flow access review: "who can see/edit flow X?" | High-value if One flows are widely shared. | Partial | The repo wires flow permissions endpoints, but telemetry simultaneously treats workflow access as workspace-wide because "One has no per-flow ACL endpoint." That inconsistency needs resolution before making this a flagship UX. |
| Effective-permissions audit for one user | Admins want "tell me everything Bob can access and why." | Partial | You can combine workspace membership, connection grants, plan grants, token ownership, and maybe flow permissions, but there is no role catalog and no effective-permission evaluator. |
| Over-shared or orphaned connections | High governance value; finds broad grants, stale owners, and risky shared credentials. | Partial | The raw connection list and permissions are there, plus connector metadata/overrides. Missing pieces are richer owner modeling, policy rules, and automated heuristics. |
| Bulk revoke / bulk access edits | Common admin pain point where TUI can beat CLI. | Partial | The underlying APIs support per-item writes, but there are no bulk endpoints or built-in orchestration flows yet. A TUI/agent could still batch them client-side. |
| API token hygiene | Useful for least-privilege and stale-token cleanup. | Yes | List/detail/create/delete of API access tokens is already wrapped cleanly. |
| Cross-workspace admin drift review | Important in multi-workspace organizations: who is admin where, which workspace is missing controls. | Partial | Workspace list/switch exists, but most operations are workspace-bound and the repo assumes per-workspace credentials. This is feasible but not turnkey. |
| Usage-versus-access review | Valuable governance question: who can access expensive assets vs who is actually consuming. | Partial | Billing usage export exists, but there is no join to principals/assets and no access-plus-usage report. |
| Tenant audit timeline for access changes | Important for incident response and compliance. | None | No One audit-log API surface is wrapped today; only local CLI audit artifacts exist. |
| Role hygiene review | Who has privileged roles, which roles are unused, where are admin-like grants concentrated. | Partial | Assignments for a known role id are supported, but there is no role discovery/list/detail surface, so you cannot start from "show me all roles." |

## Gap Summary

| Capability | API support today | Value |
|---|---|---|
| Connection access matrix and reverse subject index | Yes | High |
| Workspace roster, admins, suspension, invite/remove | Yes | High |
| Plan sharing and plan permission review | Partial | High |
| Flow sharing / flow permission review | Partial | High |
| Effective-permissions audit for one user | Partial | High |
| Bulk deprovision / revoke across surfaces | Partial | High |
| Role hygiene / privileged-role review | Partial | High |
| Cross-workspace access governance | Partial | High |
| Token hygiene / stale PAT cleanup | Yes | Medium |
| Usage-versus-access reporting | Partial | Medium |
| Audit trail of access changes | None | High |
| Group / SSO / session / OAuth-client governance | None | Medium |

## Bottom Line

The repo already has real substance for an admin/governance-oriented `ayx` experience, especially around:

- workspace membership and admin actions
- connection permissions and connection governance
- plan permissions
- token hygiene
- derived access telemetry for connection grants

The biggest blockers to a strong "who has access to what?" TUI are:

- no role enumeration / no effective-permissions model
- no One audit-log surface
- unclear flow-permission semantics inside the repo itself
- no first-class bulk deprovision or cross-workspace governance workflows

That points to a pragmatic product direction: center the next TUI/admin work on connection access, workspace roster/admin state, token hygiene, and user-centric access review first; treat role governance, flow access truth, and audit as explicit gap-closure projects rather than assuming they already exist.
