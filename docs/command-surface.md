# AYX Command Surface

_Generated from_ `cargo run -q -p ayx-rs -- --output json catalog list --format full --scope all` _on 2026-07-17 06:57:30 UTC._

This is the full, flattened **catalog** index — every visible node in the live `clap` command tree, one row per command, plus every registered capability. Command identity (`name`, `path`) and `summary` are derived live from the clap tree at generation time, so a command can never be silently missing here. `Safety`/`Mutating` reflect catalog metadata: commands with a curated metadata entry show that classification; every other command is honestly marked `unclassified` (blank `Mutating`) rather than borrowing a value that would misrepresent it — see `ayx catalog list --scope curated` for the fully annotated compatibility view.

For flags, positional arguments, aliases, payload schemas, and nested tree traversal, use `ayx --help`, `ayx <group> --help`, or `ayx discover --deep`.

This file is generated. Refresh it with:

```powershell
cargo run -q -p xtask -- refresh-command-surface
```

## Summary

- Commands: 345
- Capabilities: 6

## Commands

### `actions`

| Name | Path | Safety | Mutating | Summary |
| --- | --- | --- | --- | --- |
| actions | `actions` | unclassified |  | Action registry — named playbooks with safety, validation, and rollback notes |
| actions describe | `actions/describe` | unclassified |  | Describe a single action: steps, validations, rollback, plus its effective `input_schema` (declared or inferred, tagged by `input_schema_source`) and declared `output_schema`, if any — the agent-facing source of truth for what this action requires/returns |
| actions export | `actions/export` | unclassified |  | Print an action's full YAML so an operator can fork it into their config home (`${AYX_CONFIG_HOME}/registry/`) to override the bundled stdlib version |
| actions list | `actions/list` | unclassified |  | List every action, with title, safety classification, and tags. Compact index only — no input/output schema. Call `describe` on a candidate id for its full contract before constructing `--param`s |
| actions resolve | `actions/resolve` | unclassified |  | Resolve a free-text task description to a ranked list of candidate actions. Ranking/lookup only — no schema. Call `describe` on the chosen id for its full contract before constructing `--param`s |
| actions run | `actions/run` | unclassified |  | Execute an action. Without `--apply`, mutating/destructive actions emit a structured plan and never invoke a subprocess. Read-only actions always run |
| actions validate | `actions/validate` | unclassified |  | Cross-check every step in every loaded action against the catalog. Emits warnings for unknown command paths, capability ids, and dangling workflow → action references. Read-only |
| actions workflows | `actions/workflows` | unclassified |  | Workflow registry — higher-order skills composing actions |
| actions workflows explain | `actions/workflows/explain` | unclassified |  | Explain a workflow: title, safety, ordered action ids with summaries, resolved/missing action detail, plus its effective `input_schema` (declared or inferred, tagged by `input_schema_source`) and declared `output_schema`, if any — the agent-facing source of truth for what this workflow requires/returns |
| actions workflows list | `actions/workflows/list` | unclassified |  | List every workflow with its title, safety, and action count. Compact index only — no input/output schema. Call `explain` on a candidate id for its full contract before constructing `--param`s |
| actions workflows run | `actions/workflows/run` | unclassified |  | Execute a workflow as an ordered chain of actions. Honors the same `--apply` semantics as `actions run` |

### `audit`

| Name | Path | Safety | Mutating | Summary |
| --- | --- | --- | --- | --- |
| audit | `audit` | unclassified |  | Audit artifact management — list, sweep, retention. Audit files live under ${AYX_CONFIG_HOME}/audits/ by default. |
| audit status | `audit/status` | unclassified |  | Show the resolved audit directory and a quick file count / size summary |
| audit sweep | `audit/sweep` | unclassified |  | Delete audit artifacts older than `--retain-days`. Dry-run by default |

### `catalog`

| Name | Path | Safety | Mutating | Summary |
| --- | --- | --- | --- | --- |
| catalog | `catalog` | unclassified |  | Machine-readable command registry |
| catalog describe | `catalog/describe` | read-only | no | Describe a single command in the catalog. |
| catalog list | `catalog/list` | read-only | no | List machine-readable command metadata. |
| catalog run | `catalog/run` | unclassified |  | Run a registered capability with JSON input |

### `completions`

| Name | Path | Safety | Mutating | Summary |
| --- | --- | --- | --- | --- |
| completions | `completions` | unclassified |  | Generate shell completion scripts (bash, zsh, fish, powershell, elvish) |

### `designer`

| Name | Path | Safety | Mutating | Summary |
| --- | --- | --- | --- | --- |
| designer | `designer` | unclassified |  | Alteryx Designer / Server artifact tooling — .yxmd/.yxmc/.yxzp/.yxdb |
| designer workflow | `designer/workflow` | unclassified |  | Workflow package and XML tooling for .yxmd, .yxmc, .yxzp, and .yxdb |
| designer workflow convert-cloud | `designer/workflow/convert-cloud` | unclassified |  | Convert a desktop workflow into cloud JSON |
| designer workflow inspect | `designer/workflow/inspect` | read-only | no | Inspect Alteryx workflow, macro, package, or data artifacts. |
| designer workflow migrate | `designer/workflow/migrate` | mutating | yes | Perform an end-to-end workflow XML migration pass. |
| designer workflow publish | `designer/workflow/publish` | mutating | yes | Republish a workflow package through the Server API. |
| designer workflow recurse | `designer/workflow/recurse` | mutating | yes | Recursively apply XML replacement rules across workflow artifacts. |
| designer workflow repackage | `designer/workflow/repackage` | mutating | yes | Rebuild a .yxzp package from a directory tree. |
| designer workflow replace | `designer/workflow/replace` | mutating | yes | Find and replace text in workflow XML or packages. |
| designer workflow scan | `designer/workflow/scan` | read-only | no | Preflight scan workflow artifacts for rule matches without rewriting. |
| designer workflow unpack | `designer/workflow/unpack` | read-only | no | Unpack a .yxzp workflow package. |
| designer workflow validate | `designer/workflow/validate` | read-only | no | Validate workflow and macro XML structures. |
| designer workflow yxdb | `designer/workflow/yxdb` | unclassified |  | Read and export .yxdb data; use --csv for export and top-level --output json for machine-readable envelopes |

### `discover`

| Name | Path | Safety | Mutating | Summary |
| --- | --- | --- | --- | --- |
| discover | `discover` | read-only | no | Progressive live discovery of the CLI tree |

### `doctor`

| Name | Path | Safety | Mutating | Summary |
| --- | --- | --- | --- | --- |
| doctor | `doctor` | read-only-or-safe-local-fix | no | Run configuration, auth, network, and product health diagnostics |
| doctor all | `doctor/all` | unclassified |  | Run every applicable diagnostic in sequence and return one merged envelope with per-check status/summary fields plus an overall rollup |
| doctor auth | `doctor/auth` | unclassified |  | Check One and Server credential posture |
| doctor config | `doctor/config` | read-only-or-safe-local-fix | no | Validate config home, active profile resolution, and inline secret posture. |
| doctor mongo | `doctor/mongo` | unclassified |  | Check Mongo mode and managed connection posture |
| doctor network | `doctor/network` | unclassified |  | Check configured One and Server network targets |
| doctor one | `doctor/one` | unclassified |  | Check One auth and workspace probe posture |
| doctor server | `doctor/server` | unclassified |  | Check Server configuration posture and next-step guidance |

### `license`

| Name | Path | Safety | Mutating | Summary |
| --- | --- | --- | --- | --- |
| license | `license` | unclassified |  | Licensing portal branch and API surface |
| license api | `license/api` | unclassified |  | Licensing portal API status and diagnostics |
| license api diagnose | `license/api/diagnose` | read-only | no | Validate Licensing API reachability and auth posture. |
| license api status | `license/api/status` | read-only | no | Summarize the Licensing portal API posture. |
| license inventory | `license/inventory` | read-only | no | Summarize Licensing branch inventory candidates. |
| license status | `license/status` | read-only | no | Summarize the Licensing branch posture. |

### `mongo`

| Name | Path | Safety | Mutating | Summary |
| --- | --- | --- | --- | --- |
| mongo | `mongo` | unclassified |  | Mongo inventory, backup, restore, query, and doctor helpers |
| mongo backup | `mongo/backup` | mutating | yes | Back up the Gallery and Service Mongo databases. |
| mongo doctor | `mongo/doctor` | read-only | no | Run the default support query suite across critical Mongo collections. |
| mongo inventory | `mongo/inventory` | read-only | no | Generate an inventory plan for the Mongo-backed databases. |
| mongo mutate | `mongo/mutate` | destructive | yes | Apply a guarded, template-based Mongo mutation with mandatory preview approval. |
| mongo query | `mongo/query` | read-only | no | Run a read-only Mongo query against a Server collection. |
| mongo restore | `mongo/restore` | mutating | yes | Restore Mongo data from a backup input path. |
| mongo status | `mongo/status` | read-only | no | Resolve the configured Mongo connection and database names. |
| mongo undo | `mongo/undo` | destructive | yes | Reverse a prior guarded Mongo mutation from its execution audit artifact. |

### `onboard`

| Name | Path | Safety | Mutating | Summary |
| --- | --- | --- | --- | --- |
| onboard | `onboard` | unclassified |  | Interactive first-run setup for config.yaml or environments.yaml with validation and secret reuse |

### `one`

| Name | Path | Safety | Mutating | Summary |
| --- | --- | --- | --- | --- |
| one | `one` | unclassified |  | Alteryx One command surface |
| one api | `one/api` | unclassified |  | Alteryx One API introspection (spec + coverage) |
| one api coverage | `one/api/coverage` | read-only | no | Diff the live One OpenAPI spec against wired commands (covered / missing / stale) |
| one api diagnose | `one/api/diagnose` | read-only | no | Validate Alteryx One API reachability and auth posture |
| one api open-api-spec | `one/api/open-api-spec` | read-only | no | Fetch the Alteryx One OpenAPI specification |
| one api status | `one/api/status` | read-only | no | Summarize the Alteryx One API posture |
| one auth | `one/auth` | unclassified |  | Summarize One API token posture for managed IAM |
| one auth diagnose | `one/auth/diagnose` | read-only | no | Validate One API token reachability and workspace scope |
| one auth status | `one/auth/status` | read-only | no | Summarize One API token posture for managed IAM |
| one billing | `one/billing` | unclassified |  | Alteryx One billing account and usage export |
| one billing current-account | `one/billing/current-account` | read-only | no | Inspect the current One billing account |
| one billing usage-export | `one/billing/usage-export` | read-only | no | Export One billing usage data |
| one connections | `one/connections` | unclassified |  | Alteryx One connections — list, create, and manage credentials |
| one connections connector-metadata | `one/connections/connector-metadata` | unclassified |  | Inspect connector metadata — defaults, detail, publish info, and overrides |
| one connections connector-metadata defaults | `one/connections/connector-metadata/defaults` | read-only | no | Inspect connector defaults |
| one connections connector-metadata detail | `one/connections/connector-metadata/detail` | read-only | no | Inspect current connector metadata |
| one connections connector-metadata overrides | `one/connections/connector-metadata/overrides` | unclassified |  | Manage connector metadata overrides |
| one connections connector-metadata overrides create | `one/connections/connector-metadata/overrides/create` | mutating | yes | Create connector metadata overrides from JSON payload |
| one connections connector-metadata overrides delete | `one/connections/connector-metadata/overrides/delete` | mutating | yes | Delete connector metadata overrides |
| one connections connector-metadata overrides list | `one/connections/connector-metadata/overrides/list` | read-only | no | Inspect connector metadata overrides |
| one connections connector-metadata publish-info | `one/connections/connector-metadata/publish-info` | read-only | no | Inspect connector publish information |
| one connections connector-metadata template | `one/connections/connector-metadata/template` | unclassified |  | Fetch connector metadata defaults and emit a fillable JSON template for use with `connections create --body <file>` |
| one connections count | `one/connections/count` | read-only | no | Count One connections |
| one connections create | `one/connections/create` | mutating | yes | Create a One connection from JSON payload |
| one connections delete | `one/connections/delete` | mutating | yes | Delete a One connection |
| one connections detail | `one/connections/detail` | read-only | no | Inspect a One connection |
| one connections dry-run | `one/connections/dry-run` | read-only | no | Dry-run creation of a One connection |
| one connections list | `one/connections/list` | read-only | no | List One connections |
| one connections permissions | `one/connections/permissions` | unclassified |  | Manage permissions for a One connection |
| one connections permissions create | `one/connections/permissions/create` | mutating | yes | Create permissions for a One connection |
| one connections permissions delete | `one/connections/permissions/delete` | mutating | yes | Delete a One connection permission by subject id |
| one connections permissions detail | `one/connections/permissions/detail` | read-only | no | Inspect a One connection permission by subject id |
| one connections permissions list | `one/connections/permissions/list` | read-only | no | List permissions for a One connection |
| one connections status | `one/connections/status` | read-only | no | Inspect connection status |
| one connections update | `one/connections/update` | mutating | yes | Update a One connection from JSON payload |
| one datasets | `one/datasets` | unclassified |  | Read datasets from the Alteryx One dataset APIs |
| one datasets count | `one/datasets/count` | read-only | no | Count datasets in the user-facing One dataset library |
| one datasets imported | `one/datasets/imported` | unclassified |  | Read imported-dataset resources |
| one datasets imported detail | `one/datasets/imported/detail` | read-only | no | Inspect an imported dataset by id |
| one datasets list | `one/datasets/list` | read-only | no | List datasets in the user-facing One dataset library |
| one datasets wrangled | `one/datasets/wrangled` | unclassified |  | Read wrangled-dataset resources |
| one datasets wrangled count | `one/datasets/wrangled/count` | read-only | no | Count wrangled datasets |
| one datasets wrangled detail | `one/datasets/wrangled/detail` | read-only | no | Inspect a wrangled dataset by id |
| one datasets wrangled list | `one/datasets/wrangled/list` | read-only | no | List wrangled datasets |
| one doctor | `one/doctor` | unclassified |  | Alteryx One configuration, auth, and product health diagnostics |
| one doctor auth | `one/doctor/auth` | read-only | no | Run the One auth doctor workflow |
| one doctor billing | `one/doctor/billing` | read-only | no | Run the One billing doctor workflow |
| one doctor discover | `one/doctor/discover` | read-only | no | Run the One discovery doctor workflow |
| one doctor identity | `one/doctor/identity` | read-only | no | Run the One identity doctor workflow |
| one doctor plans | `one/doctor/plans` | read-only | no | Run the One plans doctor workflow |
| one doctor scheduling | `one/doctor/scheduling` | read-only | no | Run the One scheduling doctor workflow |
| one flows | `one/flows` | unclassified |  | Alteryx One flows — list, run, import, and export |
| one flows copy | `one/flows/copy` | mutating | yes | Copy a One flow using a JSON payload |
| one flows count | `one/flows/count` | read-only | no | Count One flows (flat — see `flows library count` for a breakdown that includes folders) |
| one flows create | `one/flows/create` | mutating | yes | Create a One flow from JSON payload |
| one flows delete | `one/flows/delete` | mutating | yes | Delete a One flow |
| one flows detail | `one/flows/detail` | read-only | no | Inspect a One flow by id |
| one flows export | `one/flows/export` | read-only | no | Export a flow package to disk |
| one flows export-dry-run | `one/flows/export-dry-run` | read-only | no | Dry-run export of a flow package |
| one flows folders | `one/flows/folders` | unclassified |  | Manage One flow folders (list, create, update, delete, nested flows) |
| one flows folders count | `one/flows/folders/count` | read-only | no | Count flow folders |
| one flows folders create | `one/flows/folders/create` | mutating | yes | Create a flow folder from JSON payload |
| one flows folders delete | `one/flows/folders/delete` | mutating | yes | Delete a flow folder |
| one flows folders detail | `one/flows/folders/detail` | read-only | no | Inspect a flow folder by id |
| one flows folders flows | `one/flows/folders/flows` | unclassified |  | List or count flows within a folder |
| one flows folders flows count | `one/flows/folders/flows/count` | read-only | no | Count flows in a folder |
| one flows folders flows list | `one/flows/folders/flows/list` | read-only | no | List flows in a folder |
| one flows folders list | `one/flows/folders/list` | read-only | no | List flow folders |
| one flows folders update | `one/flows/folders/update` | mutating | yes | Update a flow folder from JSON payload |
| one flows import | `one/flows/import` | mutating | yes | Import a flow package |
| one flows import-dry-run | `one/flows/import-dry-run` | read-only | no | Dry-run import of a flow package |
| one flows inputs | `one/flows/inputs` | read-only | no | List inputs for a One flow |
| one flows library | `one/flows/library` | unclassified |  | Browse the One flow library: flows AND their containing folders together, unlike the flat `flows list`/`flows count` (list, count) |
| one flows library count | `one/flows/library/count` | read-only | no | Count the One flow library — returns separate flow/folder/total counts, unlike the flat `flows count` |
| one flows library list | `one/flows/library/list` | read-only | no | List the One flow library — a folder-aware view combining flows and folders, unlike the flat `flows list` |
| one flows list | `one/flows/list` | read-only | no | List One flows (flat — no folder structure; see `flows library` for a folder-aware view) |
| one flows move | `one/flows/move` | mutating | yes | Move a One flow from JSON payload |
| one flows outputs | `one/flows/outputs` | read-only | no | List outputs for a One flow |
| one flows parameters | `one/flows/parameters` | read-only | no | Inspect flow-level parameters and overrides |
| one flows permissions | `one/flows/permissions` | mutating | yes | Share a flow from JSON payload |
| one flows permissions-get | `one/flows/permissions-get` | read-only | no | List permissions for a One flow |
| one flows replace-dataset | `one/flows/replace-dataset` | mutating | yes | Replace a dataset in a One flow from JSON payload |
| one flows run | `one/flows/run` | mutating | yes | Run a One flow using a JSON payload |
| one flows update | `one/flows/update` | mutating | yes | Update a One flow from JSON payload |
| one flows validate | `one/flows/validate` | read-only | no | Validate a One flow |
| one inventory | `one/inventory` | read-only | no | Summarize the current One API surface registry |
| one job-groups | `one/job-groups` | unclassified |  | Alteryx One job groups — run, publish, and inspect |
| one job-groups cancel | `one/job-groups/cancel` | mutating | yes | Cancel a One job group |
| one job-groups count | `one/job-groups/count` | read-only | no | Count One job groups |
| one job-groups detail | `one/job-groups/detail` | read-only | no | Inspect a One job group |
| one job-groups inputs | `one/job-groups/inputs` | read-only | no | List One job group inputs |
| one job-groups jobs | `one/job-groups/jobs` | read-only | no | List jobs for a One job group |
| one job-groups list | `one/job-groups/list` | read-only | no | List One job groups |
| one job-groups outputs | `one/job-groups/outputs` | read-only | no | List One job group outputs |
| one job-groups pdf-results | `one/job-groups/pdf-results` | read-only | no | Inspect PDF results for a One job group |
| one job-groups profile | `one/job-groups/profile` | read-only | no | Inspect profile data for a One job group |
| one job-groups profile-results | `one/job-groups/profile-results` | read-only | no | Inspect profile results for a One job group |
| one job-groups publications | `one/job-groups/publications` | read-only | no | List publications for a One job group |
| one job-groups publish | `one/job-groups/publish` | mutating | yes | Publish job-group results to a target |
| one job-groups run | `one/job-groups/run` | mutating | yes | Run a One job group |
| one job-groups status | `one/job-groups/status` | read-only | no | Inspect a One job group status |
| one login | `one/login` | mutating | yes | Authenticate with Alteryx One and store credentials |
| one logout | `one/logout` | mutating | yes | Clear stored Alteryx One credentials from the active profile |
| one output-objects | `one/output-objects` | unclassified |  | Alteryx One output objects — list, create, and manage |
| one output-objects count | `one/output-objects/count` | read-only | no | Count One output objects |
| one output-objects create | `one/output-objects/create` | mutating | yes | Create a One output object from JSON payload |
| one output-objects delete | `one/output-objects/delete` | mutating | yes | Delete a One output object |
| one output-objects detail | `one/output-objects/detail` | read-only | no | Inspect a One output object |
| one output-objects inputs | `one/output-objects/inputs` | read-only | no | List inputs for a One output object |
| one output-objects list | `one/output-objects/list` | read-only | no | List One output objects |
| one output-objects update | `one/output-objects/update` | mutating | yes | Update a One output object from JSON payload |
| one output-objects wrangle-to-python | `one/output-objects/wrangle-to-python` | read-only | no | Generate Python from a One output object |
| one person | `one/person` | unclassified |  | Alteryx One person (user) management |
| one person count | `one/person/count` | read-only | no | Count One people |
| one person create | `one/person/create` | mutating | yes | Create a One person from JSON payload |
| one person current | `one/person/current` | read-only | no | Inspect the current One person record |
| one person delete | `one/person/delete` | mutating | yes | Delete a One person record |
| one person detail | `one/person/detail` | read-only | no | Inspect a One person record by id |
| one person list | `one/person/list` | read-only | no | List One people |
| one person password-reset-request | `one/person/password-reset-request` | mutating | yes | Request a One password reset from JSON payload |
| one person patch | `one/person/patch` | mutating | yes | Patch a One person record from JSON payload |
| one person update | `one/person/update` | mutating | yes | Replace a One person record from JSON payload |
| one person update-password | `one/person/update-password` | mutating | yes | Update the current One person's password from JSON payload |
| one plans | `one/plans` | unclassified |  | Alteryx One plans — list, run, share, and manage |
| one plans count | `one/plans/count` | read-only | no | Count One plans |
| one plans create | `one/plans/create` | mutating | yes | Create a One plan |
| one plans delete | `one/plans/delete` | mutating | yes | Delete a One plan |
| one plans detail | `one/plans/detail` | read-only | no | Inspect a One plan |
| one plans export | `one/plans/export` | read-only | no | Fetch a One plan package |
| one plans full | `one/plans/full` | read-only | no | Inspect a One plan with the full documented payload |
| one plans import | `one/plans/import` | mutating | yes | Import a One plan package |
| one plans list | `one/plans/list` | read-only | no | List One plans |
| one plans permissions | `one/plans/permissions` | mutating | yes | List plan permissions, or delete one when `--subject-id` is provided |
| one plans run | `one/plans/run` | mutating | yes | Run a One plan |
| one plans run-parameters | `one/plans/run-parameters` | read-only | no | Inspect run parameters for a One plan |
| one plans schedules | `one/plans/schedules` | read-only | no | List schedules for a One plan |
| one plans share | `one/plans/share` | mutating | yes | Share a One plan from JSON payload |
| one plans update | `one/plans/update` | mutating | yes | Update a One plan from JSON payload |
| one role | `one/role` | unclassified |  | Alteryx One managed-IAM role assignments |
| one role assign | `one/role/assign` | mutating | yes | Assign a subject to a One managed IAM role |
| one role list-assignments | `one/role/list-assignments` | read-only | no | Inspect role assignments for One managed IAM |
| one role unassign | `one/role/unassign` | mutating | yes | Unassign a subject from a One managed IAM role |
| one scheduling | `one/scheduling` | unclassified |  | Alteryx One schedules — list, enable, and disable |
| one scheduling count | `one/scheduling/count` | read-only | no | Count One schedules |
| one scheduling detail | `one/scheduling/detail` | read-only | no | Inspect a One schedule by id |
| one scheduling disable | `one/scheduling/disable` | mutating | yes | Disable a One schedule |
| one scheduling enable | `one/scheduling/enable` | mutating | yes | Enable a One schedule |
| one scheduling list | `one/scheduling/list` | read-only | no | List One schedules |
| one token | `one/token` | unclassified |  | Alteryx One API access token management |
| one token create | `one/token/create` | mutating | yes | Create a One API access token from JSON payload |
| one token delete | `one/token/delete` | mutating | yes | Delete a One API access token by id |
| one token detail | `one/token/detail` | read-only | no | Inspect a One API access token by id |
| one token list | `one/token/list` | read-only | no | List One API access tokens |
| one webhook-flow-tasks | `one/webhook-flow-tasks` | unclassified |  | Alteryx One webhook flow tasks — create, inspect, and test |
| one webhook-flow-tasks create | `one/webhook-flow-tasks/create` | mutating | yes | Create a webhook flow task from JSON payload |
| one webhook-flow-tasks delete | `one/webhook-flow-tasks/delete` | mutating | yes | Delete a webhook flow task |
| one webhook-flow-tasks detail | `one/webhook-flow-tasks/detail` | read-only | no | Inspect a webhook flow task |
| one webhook-flow-tasks test | `one/webhook-flow-tasks/test` | mutating | yes | Send a test webhook from JSON payload |
| one whoami | `one/whoami` | read-only | no | Show the current One user profile |
| one workspace | `one/workspace` | unclassified |  | Alteryx One workspace inspection and administration |
| one workspace admins | `one/workspace/admins` | read-only | no | List workspace admins |
| one workspace configuration | `one/workspace/configuration` | read-only | no | Inspect a One workspace configuration by id |
| one workspace configuration-schema | `one/workspace/configuration-schema` | read-only | no | Inspect the workspace configuration schema |
| one workspace configuration-v4 | `one/workspace/configuration-v4` | read-only | no | Inspect a One workspace configuration by id |
| one workspace current | `one/workspace/current` | read-only | no | Inspect the current One workspace posture |
| one workspace current-configuration | `one/workspace/current-configuration` | read-only | no | Inspect the current One workspace configuration |
| one workspace current-configuration-schema | `one/workspace/current-configuration-schema` | read-only | no | Inspect the current workspace configuration schema |
| one workspace delete-configuration | `one/workspace/delete-configuration` | mutating | yes | Reset a workspace configuration by workspace id |
| one workspace delete-current-configuration | `one/workspace/delete-current-configuration` | mutating | yes | Reset the current workspace configuration |
| one workspace invite-users | `one/workspace/invite-users` | mutating | yes | Invite users to a One workspace |
| one workspace list | `one/workspace/list` | read-only | no | List accessible One workspaces |
| one workspace people | `one/workspace/people` | read-only | no | List people in the current One workspace |
| one workspace remove-user | `one/workspace/remove-user` | mutating | yes | Remove a user from a One workspace |
| one workspace save-configuration-v4 | `one/workspace/save-configuration-v4` | mutating | yes | Update a One workspace configuration by id from JSON payload |
| one workspace save-current-configuration | `one/workspace/save-current-configuration` | mutating | yes | Update the current One workspace configuration from JSON payload |
| one workspace suspend-users | `one/workspace/suspend-users` | mutating | yes | Suspend users in a One workspace |
| one workspace switch | `one/workspace/switch` | mutating | yes | Select which authenticated workspace is active for this profile |
| one workspace transfer | `one/workspace/transfer` | mutating | yes | Start a transfer for a One workspace |
| one workspace transfer-assets | `one/workspace/transfer-assets` | mutating | yes | Transfer assets from the current One workspace from JSON payload |
| one workspace unsuspend-users | `one/workspace/unsuspend-users` | mutating | yes | Unsuspend users in a One workspace |
| one write-settings | `one/write-settings` | unclassified |  | Alteryx One write settings — list, create, and manage |
| one write-settings count | `one/write-settings/count` | read-only | no | Count One write settings |
| one write-settings create | `one/write-settings/create` | mutating | yes | Create a One write setting from JSON payload |
| one write-settings delete | `one/write-settings/delete` | mutating | yes | Delete a One write setting |
| one write-settings detail | `one/write-settings/detail` | read-only | no | Inspect a One write setting |
| one write-settings list | `one/write-settings/list` | read-only | no | List One write settings |
| one write-settings update | `one/write-settings/update` | mutating | yes | Update a One write setting from JSON payload |

### `profile`

| Name | Path | Safety | Mutating | Summary |
| --- | --- | --- | --- | --- |
| profile | `profile` | unclassified |  | Central profile registry and active profile management |
| profile current | `profile/current` | read-only | no | Show the active central profile pointer. |
| profile list | `profile/list` | read-only | no | List centrally managed profiles and show the active profile. |
| profile migrate | `profile/migrate` | unclassified |  | Migrate a legacy profile into the central registry |
| profile path | `profile/path` | unclassified |  | Show central profile storage paths |
| profile show | `profile/show` | unclassified |  | Show the resolved central profile and configured sections |
| profile use | `profile/use` | mutating-local | yes | Set the active central profile. |

### `secret`

| Name | Path | Safety | Mutating | Summary |
| --- | --- | --- | --- | --- |
| secret | `secret` | unclassified |  | Keyring secret inspection and maintenance |
| secret prune | `secret/prune` | unclassified |  | Remove orphaned keyring accounts from the pre-v0.11.0 profile_name-scoped naming scheme |

### `server`

| Name | Path | Safety | Mutating | Summary |
| --- | --- | --- | --- | --- |
| server | `server` | unclassified |  | Server discovery, logs, auth, diagnose, doctor, upgrade, and low-level API calls |
| server api | `server/api` | unclassified |  | Server API status, diagnostics, and OpenAPI-driven calls |
| server api call | `server/api/call` | mutating-or-read-only | no | Invoke a Server API operation by operationId. |
| server api diagnose | `server/api/diagnose` | read-only | no | Validate token acquisition and API reachability for Server. |
| server api import-swagger | `server/api/import-swagger` | read-only | no | Download and cache the Server OpenAPI document. |
| server api status | `server/api/status` | read-only | no | Summarize Server API credentials and base URL posture. |
| server auth | `server/auth` | unclassified |  | Server SSO/SAML auth diagnosis and simulation |
| server auth diagnose | `server/auth/diagnose` | unclassified |  | Inspect Server auth configuration and failure signals |
| server auth diagnose ad-legacy | `server/auth/diagnose/ad-legacy` | read-only | no | Inspect legacy Active Directory auth support signals. |
| server auth diagnose certificate | `server/auth/diagnose/certificate` | read-only | no | Inspect certificate posture for SAML auth. |
| server auth diagnose saml | `server/auth/diagnose/saml` | read-only | no | Inspect SAML configuration, metadata, and callback alignment. |
| server auth diagnose saml-logs | `server/auth/diagnose/saml-logs` | read-only | no | Collect and summarize SAML login logs. |
| server auth simulate | `server/auth/simulate` | unclassified |  | Simulate Server SAML authentication flows |
| server auth simulate saml | `server/auth/simulate/saml` | read-only | no | Simulate a SAML auth flow using metadata and expected endpoints. |
| server auth status | `server/auth/status` | read-only | no | Summarize Server authentication configuration. |
| server ayx-paths | `server/ayx-paths` | unclassified |  | Show common Alteryx Server filesystem paths |
| server backup | `server/backup` | unclassified |  | Run or simulate a full Server backup |
| server backup-plan | `server/backup-plan` | unclassified |  | Generate a Server backup file plan |
| server diagnose | `server/diagnose` | unclassified |  | Run targeted Server diagnostics |
| server diagnose logs | `server/diagnose/logs` | unclassified |  | Inspect Server log sources and triage targets |
| server diagnose network | `server/diagnose/network` | unclassified |  | Inspect Server network and connectivity checks |
| server diagnose runtime-settings | `server/diagnose/runtime-settings` | unclassified |  | Inspect Server runtime settings and Mongo config |
| server diagnose startup | `server/diagnose/startup` | read-only | no | Run a guided startup failure diagnosis. |
| server diagnose tls | `server/diagnose/tls` | read-only | no | Inspect TLS, certificate, and proxy-related Server checks. |
| server doctor | `server/doctor` | unclassified |  | Guided Server troubleshooting workflows |
| server doctor logs | `server/doctor/logs` | unclassified |  | Guide Server log-family triage and next steps |
| server doctor network | `server/doctor/network` | unclassified |  | Guide Server network troubleshooting checks |
| server doctor runtime-settings | `server/doctor/runtime-settings` | unclassified |  | Guide Server runtime settings validation |
| server doctor startup | `server/doctor/startup` | read-only | no | Run a guided startup doctor workflow. |
| server runtime-settings | `server/runtime-settings` | unclassified |  | Summarize RuntimeSettings.xml and export JSON |
| server server-logs | `server/server-logs` | unclassified |  | Discover, summarize, and parse Server logs |
| server server-logs context | `server/server-logs/context` | read-only | no | Extract matching context from a Server log file |
| server server-logs discover | `server/server-logs/discover` | read-only | no | Discover Server log locations from the active profile |
| server server-logs gallery-events | `server/server-logs/gallery-events` | unclassified |  | Parse Gallery log events from a log file |
| server server-logs inventory | `server/server-logs/inventory` | read-only | no | Inventory known Server log files and metadata |
| server server-logs parse-csv | `server/server-logs/parse-csv` | unclassified |  | Parse a Gallery log CSV export |
| server server-logs recent | `server/server-logs/recent` | unclassified |  | List recent Server log candidates |
| server server-logs service-events | `server/server-logs/service-events` | unclassified |  | Parse Service log events from a log file |
| server server-logs summary | `server/server-logs/summary` | read-only | no | Summarize a Server log file |
| server server-logs tail | `server/server-logs/tail` | unclassified |  | Read the tail of a Server log file |
| server system-info | `server/system-info` | unclassified |  | Capture host system information to JSON |
| server upgrade | `server/upgrade` | unclassified |  | Server upgrade planning, backup, apply simulation, and postcheck helpers |
| server upgrade apply | `server/upgrade/apply` | unclassified |  | Run or simulate an upgrade manifest |
| server upgrade backup | `server/upgrade/backup` | unclassified |  | Run a Server upgrade backup |
| server upgrade bundle | `server/upgrade/bundle` | unclassified |  | Bundle upgrade artifacts into a package |
| server upgrade path | `server/upgrade/path` | unclassified |  | Compute a supported Server upgrade path |
| server upgrade plan | `server/upgrade/plan` | read-only | no | Compute an upgrade path between versions. |
| server upgrade postcheck | `server/upgrade/postcheck` | unclassified |  | Run a Server upgrade postcheck |
| server upgrade precheck | `server/upgrade/precheck` | unclassified |  | Run a Server upgrade precheck |

### `sqlserver`

| Name | Path | Safety | Mutating | Summary |
| --- | --- | --- | --- | --- |
| sqlserver | `sqlserver` | unclassified |  | SQL Server status, prechecks, connection helpers, and migration planning |
| sqlserver connection-string | `sqlserver/connection-string` | unclassified |  | Generate a SQL Server connection string |
| sqlserver inventory | `sqlserver/inventory` | unclassified |  | Summarize SQL Server inventory and database posture |
| sqlserver migrate | `sqlserver/migrate` | unclassified |  | Generate a SQL Server migration plan |
| sqlserver precheck | `sqlserver/precheck` | unclassified |  | Run SQL Server migration prechecks |
| sqlserver prepare | `sqlserver/prepare` | unclassified |  | Generate SQL Server migration preparation guidance |
| sqlserver status | `sqlserver/status` | unclassified |  | Summarize configured SQL Server connection posture |
| sqlserver validate-strings | `sqlserver/validate-strings` | unclassified |  | Validate configured SQL Server connection strings |

### `telemetry`

| Name | Path | Safety | Mutating | Summary |
| --- | --- | --- | --- | --- |
| telemetry | `telemetry` | unclassified |  | Operational telemetry: running jobs, run history, top workflows/plans, errors, weekly run-counts |
| telemetry errors | `telemetry/errors` | unclassified |  | Recent failed-job messages with timestamps |
| telemetry errors recent | `telemetry/errors/recent` | unclassified |  | Recent failed job groups with error messages |
| telemetry jobs | `telemetry/jobs` | unclassified |  | Job-group telemetry: running, history, top |
| telemetry jobs history | `telemetry/jobs/history` | unclassified |  | Recent job-group history (succeeded + failed + cancelled) in --since window |
| telemetry jobs running | `telemetry/jobs/running` | unclassified |  | List job groups currently in Running or Queued state |
| telemetry jobs top | `telemetry/jobs/top` | unclassified |  | Top flows by run count over --since window |
| telemetry permissions | `telemetry/permissions` | unclassified |  | Who has access to which connections, workflows, and collections |
| telemetry permissions collections | `telemetry/permissions/collections` | unclassified |  | Collections / Gallery item-membership ACLs (Server only) |
| telemetry permissions connections | `telemetry/permissions/connections` | unclassified |  | DCM connections and the subjects with access to each |
| telemetry permissions summary | `telemetry/permissions/summary` | unclassified |  | Roll up access counts: connections per subject, people per workspace |
| telemetry permissions workflows | `telemetry/permissions/workflows` | unclassified |  | Who has workflow access. On One that's workspace people (no per-flow ACL endpoint); on Server it's the collections.appinfos surface |
| telemetry plans | `telemetry/plans` | unclassified |  | Plan telemetry: top by run-count, performance percentiles |
| telemetry plans performance | `telemetry/plans/performance` | unclassified |  | Per-plan duration percentiles |
| telemetry plans top | `telemetry/plans/top` | unclassified |  | Top plans by run count over --since window |
| telemetry queue | `telemetry/queue` | unclassified |  | Queue depth and wait-time stats (Server source only in Phase 2) |
| telemetry queue status | `telemetry/queue/status` | unclassified |  | Currently running + queued jobs (Server side) |
| telemetry queue wait-time | `telemetry/queue/wait-time` | unclassified |  | Wait-time stats over recent queue entries |
| telemetry summary | `telemetry/summary` | unclassified |  | One-shot overview composing the above into a single envelope |
| telemetry weekly | `telemetry/weekly` | unclassified |  | Weekly run-count matrix (7×24 buckets) — data feed for the deferred heatmap phase |
| telemetry weekly run-counts | `telemetry/weekly/run-counts` | unclassified |  | Emit a stable 168-bucket run-count matrix (day_of_week × hour) |
| telemetry workflows | `telemetry/workflows` | unclassified |  | Workflow telemetry: top by run-count / failure-rate / duration, errors |
| telemetry workflows errors | `telemetry/workflows/errors` | unclassified |  | Flows ordered by failure count over --since window |
| telemetry workflows performance | `telemetry/workflows/performance` | unclassified |  | Per-flow duration percentiles (p50/p95/p99) over --since window |
| telemetry workflows top | `telemetry/workflows/top` | unclassified |  | Top flows by run count, failure rate, or duration over --since window |

### `tools`

| Name | Path | Safety | Mutating | Summary |
| --- | --- | --- | --- | --- |
| tools | `tools` | unclassified |  | Cross-environment tools for environments.yaml source/target workflows |
| tools workspace | `tools/workspace` | unclassified |  | Cross-environment workspace scaffolding and comparison |
| tools workspace check-dcm-connections | `tools/workspace/check-dcm-connections` | unclassified |  | Scaffold cross-environment DCM connection checks |
| tools workspace compare | `tools/workspace/compare` | unclassified |  | Compare source and target workspace profiles |
| tools workspace init | `tools/workspace/init` | unclassified |  | Write an environments.yaml workspace template |
| tools workspace migrate-workflows | `tools/workspace/migrate-workflows` | unclassified |  | Scaffold cross-environment workflow migration |
| tools workspace resolve | `tools/workspace/resolve` | unclassified |  | Resolve source and target environments from a workspace |

### `tui`

| Name | Path | Safety | Mutating | Summary |
| --- | --- | --- | --- | --- |
| tui | `tui` | unclassified |  | Interactive TUI for central profile selection, explicit file editing, One credentials, and connectivity checks |

### `update`

| Name | Path | Safety | Mutating | Summary |
| --- | --- | --- | --- | --- |
| update | `update` | unclassified |  | Self-update from GitHub releases |

### `whoami`

| Name | Path | Safety | Mutating | Summary |
| --- | --- | --- | --- | --- |
| whoami | `whoami` | unclassified |  | Show active profile, account email, workspace, and environment in one shot. |

## Capabilities

| Id | Provider | Safety | Available | Tags | Summary |
| --- | --- | --- | --- | --- | --- |
| `designer.tool.add` | designer_local | mutating | yes | designer<br />tool<br />mutating<br />local | Add a tool node to a local workflow XML document. |
| `designer.tool.edit` | designer_local | mutating | yes | designer<br />tool<br />mutating<br />local | Replace a tool node in a local workflow XML document. |
| `designer.tool.remove` | designer_local | mutating | yes | designer<br />tool<br />mutating<br />local | Remove a tool node and related connections from a local workflow XML document. |
| `designer.tool.replace-connections` | designer_local | mutating | yes | designer<br />tool<br />connections<br />mutating<br />local | Apply connection-fragment replacements inside a local workflow XML document. |
| `designer.workflow.context` | designer_local | read-only | yes | designer<br />workflow<br />context<br />local | Build local workflow context from a workflow XML artifact. |
| `designer.workflow.run` | designer_local | read-only | yes | designer<br />workflow<br />run<br />local | Run the local workflow capability surface with dry-run-aware validation. |

## Non-goals for This Doc

This spec intentionally does not duplicate:

- every flag, positional argument, or alias
- every payload schema
- every API endpoint path
- every implementation detail of module layout

Those details belong in command help, `ayx discover --deep`, targeted handoff docs, or generated references.
