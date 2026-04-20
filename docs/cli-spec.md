# AYX-RS Spec (v0.2)

## Global
- Binary: `ayx`
- Output modes: `--output text|json`
- Default config file: `config.yaml`
- Envelope root fields: `ok`, `message`, `timestamp_utc`, `data`

## First Run

The fastest path from zero to useful output is:

```bash
curl -fsSL https://raw.githubusercontent.com/RyanMerlin/ayx-cli/main/scripts/install.sh | bash
```

Then create a small `config.yaml` with your Server and One credentials, and start with:

```powershell
ayx server api status --profile config.yaml
ayx mongo status --profile config.yaml
ayx catalog list
```

If you want the command to feed another tool, add `--output json`.

## Configuration

`ayx` reads JSON/YAML profiles through `ayx-core::profile::Config`. The default `config.yaml` sample in the repo demonstrates both embedded and managed Mongo scenarios, OAuth2, the shared API observability toggle, and the required Alteryx One email. `workspace.yaml` is the canonical multi-environment form; it holds multiple named `Config` entries and an explicit `active_environment`. Replace the placeholders before committing the file for production usage. When pointing at a live Server, make sure the Mongo connection string, database names, TLS artifacts, observability path, and Alteryx One email are accurate for that environment.

### Required Config Fields
- `profile_name`: user-friendly label surfaced in audit/output envelopes.
- `mongo.mode`: `embedded` or `managed`.
- `mongo.databases.gallery_name` and `mongo.databases.service_name`: required database names so every operation knows which namespaces to touch.
- For embedded mode, `mongo.embedded.runtime_settings_path` may remain null; runtime discovery handles the default Server layout.
- In managed mode, provide `mongo.managed.url` or `mongo.managed.host` plus `mongo.managed.port`. TLS (`mongo.managed.tls`) and credentials (`username`, `password`, `auth_database`) control how `mongodump/mongorestore` authenticate.
- `server_api.base_url` plus OAuth2 client-credential inputs (`server_api.client_id`, `server_api.client_secret`) for the Server API surface.
- `server_api` keeps the public config model product-scoped instead of using a generic top-level `api` section.
- future product API branches will carry their own config blocks under the product root.
- `alteryx_one.account_email` is the Alteryx One identity used throughout owner-transfer and gallery operations.

## Embedded RuntimeSettings Discovery

- `mongo.embedded.runtime_settings_path` defaults to `null`; the CLI tries the documented Server location (`C:\ProgramData\Alteryx\RuntimeSettings.xml`) first, then `%ProgramData%/Alteryx/*`, `%ProgramFiles%/Alteryx/*`, `%ProgramFiles(x86)%/Alteryx/*`, and finally probes relocated drives (for example `D:\ProgramData\Alteryx\RuntimeSettings.xml`). The CLI bails with a helpful error if no candidate exists, instructing you to set the path manually.
- `mongo.embedded.alteryx_service_path` is optional; the embedding logic derives the install root from RuntimeSettings and looks for `bin/AlteryxService.exe` before asking for an override.
- `mongo.embedded.restore_target_path` is optional; the CLI uses the runtime payload to infer the persistence target, defaulting to `C:\ProgramData\Alteryx\Service\Persistence\MongoDB` when the XML lacks the field.
- The repo includes `C:\code\RuntimeSettings.xml` as a fixture — copy your Server runtime settings there or point `mongo.embedded.runtime_settings_path` at your install when validating the embedded workflow locally.

## Mongo Commands
- `ayx mongo status --profile <path>`
- `ayx mongo inventory --profile <path>`
-- `ayx mongo backup --profile <path> --output-dir <dir> [--apply] [--audit-dir <dir>]`
-- `ayx mongo restore --profile <path> --input-path <path> [--apply] [--audit-dir <dir>]`

Mutating commands (`backup`, `restore`) default to dry-run and require `--apply` to execute. Every mutating command writes an audit artifact JSON file.

Execution behavior:
- Embedded mode uses the AlteryxService wrappers:
  - `emongodump=path`
  - `emongorestore=source,target`
- Managed mode uses the MongoDB CLI tools (`mongodump`, `mongorestore`) and honors the TLS flags, credentials, and connection-time tuning parameters configured in `mongo.managed`.

## API Commands
- `ayx server api status --profile <path>`
- `ayx server api users --profile <path> [--view Default|Full]`
- `ayx server api user-detail --profile <path> --user-id <id>`
- `ayx server api user-update --profile <path> --user-id <id> --payload-file <path> [--apply]`
- `ayx server api user-delete --profile <path> --user-id <id> [--apply]`
- `ayx server api user-assets --profile <path> --user-id <id> [--asset-type All|Workflows|Schedules|Collections]`
- `ayx server api user-transfer-assets --profile <path> --user-id <id> --payload-file <path> [--apply]`
- `ayx server api user-deactivate --profile <path> --user-id <id> [--apply]`
- `ayx server api user-password-reset --profile <path> --user-id <id> [--apply]`
- `ayx server api workflows --profile <path> [--view Default|Full]`
- `ayx server api workflow-detail --profile <path> --workflow-id <id>`
- `ayx server api workflow-jobs --profile <path> --workflow-id <id>`
- `ayx server api workflow-questions --profile <path> --workflow-id <id> [--version-id <id>]`
- `ayx server api workflow-package --profile <path> --workflow-id <id> [--version-id <id>] [--output-path <path>]`
- `ayx server api workflow-version-upload --profile <path> --workflow-id <id> --file-path <path> --name <value> --owner-id <id> [--execution-mode Safe|SemiSafe|Standard] [--workflow-credential-type Default|Required|Specific] [--others-may-download] [--others-can-execute] [--has-private-data-exemption] [--comments <text>] [--make-published] [--credential-id <id>] [--bypass-workflow-version-check] [--apply]`

## Upgrade Commands
- `ayx server upgrade path --from <source> --to <target> [--deployment embedded-mongo|user-mongo|sql]`
- `ayx server upgrade precheck --profile <path> --target <version> --out <dir> [--deployment embedded-mongo|user-mongo|sql]`
- `ayx server upgrade backup --profile <path> --type <mongo|runtime|logs|all> --out <dir>`
- `ayx server upgrade plan --from <source> --to <target> --out <dir> [--deployment embedded-mongo|user-mongo|sql]`
- `ayx server upgrade apply --manifest <path> --apply --yes`
- `ayx server upgrade postcheck --profile <path> --manifest <path> --out <dir>`
- `ayx server upgrade bundle --input <dir> --out <zip>`

Upgrade commands rely on the optional `upgrade` block in `config.yaml`, for example:

```
upgrade:
  target_version: 2024.1
  deployment: embedded-mongo
```

`server upgrade precheck` validates runtime/service expectations and curator access before evaluating the supported path between the configured `target_version` and the CLI `--target`. `server upgrade backup` captures runtime/service files, writes `backup_results.csv`, and records instructions for embedded Mongo. `server upgrade plan` writes `upgrade_plan.json` plus the hashed `plan_manifest.json` and a run manifest describing each hop. `server upgrade apply` replays the plan manifest with simulated steps (`execution_audit.csv`), while `server upgrade postcheck` verifies migration logs and the manifest hash. `server upgrade bundle` zips an input directory for sharing with operations or support.

## Update Command
- `ayx update [--repo-owner <owner>] [--repo-name <repo>] [--bin-name <name>] [--target-version <tag>] [--skip-confirm]`

`ayx update` checks the latest GitHub release (defaulting to `RyanMerlin/ayx-cli`) and, after prompting unless `--skip-confirm` is used, downloads and replaces the running binary with the release asset named for the current target triple. Use `--target-version` to install a specific tag instead of the latest release, and `--repo-owner/--repo-name` if you host releases in a different repo.
- `ayx server api workflow-detail --profile <path> --workflow-id <id>`
- `ayx server api workflow-jobs --profile <path> --workflow-id <id>`
- `ayx server api schedules --profile <path> [--view Default|Full]`
- `ayx server api schedule-detail --profile <path> --schedule-id <id>`
- `ayx server api schedule-create --profile <path> --payload-file <path> [--apply]`
- `ayx server api schedule-update --profile <path> --schedule-id <id> --payload-file <path> [--apply]`
- `ayx server api schedule-patch --profile <path> --schedule-id <id> --payload-file <path> [--apply]`
- `ayx server api schedule-delete --profile <path> --schedule-id <id> [--apply]`
- `ayx server api collections --profile <path> [--view Default|Full]`
- `ayx server api collection-detail --profile <path> --collection-id <id>`
- `ayx server api collection-create --profile <path> --name <value> [--apply]`
- `ayx server api collection-update --profile <path> --collection-id <id> --payload-file <path> [--apply]`
- `ayx server api collection-delete --profile <path> --collection-id <id> [--force] [--apply]`
 - `ayx server api collection-add-user --profile <path> --collection-id <id> --payload-file <path> [--apply]`
 - `ayx server api collection-remove-user --profile <path> --collection-id <id> --user-id <id> [--apply]`
 - `ayx server api collection-add-schedule --profile <path> --collection-id <id> --payload-file <path> [--apply]`
 - `ayx server api collection-remove-schedule --profile <path> --collection-id <id> --schedule-id <id> [--apply]`
 - `ayx server api collection-add-workflow --profile <path> --collection-id <id> --payload-file <path> [--apply]`
 - `ayx server api collection-remove-workflow --profile <path> --collection-id <id> --workflow-id <id> [--apply]`
 - `ayx server api collection-add-user-group --profile <path> --collection-id <id> --payload-file <path> [--apply]`
 - `ayx server api collection-remove-user-group --profile <path> --collection-id <id> --user-group-id <id> [--apply]`
 - `ayx server api collection-update-user-permissions --profile <path> --collection-id <id> --user-id <id> --payload-file <path> [--apply]`
 - `ayx server api collection-update-user-group-permissions --profile <path> --collection-id <id> --user-group-id <id> --payload-file <path> [--apply]`
- `ayx server api usergroups --profile <path> [--view Default|Full]`
- `ayx server api usergroup-detail --profile <path> --user-group-id <id>`
- `ayx server api usergroup-create --profile <path> --payload-file <path> [--apply]`
- `ayx server api usergroup-update --profile <path> --user-group-id <id> --payload-file <path> [--apply]`
- `ayx server api usergroup-delete --profile <path> --user-group-id <id> [--force] [--apply]`
- `ayx server api usergroup-add-users --profile <path> --user-group-id <id> --payload-file <path> [--apply]`
- `ayx server api usergroup-remove-user --profile <path> --user-group-id <id> --user-id <id> [--apply]`
- `ayx server api usergroup-add-adgroup --profile <path> --user-group-id <id> --payload-file <path> [--apply]`
- `ayx server api usergroup-remove-adgroup --profile <path> --user-group-id <id> --ad-group-id <sid> [--apply]`
- `ayx server api dcm-connections --profile <path>`
- `ayx server api dcm-connection-lookup --profile <path> --connection-id <id>`
- `ayx server api dcm-connection-share-collaboration --profile <path> --connection-id <id> --payload-file <path> [--apply]`
- `ayx server api dcm-connection-share-execution --profile <path> --connection-id <id> --payload-file <path> [--apply]`
- `ayx server api dcm-admin-connections --profile <path> [--connection-id <value>] [--visible-by <userId>]`
- `ayx server api dcm-admin-connection-detail --profile <path> --connection-id <id>`
- `ayx server api dcm-admin-connection-upsert --profile <path> --payload-file <path> [--apply]`
- `ayx server api dcm-admin-connection-delete --profile <path> --connection-id <id> [--apply]`
- `ayx server api dcm-admin-connection-remove-collaboration --profile <path> --connection-id <id> [--apply]`
- `ayx server api dcm-admin-connection-remove-execution --profile <path> --connection-id <id> [--apply]`
- `ayx server api credential-share-user --profile <path> --credential-id <id> --payload-file <path> [--apply]`
- `ayx server api credential-unshare-user --profile <path> --credential-id <id> --user-id <user> [--apply]`
- `ayx server api credential-share-user-group --profile <path> --credential-id <id> --payload-file <path> [--apply]`
- `ayx server api credential-unshare-user-group --profile <path> --credential-id <id> --user-group-id <group> [--apply]`
- `ayx server api subscriptions --profile <path> [--name <value>] [--can-share-schedule <bool>] [--default-workflow-credential-id <id>] [--user-count-gte <int>] [--user-count-lte <int>] [--workflow-count-gte <int>] [--workflow-count-lte <int>]`
- `ayx server api subscription-detail --profile <path> --subscription-id <id>`
- `ayx server api subscription-create --profile <path> --payload-file <path> [--apply]`
- `ayx server api subscription-update --profile <path> --subscription-id <id> --payload-file <path> [--apply]`
- `ayx server api subscription-delete --profile <path> --subscription-id <id> [--apply]`
- `ayx server api subscription-change-users --profile <path> --subscription-id <id> --payload-file <path> [--apply]`
- `ayx server api credentials --profile <path> [--view Default|Full] [--user-id <id>] [--user-group-id <id>]`
- `ayx server api credential-detail --profile <path> --credential-id <id>`
- `ayx server api credential-add --profile <path> --payload-file <path> [--apply]`
- `ayx server api credential-update --profile <path> --credential-id <id> --payload-file <path> [--apply]`
- `ayx server api credential-delete --profile <path> --credential-id <id> [--force] [--apply]`
- `ayx server api transfer-workflow-owner --profile <path> --workflow-id <id> --owner-id <id> [--transfer-schedules <bool>] [--apply] [--audit-dir <dir>]`

## Server Commands
- `ayx server api import-swagger --profile <path> --url <url> [--version 3] [--cache-dir .omni/swagger]`
- `ayx server api call --profile <path> --operation-id <id> [--version 3] [--cache-dir .omni/swagger] [--swagger <path>] [--param KEY=VALUE ...] [--body <path>]`

Server commands reuse `config.yaml` but require the `alteryx_server.server_api` block illustrated above (`base_url`, `client_id`, `client_secret`) in addition to the Server-specific settings already flattened by the config loader. `import-swagger` downloads the OpenAPI document for the requested version and caches it under `cache-dir/<profile>_swagger_v<version>.json`. `call` loads the cached Swagger, resolves the `operationId`, substitutes path/query parameters supplied via `--param`, and exchanges JSON payloads with the Server API using the curator credentials so automation can inspect `status_code`, `url`, and the parsed response.

## Product-Scoped API Branches

The CLI is intentionally product-first:

- `ayx server api`
- `ayx license api`
- `ayx one platform api`
- `ayx one platform auth`
- `ayx one plans`
- `ayx one scheduling`
- `ayx one billing`
- `ayx one auto-insights`
- `ayx one desktop-exec`

`server` is the mature branch today. `license` and `one` are separate product roots so each Alteryx surface can grow independently. `one platform` is the managed IAM / workspace-admin entry point, `one platform auth` covers token posture and workspace reachability, `one plans` is the managed plans entry point, `one scheduling` covers schedule lifecycle, and `one billing` covers billing posture / usage export. The remaining `one` branches stay reserved until their KBA sets justify more deterministic commands.

Mutating commands (`schedule-create`, `schedule-update`, `schedule-patch`, `schedule-delete`, `collection-create`, `collection-update`, `collection-delete`, any collection membership mutation or permission update, `credential-add`, `credential-update`, `credential-delete`, `credential-share-user`, `credential-share-user-group`, `credential-unshare-*`, `subscription-create`, `subscription-update`, `subscription-delete`, `subscription-change-users`, `user-update`, `user-delete`, `user-transfer-assets`, `user-deactivate`, `user-password-reset`, `workflow-version-upload`, `usergroup-create`, `usergroup-update`, `usergroup-delete`, `usergroup-*` membership moves, and all DCM admin mutators) require `--apply` before they invoke the live API to avoid accidental writes; when that flag is omitted the CLI returns a dry-run envelope with guidance to provide the safety gate.

Ownership transfer is API-first through `PUT /v3/workflows/{workflowId}/transfer`. HTTP retries/backoff handle 429/5xx responses, and the CLI surface maps API failures into structured envelope data for downstream automation.

## Payload files

- `schedule-create` expects a JSON file matching the `CreateScheduleContract` from the Swagger schema (minimum `workflowId` and `iteration` properties; the repo’s `docs/swagger-v3.json` contains the full contract).
- `schedule-update` requires a JSON payload adhering to `UpdateScheduleContract`.
- `collection-update` expects a JSON payload matching `UpdateCollectionContract` (usually `name` and `ownerId`).
- `schedule-patch` expects a JSON payload matching `PatchScheduleContract`.
- `dcm-connection-share-collaboration` expects `DCMEShareForCollaborationContract`.
- `dcm-connection-share-execution` expects `DCMEShareForExecutionContract`.
- `credential-share-user` expects `AddCredentialsUserContract`.
- `credential-share-user-group` expects `AddCredentialsUserGroupContract`.
- `dcm-admin-connection-upsert` expects a JSON payload matching `DCMEUpsertConnectionAdminContract`.
- `credential-add` expects `CredentialAddContract`.
- `credential-update` expects `CredentialUpdateContract`.
- `subscription-create` expects `CreateSubscriptionContract`.
- `subscription-update` expects `UpdateSubscriptionContract`.
- `subscription-change-users` expects `ChangeUsersSubscriptionContract`.
- `user-update` expects `UpdateUserContract`.
- `user-transfer-assets` expects `TransferUserAssetsContract`.
- `workflow-version-upload` uploads a `.yxzp` package via multipart form data; required fields are `name`, `ownerId`, `executionMode`, `workflowCredentialType`, `makePublished`, `othersMayDownload`, `othersCanExecute`, and `bypassWorkflowVersionCheck`, while optional booleans are `hasPrivateDataExemption` and `credentialId`, and `comments` can be supplied.
- `collection-add-user` expects `AddUserContract`.
- `collection-add-schedule` expects `AddScheduleContract`.
- `collection-add-workflow` expects `AddWorkflowContract`.
- `collection-add-user-group` expects `AddUserGroupContract`.
- `collection-update-user-permissions` expects `UpdatePermissionsContract`.
- `collection-update-user-group-permissions` expects `UpdatePermissionsContract`.
- `workflow yxdb --input <file>` reads `.yxdb` files and can export the full result set to CSV with `--csv <path>`.
- `workflow yxdb --input <file> --csv <path> --output json` exports CSV and returns a structured JSON envelope with path, field, row, and CSV metadata.
- `onboard --profile <file> --non-interactive` validates an existing profile and returns a machine-readable summary without prompting.
- `onboard --profile workspace.yaml --workspace` writes a starter workspace file with `dev` and `prod` entries.
- `--environment <name>` selects the active environment when loading a workspace file with multiple named environments.
- `tools workspace init --output workspace.yaml` writes the canonical workspace template, while `tools workspace resolve --workspace workspace.yaml --source dev --target prod` resolves explicit source/target environments for future cross-environment operations.
- `workflow-package` saves the yxzp to the filesystem (default `<workflowId>.yxzp`) and accepts an optional `versionId`.
- Payload files are supplied via `--payload-file <path>` and only evaluated when `--apply` is supplied; otherwise the CLI emits a dry-run envelope pointing to the payload file.

