# AYX Command Surface

_Generated from_ `cargo run -q -p ayx-rs -- --output json catalog list --format full` _on 2026-07-06 13:39:11 UTC._

This file is generated. Refresh it with:

```powershell
cargo run -q -p xtask -- refresh-command-surface
```

## Summary

- Commands: 208
- Capabilities: 8

## Commands

### `catalog`

| Name | Path | Safety | Mutating | Summary |
| --- | --- | --- | --- | --- |
| catalog describe | `catalog/describe` | read-only | no | Describe a single command in the catalog. |
| catalog list | `catalog/list` | read-only | no | List machine-readable command metadata. |

### `discover`

| Name | Path | Safety | Mutating | Summary |
| --- | --- | --- | --- | --- |
| discover | `discover` | read-only | no | Progressively discover the live CLI tree and metadata. |

### `doctor`

| Name | Path | Safety | Mutating | Summary |
| --- | --- | --- | --- | --- |
| doctor | `doctor` | read-only-or-safe-local-fix | no | Run the full ayx health sequence for config, auth, network, and product posture. |
| doctor config | `doctor/config` | read-only-or-safe-local-fix | no | Validate config home, active profile resolution, and inline secret posture. |

### `license`

| Name | Path | Safety | Mutating | Summary |
| --- | --- | --- | --- | --- |
| license api diagnose | `license/api/diagnose` | read-only | no | Validate Licensing API reachability and auth posture. |
| license api status | `license/api/status` | read-only | no | Summarize the Licensing portal API posture. |
| license inventory | `license/inventory` | read-only | no | Summarize Licensing branch inventory candidates. |
| license status | `license/status` | read-only | no | Summarize the Licensing branch posture. |

### `mongo`

| Name | Path | Safety | Mutating | Summary |
| --- | --- | --- | --- | --- |
| mongo backup | `mongo/backup` | mutating | yes | Back up the Gallery and Service Mongo databases. |
| mongo doctor | `mongo/doctor` | read-only | no | Run the default support query suite across critical Mongo collections. |
| mongo inventory | `mongo/inventory` | read-only | no | Generate an inventory plan for the Mongo-backed databases. |
| mongo query | `mongo/query` | read-only | no | Run a read-only Mongo query against a Server collection. |
| mongo restore | `mongo/restore` | mutating | yes | Restore Mongo data from a backup input path. |
| mongo status | `mongo/status` | read-only | no | Resolve the configured Mongo connection and database names. |

### `one`

| Name | Path | Safety | Mutating | Summary |
| --- | --- | --- | --- | --- |
| one auto-insights | `one/auto-insights` | read-only | no | Validate Alteryx One Auto Insights reachability and auth posture. |
| one billing current-account | `one/billing/current-account` | read-only | no | Inspect the current One billing account. |
| one billing usage-export | `one/billing/usage-export` | read-only | no | Export One billing usage data. |
| one connections connector-metadata defaults | `one/connections/connector-metadata/defaults` | read-only | no | Inspect connector defaults. |
| one connections connector-metadata detail | `one/connections/connector-metadata/detail` | read-only | no | Inspect current connector metadata. |
| one connections connector-metadata overrides create | `one/connections/connector-metadata/overrides/create` | mutating | yes | Create connector metadata overrides from JSON payload. |
| one connections connector-metadata overrides delete | `one/connections/connector-metadata/overrides/delete` | mutating | yes | Delete connector metadata overrides. |
| one connections connector-metadata overrides list | `one/connections/connector-metadata/overrides/list` | read-only | no | Inspect connector metadata overrides. |
| one connections connector-metadata publish-info | `one/connections/connector-metadata/publish-info` | read-only | no | Inspect connector publish information. |
| one connections count | `one/connections/count` | read-only | no | Count One connections. |
| one connections create | `one/connections/create` | mutating | yes | Create a One connection from JSON payload. |
| one connections delete | `one/connections/delete` | mutating | yes | Delete a One connection. |
| one connections detail | `one/connections/detail` | read-only | no | Inspect a One connection. |
| one connections dry-run | `one/connections/dry-run` | read-only | no | Dry-run creation of a One connection. |
| one connections list | `one/connections/list` | read-only | no | List One connections. |
| one connections permissions create | `one/connections/permissions/create` | mutating | yes | Create permissions for a One connection. |
| one connections permissions delete | `one/connections/permissions/delete` | mutating | yes | Delete a One connection permission by subject id. |
| one connections permissions detail | `one/connections/permissions/detail` | read-only | no | Inspect a One connection permission by subject id. |
| one connections permissions list | `one/connections/permissions/list` | read-only | no | List permissions for a One connection. |
| one connections status | `one/connections/status` | read-only | no | Inspect connection status. |
| one connections update | `one/connections/update` | mutating | yes | Update a One connection from JSON payload. |
| one datasets count | `one/datasets/count` | read-only | no | Count datasets in the One dataset library. |
| one datasets imported detail | `one/datasets/imported/detail` | read-only | no | Inspect a One imported dataset by id. |
| one datasets list | `one/datasets/list` | read-only | no | List datasets in the One dataset library. |
| one datasets wrangled count | `one/datasets/wrangled/count` | read-only | no | Count One wrangled datasets. |
| one datasets wrangled detail | `one/datasets/wrangled/detail` | read-only | no | Inspect a One wrangled dataset by id. |
| one datasets wrangled list | `one/datasets/wrangled/list` | read-only | no | List One wrangled datasets. |
| one desktop-exec | `one/desktop-exec` | read-only | no | Summarize the Alteryx One desktop execution posture. |
| one doctor auth | `one/doctor/auth` | read-only | no | Run the One auth doctor workflow. |
| one doctor billing | `one/doctor/billing` | read-only | no | Run the One billing doctor workflow. |
| one doctor discover | `one/doctor/discover` | read-only | no | Run the One discovery doctor workflow. |
| one doctor plans | `one/doctor/plans` | read-only | no | Run the One plans doctor workflow. |
| one doctor platform | `one/doctor/platform` | read-only | no | Run the One platform doctor workflow. |
| one doctor scheduling | `one/doctor/scheduling` | read-only | no | Run the One scheduling doctor workflow. |
| one flows copy | `one/flows/copy` | mutating | yes | Copy a One flow using a JSON payload. |
| one flows count | `one/flows/count` | read-only | no | Count One flows. |
| one flows create | `one/flows/create` | mutating | yes | Create a One flow from JSON payload. |
| one flows delete | `one/flows/delete` | mutating | yes | Delete a One flow. |
| one flows detail | `one/flows/detail` | read-only | no | Inspect a One flow by id. |
| one flows export | `one/flows/export` | read-only | no | Export a flow package to disk. |
| one flows export-dry-run | `one/flows/export-dry-run` | read-only | no | Dry-run export of a flow package. |
| one flows folders count | `one/flows/folders/count` | read-only | no | Count flow folders. |
| one flows folders create | `one/flows/folders/create` | mutating | yes | Create a flow folder from JSON payload. |
| one flows folders delete | `one/flows/folders/delete` | mutating | yes | Delete a flow folder. |
| one flows folders detail | `one/flows/folders/detail` | read-only | no | Inspect a flow folder by id. |
| one flows folders flows count | `one/flows/folders/flows/count` | read-only | no | Count flows in a folder. |
| one flows folders flows list | `one/flows/folders/flows/list` | read-only | no | List flows in a folder. |
| one flows folders list | `one/flows/folders/list` | read-only | no | List flow folders. |
| one flows folders update | `one/flows/folders/update` | mutating | yes | Update a flow folder from JSON payload. |
| one flows import | `one/flows/import` | mutating | yes | Import a flow package. |
| one flows import-dry-run | `one/flows/import-dry-run` | read-only | no | Dry-run import of a flow package. |
| one flows inputs | `one/flows/inputs` | read-only | no | List inputs for a One flow. |
| one flows library count | `one/flows/library/count` | read-only | no | Count the One flow library. |
| one flows library list | `one/flows/library/list` | read-only | no | List the One flow library. |
| one flows list | `one/flows/list` | read-only | no | List One flows. |
| one flows move | `one/flows/move` | mutating | yes | Move a One flow from JSON payload. |
| one flows outputs | `one/flows/outputs` | read-only | no | List outputs for a One flow. |
| one flows parameters | `one/flows/parameters` | read-only | no | Inspect flow-level parameters and overrides. |
| one flows permissions | `one/flows/permissions` | mutating | yes | Share a flow from JSON payload. |
| one flows permissions-get | `one/flows/permissions-get` | read-only | no | List permissions for a One flow. |
| one flows replace-dataset | `one/flows/replace-dataset` | mutating | yes | Replace a dataset in a One flow from JSON payload. |
| one flows run | `one/flows/run` | mutating | yes | Run a One flow using a JSON payload. |
| one flows update | `one/flows/update` | mutating | yes | Update a One flow from JSON payload. |
| one flows validate | `one/flows/validate` | read-only | no | Validate a One flow. |
| one inventory | `one/inventory` | read-only | no | Show One API inventory, or redirect One-only profiles to the platform doctor. |
| one job-group cancel | `one/job-group/cancel` | mutating | yes | Cancel a One job group. |
| one job-group count | `one/job-group/count` | read-only | no | Count One job groups. |
| one job-group detail | `one/job-group/detail` | read-only | no | Inspect a One job group. |
| one job-group inputs | `one/job-group/inputs` | read-only | no | List One job group inputs. |
| one job-group jobs | `one/job-group/jobs` | read-only | no | List jobs for a One job group. |
| one job-group list | `one/job-group/list` | read-only | no | List One job groups. |
| one job-group outputs | `one/job-group/outputs` | read-only | no | List One job group outputs. |
| one job-group pdf-results | `one/job-group/pdf-results` | read-only | no | Inspect PDF results for a One job group. |
| one job-group profile | `one/job-group/profile` | read-only | no | Inspect profile data for a One job group. |
| one job-group profile-results | `one/job-group/profile-results` | read-only | no | Inspect profile results for a One job group. |
| one job-group publications | `one/job-group/publications` | read-only | no | List publications for a One job group. |
| one job-group publish | `one/job-group/publish` | mutating | yes | Publish job-group results to a target. |
| one job-group run | `one/job-group/run` | mutating | yes | Run a One job group. |
| one job-group status | `one/job-group/status` | read-only | no | Inspect a One job group status. |
| one output-object count | `one/output-object/count` | read-only | no | Count One output objects. |
| one output-object create | `one/output-object/create` | mutating | yes | Create a One output object from JSON payload. |
| one output-object delete | `one/output-object/delete` | mutating | yes | Delete a One output object. |
| one output-object detail | `one/output-object/detail` | read-only | no | Inspect a One output object. |
| one output-object inputs | `one/output-object/inputs` | read-only | no | List inputs for a One output object. |
| one output-object list | `one/output-object/list` | read-only | no | List One output objects. |
| one output-object update | `one/output-object/update` | mutating | yes | Update a One output object from JSON payload. |
| one output-object wrangle-to-python | `one/output-object/wrangle-to-python` | read-only | no | Generate Python from a One output object. |
| one plans count | `one/plans/count` | read-only | no | Count One plans. |
| one plans create | `one/plans/create` | mutating | yes | Create a One plan. |
| one plans delete | `one/plans/delete` | mutating | yes | Delete a One plan. |
| one plans detail | `one/plans/detail` | read-only | no | Inspect a One plan. |
| one plans export | `one/plans/export` | read-only | no | Fetch a One plan package. |
| one plans full | `one/plans/full` | read-only | no | Inspect a One plan with the full documented payload. |
| one plans import | `one/plans/import` | mutating | yes | Import a One plan package. |
| one plans list | `one/plans/list` | read-only | no | List One plans. |
| one plans permissions | `one/plans/permissions` | mutating | yes | List plan permissions, or delete one when `--subject-id` is provided. |
| one plans run | `one/plans/run` | mutating | yes | Run a One plan. |
| one plans run-parameters | `one/plans/run-parameters` | read-only | no | Inspect run parameters for a One plan. |
| one plans schedules | `one/plans/schedules` | read-only | no | List schedules for a One plan. |
| one plans share | `one/plans/share` | mutating | yes | Share a One plan from JSON payload. |
| one plans status | `one/plans/status` | read-only | no | Summarize the Alteryx One plans posture. |
| one plans update | `one/plans/update` | mutating | yes | Update a One plan from JSON payload. |
| one platform api diagnose | `one/platform/api/diagnose` | read-only | no | Validate One platform API reachability and auth posture. |
| one platform api open-api-spec | `one/platform/api/open-api-spec` | read-only | no | Fetch the One platform OpenAPI specification. |
| one platform api status | `one/platform/api/status` | read-only | no | Summarize the Alteryx One platform API posture. |
| one platform auth diagnose | `one/platform/auth/diagnose` | read-only | no | Validate One API token reachability and workspace scope. |
| one platform auth status | `one/platform/auth/status` | read-only | no | Summarize One API token posture for managed IAM. |
| one platform inventory | `one/platform/inventory` | read-only | no | Summarize the current One API surface registry. |
| one platform person count | `one/platform/person/count` | read-only | no | Count One people. |
| one platform person create | `one/platform/person/create` | mutating | yes | Create a One person from JSON payload. |
| one platform person current | `one/platform/person/current` | read-only | no | Inspect the current One person record. |
| one platform person delete | `one/platform/person/delete` | mutating | yes | Delete a One person record. |
| one platform person detail | `one/platform/person/detail` | read-only | no | Inspect a One person record by id. |
| one platform person list | `one/platform/person/list` | read-only | no | List One people. |
| one platform person password-reset-request | `one/platform/person/password-reset-request` | mutating | yes | Request a One password reset from JSON payload. |
| one platform person patch | `one/platform/person/patch` | mutating | yes | Patch a One person record from JSON payload. |
| one platform person update | `one/platform/person/update` | mutating | yes | Replace a One person record from JSON payload. |
| one platform person update-password | `one/platform/person/update-password` | mutating | yes | Update the current One person's password from JSON payload. |
| one platform role assign | `one/platform/role/assign` | mutating | yes | Assign a subject to a One managed IAM role. |
| one platform role list-assignments | `one/platform/role/list-assignments` | read-only | no | Inspect role assignments for One managed IAM. |
| one platform role unassign | `one/platform/role/unassign` | mutating | yes | Unassign a subject from a One managed IAM role. |
| one platform status | `one/platform/status` | read-only | no | Summarize the Alteryx One platform posture. |
| one platform token create | `one/platform/token/create` | mutating | yes | Create a One API access token from JSON payload. |
| one platform token delete | `one/platform/token/delete` | mutating | yes | Delete a One API access token by id. |
| one platform token detail | `one/platform/token/detail` | read-only | no | Inspect a One API access token by id. |
| one platform token list | `one/platform/token/list` | read-only | no | List One API access tokens. |
| one platform user | `one/platform/user` | read-only | no | Show the current One user profile. |
| one platform workspace admins | `one/platform/workspace/admins` | read-only | no | List workspace admins. |
| one platform workspace configuration | `one/platform/workspace/configuration` | read-only | no | Inspect a One workspace configuration by id. |
| one platform workspace configuration-schema | `one/platform/workspace/configuration-schema` | read-only | no | Inspect the workspace configuration schema. |
| one platform workspace configuration-v4 | `one/platform/workspace/configuration-v4` | read-only | no | Inspect a One workspace configuration by id. |
| one platform workspace current | `one/platform/workspace/current` | read-only | no | Inspect the current One workspace posture. |
| one platform workspace current-configuration | `one/platform/workspace/current-configuration` | read-only | no | Inspect the current One workspace configuration. |
| one platform workspace current-configuration-schema | `one/platform/workspace/current-configuration-schema` | read-only | no | Inspect the current workspace configuration schema. |
| one platform workspace delete-configuration | `one/platform/workspace/delete-configuration` | mutating | yes | Reset a workspace configuration by workspace id. |
| one platform workspace delete-current-configuration | `one/platform/workspace/delete-current-configuration` | mutating | yes | Reset the current workspace configuration. |
| one platform workspace invite-users | `one/platform/workspace/invite-users` | mutating | yes | Invite users to a One workspace. |
| one platform workspace list | `one/platform/workspace/list` | read-only | no | List accessible One workspaces. |
| one platform workspace people | `one/platform/workspace/people` | read-only | no | List people in the current One workspace. |
| one platform workspace remove-user | `one/platform/workspace/remove-user` | mutating | yes | Remove a user from a One workspace. |
| one platform workspace save-configuration-v4 | `one/platform/workspace/save-configuration-v4` | mutating | yes | Update a One workspace configuration by id from JSON payload. |
| one platform workspace save-current-configuration | `one/platform/workspace/save-current-configuration` | mutating | yes | Update the current One workspace configuration from JSON payload. |
| one platform workspace suspend-users | `one/platform/workspace/suspend-users` | mutating | yes | Suspend users in a One workspace. |
| one platform workspace switch | `one/platform/workspace/switch` | mutating | yes | Set the active One workspace in the local profile. |
| one platform workspace transfer | `one/platform/workspace/transfer` | mutating | yes | Start a transfer for a One workspace. |
| one platform workspace transfer-assets | `one/platform/workspace/transfer-assets` | mutating | yes | Transfer assets from the current One workspace from JSON payload. |
| one platform workspace unsuspend-users | `one/platform/workspace/unsuspend-users` | mutating | yes | Unsuspend users in a One workspace. |
| one scheduling count | `one/scheduling/count` | read-only | no | Count One schedules. |
| one scheduling detail | `one/scheduling/detail` | read-only | no | Inspect a One schedule by id. |
| one scheduling disable | `one/scheduling/disable` | mutating | yes | Disable a One schedule. |
| one scheduling enable | `one/scheduling/enable` | mutating | yes | Enable a One schedule. |
| one scheduling list | `one/scheduling/list` | read-only | no | List One schedules. |
| one status | `one/status` | read-only | no | Show One API status, or redirect One-only profiles to the platform doctor. |
| one webhook-flow-task create | `one/webhook-flow-task/create` | mutating | yes | Create a webhook flow task from JSON payload. |
| one webhook-flow-task delete | `one/webhook-flow-task/delete` | mutating | yes | Delete a webhook flow task. |
| one webhook-flow-task detail | `one/webhook-flow-task/detail` | read-only | no | Inspect a webhook flow task. |
| one webhook-flow-tasks test | `one/webhook-flow-tasks/test` | mutating | yes | Send a test webhook from JSON payload. |
| one write-setting count | `one/write-setting/count` | read-only | no | Count One write settings. |
| one write-setting create | `one/write-setting/create` | mutating | yes | Create a One write setting from JSON payload. |
| one write-setting delete | `one/write-setting/delete` | mutating | yes | Delete a One write setting. |
| one write-setting detail | `one/write-setting/detail` | read-only | no | Inspect a One write setting. |
| one write-setting list | `one/write-setting/list` | read-only | no | List One write settings. |
| one write-setting update | `one/write-setting/update` | mutating | yes | Update a One write setting from JSON payload. |

### `profile`

| Name | Path | Safety | Mutating | Summary |
| --- | --- | --- | --- | --- |
| profile current | `profile/current` | read-only | no | Show the active central profile pointer. |
| profile list | `profile/list` | read-only | no | List centrally managed profiles and show the active profile. |
| profile use | `profile/use` | mutating-local | yes | Set the active central profile. |

### `server`

| Name | Path | Safety | Mutating | Summary |
| --- | --- | --- | --- | --- |
| server api call | `server/api/call` | mutating-or-read-only | no | Invoke a Server API operation by operationId. |
| server api diagnose | `server/api/diagnose` | read-only | no | Validate token acquisition and API reachability for Server. |
| server api import-swagger | `server/api/import-swagger` | read-only | no | Download and cache the Server OpenAPI document. |
| server api status | `server/api/status` | read-only | no | Summarize Server API credentials and base URL posture. |
| server auth diagnose ad-legacy | `server/auth/diagnose/ad-legacy` | read-only | no | Inspect legacy Active Directory auth support signals. |
| server auth diagnose certificate | `server/auth/diagnose/certificate` | read-only | no | Inspect certificate posture for SAML auth. |
| server auth diagnose saml | `server/auth/diagnose/saml` | read-only | no | Inspect SAML configuration, metadata, and callback alignment. |
| server auth diagnose saml-logs | `server/auth/diagnose/saml-logs` | read-only | no | Collect and summarize SAML login logs. |
| server auth simulate saml | `server/auth/simulate/saml` | read-only | no | Simulate a SAML auth flow using metadata and expected endpoints. |
| server auth status | `server/auth/status` | read-only | no | Summarize Server authentication configuration. |
| server diagnose startup | `server/diagnose/startup` | read-only | no | Run a guided startup failure diagnosis. |
| server diagnose tls | `server/diagnose/tls` | read-only | no | Inspect TLS, certificate, and proxy-related Server checks. |
| server doctor startup | `server/doctor/startup` | read-only | no | Run a guided startup doctor workflow. |
| server upgrade plan | `server/upgrade/plan` | read-only | no | Compute an upgrade path between versions. |

### `server-logs`

| Name | Path | Safety | Mutating | Summary |
| --- | --- | --- | --- | --- |
| server-logs context | `server-logs/context` | read-only | no | Extract surrounding lines around every occurrence of a query string in a log file. |
| server-logs discover | `server-logs/discover` | read-only | no | Inventory every Server log file the profile knows about. |
| server-logs inventory | `server-logs/inventory` | read-only | no | Aggregate counts and time ranges across all Server logs. |
| server-logs summary | `server-logs/summary` | read-only | no | Summarize a single log file (line count, error count, time range). |

### `workflow`

| Name | Path | Safety | Mutating | Summary |
| --- | --- | --- | --- | --- |
| workflow inspect | `workflow/inspect` | read-only | no | Inspect Alteryx workflow, macro, package, or data artifacts. |
| workflow migrate | `workflow/migrate` | mutating | yes | Perform an end-to-end workflow XML migration pass. |
| workflow publish | `workflow/publish` | mutating | yes | Republish a workflow package through the Server API. |
| workflow recurse | `workflow/recurse` | mutating | yes | Recursively apply XML replacement rules across workflow artifacts. |
| workflow repackage | `workflow/repackage` | mutating | yes | Rebuild a .yxzp package from a directory tree. |
| workflow replace | `workflow/replace` | mutating | yes | Find and replace text in workflow XML or packages. |
| workflow scan | `workflow/scan` | read-only | no | Preflight scan workflow artifacts for rule matches without rewriting. |
| workflow unpack | `workflow/unpack` | read-only | no | Unpack a .yxzp workflow package. |
| workflow validate | `workflow/validate` | read-only | no | Validate workflow and macro XML structures. |

## Capabilities

| Id | Provider | Safety | Available | Tags | Summary |
| --- | --- | --- | --- | --- | --- |
| `cloud.docs.search` | cloud_remote | read-only | no | cloud<br />docs<br />search<br />remote | Search cloud-side documentation capabilities when remote support is available. |
| `cloud.workflow.summarize` | hybrid | read-only | no | cloud<br />workflow<br />hybrid | Summarize cloud workflow posture when the remote contract is available. |
| `designer.tool.add` | designer_local | mutating | yes | designer<br />tool<br />mutating<br />local | Add a tool node to a local workflow XML document. |
| `designer.tool.edit` | designer_local | mutating | yes | designer<br />tool<br />mutating<br />local | Replace a tool node in a local workflow XML document. |
| `designer.tool.remove` | designer_local | mutating | yes | designer<br />tool<br />mutating<br />local | Remove a tool node and related connections from a local workflow XML document. |
| `designer.tool.replace-connections` | designer_local | mutating | yes | designer<br />tool<br />connections<br />mutating<br />local | Apply connection-fragment replacements inside a local workflow XML document. |
| `designer.workflow.context` | designer_local | read-only | yes | designer<br />workflow<br />context<br />local | Build local workflow context from a workflow XML artifact. |
| `designer.workflow.run` | designer_local | read-only | yes | designer<br />workflow<br />run<br />local | Run the local workflow capability surface with dry-run-aware validation. |

## Non-goals for This Doc

This spec intentionally does not duplicate:

- every leaf command
- every payload schema
- every API endpoint path
- every implementation detail of module layout

Those details belong in command help, the catalog surface, targeted handoff docs, or generated references.
