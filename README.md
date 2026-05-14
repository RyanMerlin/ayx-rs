# AYX-RS

`ayx` is a command-line tool suite for Alteryx administrators, automation, and agentic workflows.

It is designed to operate across the Alteryx surface and enable sophisticated operations.
- administrator-friendly: clear command surfaces for common Alteryx operations
- automation-friendly: a single native binary with predictable output and no interpreter dependency
- secure: explicit `--apply` gates, audit artifacts, and conservative defaults
- portable: Windows, Linux, and macOS release targets
- agent-friendly: structured envelopes and a future command/tactics/workflow registry

## Quick start

1. Install the binary with a one-liner:

```bash
curl -fsSL https://raw.githubusercontent.com/RyanMerlin/ayx-rs/main/scripts/install.sh | bash
```

On Windows PowerShell, use:

```powershell
iwr https://raw.githubusercontent.com/RyanMerlin/ayx-rs/main/scripts/install.ps1 | iex
```

2. Create a central profile and set the minimum credentials.

By default, `ayx` now resolves profiles from its central config home:
- Linux/macOS: `~/.config/ayx`
- Windows: `%AppData%\\ayx`

The fastest path is onboarding:

```powershell
ayx onboard
```

That writes the active profile to the central profile store. If you prefer to edit YAML directly, create a profile file under `profiles/default.yaml` in the config home with the minimum fields:

```yaml
profile_name: demo
alteryx_one:
  account_email: you@example.com
server:
  api:
    base_url: https://your-server.example.com
    client_id: your-client-id
    client_secret: your-client-secret
  storage:
    kind: embedded-mongo
    mongo:
      mode: embedded
```

The onboarding flow reuses existing values on later runs, masks stored secrets in its summary, and auto-discovers embedded Server runtime settings when `RuntimeSettings.xml` is available.
For automation or agents, add `--non-interactive` to validate an existing profile without prompting.

For multi-environment setups, use a central `environments.yaml` file with named environments and select the active one with `--environment <name>`.
`ayx onboard --environments` writes a starter environments file with `dev` and `prod` entries.

3. Run a first quick query:

```powershell
ayx profile current
ayx one platform workspace current
ayx one flows list
ayx server api status
```

4. Build from source if you want to hack on it locally:

```powershell
cargo install --locked --path .
```

5. Use `--output json` when another tool should consume the result. For `workflow yxdb`, pair `--csv <path>` with top-level `--output json` if you want both export and structured metadata.

If you want the shortest path from zero to useful output, start with:

```powershell
ayx one platform workspace current --output json
ayx one connections list --output json
ayx server api status --output json
```

## Quick Examples

The shortest path from zero to useful output is usually one of:

- `ayx profile current`
- `ayx doctor`
- `ayx one platform workspace current`
- `ayx one flows list`
- `ayx one connections list`
- `ayx server api status --output json`
- `ayx mongo inventory --output json`
- `ayx one job-groups list`
- `ayx one output-objects list`
- `ayx one platform person count`

The tool returns a consistent envelope model so humans and agents can parse success, failure, and artifact paths in the same way.

## Safety model

- Read-only commands are available without extra flags.
- Mutating commands require `--apply`.
- Several workflows also produce audit artifacts so operations can be reviewed or replayed.
- Unsupported command families currently fail explicitly instead of pretending to succeed.

## Configuration

`ayx` resolves profiles from its central config home by default and keeps an active-profile pointer in local state.
Use `ayx profile current` to see the active profile, `ayx profile list` to inspect stored profiles, and `ayx profile use <name>` to switch the default profile.
`--profile <name>` selects a central profile by name. Use `ayx profile migrate --profile <path>` to import a legacy YAML file into the central store; the TUI and onboarding flows are the only places that intentionally operate on explicit file paths.

`environments.yaml` is the canonical multi-environment file shape. It should contain `workspace_name`, `active_environment`, and an `environments` map of named `Config` entries. Use `--environment <name>` to override the active environment for a single run.
For promotion-style workflows with multiple Server instances, keep one environment per instance and use `tools workspace resolve`, `compare`, or the migration helpers to make source/target selection explicit.

Minimum expectations:
- `profile_name`
- `alteryx_one.account_email` when using ownership-transfer and related automation
- `alteryx_one.oauth_client_id` and `alteryx_one.token_endpoint_url` for One OAuth token posture
- `alteryx_one.access_token` when using One API commands
- `alteryx_one.refresh_token` when you want to keep the token pair together locally
- `server.api.base_url`, `server.api.client_id`, and `server.api.client_secret`
- `server.storage.kind`
- `server.storage.mongo.mode`
- `server.storage.mongo.databases.gallery_name`
- `server.storage.mongo.databases.service_name`
- `server.storage.mongo.embedded.runtime_settings_path` when you need to pin the embedded Server runtime path
- `server.storage.mongo.managed.*` when you target a managed MongoDB
- `server.storage.sqlserver.controller.*` and `server.storage.sqlserver.server_ui.*` when you use SQL-backed storage
- `observability.api_logging.enabled` when you want shared JSONL API request logging across Server, License, and One
- `observability.api_logging.path` to control where the shared API event log is written
- `observability.api_logging.redact_bodies` stays on by default so secrets are not written to the log

Sensitive values should live in the OS keyring by default, with environment variables remaining first-class for automation.
`ayx doctor config` flags inline secret fields so they can be migrated into keyring-backed refs.
Use `.env.example` only as a non-secret template for local overrides and automation.

Embedded Mongo discovery looks for `RuntimeSettings.xml` in the standard Alteryx locations first, then falls back to the configured path if provided.

## Release and install

Releases are built for Windows, Linux, and macOS from GitHub Actions.

Release archives:
- Windows: `ayx-x86_64-pc-windows-msvc.zip`
- Linux: `ayx-x86_64-unknown-linux-gnu.tar.gz`
- macOS Intel: `ayx-x86_64-apple-darwin.tar.gz`
- macOS Apple Silicon: `ayx-aarch64-apple-darwin.tar.gz`

Install scripts:
- `scripts/install.ps1`
- `scripts/install.sh`

The installers prefer dedicated bin directories such as `~/.local/bin` so
they do not get shadowed by tool-managed PATH entries like `mise` installs.

`ayx update` only updates the release binary that is currently on PATH. If you
are running a source build (`cargo run`) or a tool-managed shim, update that
copy first or switch PATH to the release install before invoking `ayx update`.

## Vision

The long-term goal is not just a CLI. It is a secure, portable operator for the Alteryx ecosystem that can also serve as a tool and skill substrate for agents and non-technical operators.

That means:
- a stable command catalog
- a tactical registry for repeatable playbooks
- workflow/skill descriptions for multi-step operations
- structured evidence after every run
- documentation that stays aligned with the actual binary

Start with:

```powershell
ayx catalog list
ayx catalog describe mongo/backup
ayx catalog describe designer.workflow.context
ayx catalog run designer.workflow.context --json '{"workflow_path":"sample.yxmd"}'
ayx one platform workspace current
ayx one platform person count
ayx one flows list
ayx one connections list
ayx one job-groups list
ayx one output-objects list
ayx one platform auth status
ayx one platform auth diagnose
ayx one plans list
ayx one scheduling list
ayx license api status
ayx server diagnose startup --error "Failed to register Service URL"
ayx server auth status
ayx server auth diagnose saml --metadata-url https://idp.example.com/metadata
ayx server auth diagnose certificate
ayx server auth diagnose ad-legacy
ayx server auth simulate saml --metadata-file .\metadata.xml
ayx server doctor startup --error "Failed to register Service URL"
ayx mongo query --database AlteryxService --collection AS_Queue --filter "{}"
ayx mongo doctor
```

Agent-oriented catalog notes:
- `ayx catalog list --tag designer --format full` surfaces capability ids, schemas, safety, and provider type alongside the existing command catalog.
- `ayx catalog describe <command-or-capability>` resolves either a legacy command path/name or a capability id such as `designer.tool.add`.
- `ayx catalog run <capability> --json <payload-or-@file> [--dry-run]` is the structured execution entry point for the native capability layer.
- The catalog layer is designed so a live IPC backend can slot in later without changing the public ids.

## Development

Run checks locally:

```powershell
cargo fmt --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

## Fixtures

The repository includes a `RuntimeSettings.xml` fixture for offline validation of embedded discovery paths.

## Full Command Tree

```text
ayx
|-- catalog                       command and capability discovery
|   |-- list
|   |-- describe
|   `-- run
|-- doctor                        configuration, auth, network, and product diagnostics
|   |-- config
|   |-- auth
|   |-- network
|   |-- one
|   |-- server
|   `-- mongo
|-- license                       Licensing portal checks and API access
|   |-- status
|   |-- inventory
|   `-- api
|       |-- status
|       `-- diagnose
|-- mongo                         embedded and managed Mongo operations
|   |-- status
|   |-- inventory
|   |-- backup
|   |-- restore
|   |-- query
|   |-- mutate
|   `-- doctor
|-- onboard                       guided first-run profile setup
|-- profile                       central profile registry and active profile management
|   |-- list
|   |-- current
|   |-- show
|   |-- use
|   |-- path
|   `-- migrate
|-- one                           Alteryx One control plane and workflow surfaces
|   |-- status
|   |-- inventory
|   |-- doctor
|   |   |-- auth
|   |   |-- discover
|   |   |-- platform
|   |   |-- plans
|   |   |-- scheduling
|   |   `-- billing
|   |-- platform                  workspace, people, roles, tokens, and API utilities
|   |   |-- status
|   |   |-- inventory
|   |   |-- api
|   |   |   |-- status
|   |   |   |-- diagnose
|   |   |   `-- open-api-spec
|   |   |-- auth
|   |   |   |-- status
|   |   |   `-- diagnose
|   |   |-- workspace
|   |   |   |-- list
|   |   |   |-- current
|   |   |   |-- current-configuration
|   |   |   |-- configuration-v4
|   |   |   |-- save-current-configuration
|   |   |   |-- save-configuration-v4
|   |   |   |-- configuration
|   |   |   |-- configuration-schema
|   |   |   |-- current-configuration-schema
|   |   |   |-- delete-current-configuration
|   |   |   |-- delete-configuration
|   |   |   |-- people
|   |   |   |-- admins
|   |   |   |-- invite-users
|   |   |   |-- remove-user
|   |   |   |-- suspend-users
|   |   |   |-- unsuspend-users
|   |   |   |-- transfer
|   |   |   `-- transfer-assets
|   |   |-- role
|   |   |   |-- list-assignments
|   |   |   |-- assign
|   |   |   `-- unassign
|   |   |-- user
|   |   |-- token
|   |   |   |-- list
|   |   |   |-- create
|   |   |   |-- detail
|   |   |   `-- delete
|   |   `-- person
|   |       |-- list
|   |       |-- current
|   |       |-- count
|   |       |-- detail
|   |       |-- create
|   |       |-- update
|   |       |-- patch
|   |       |-- delete
|   |       |-- update-password
|   |       `-- password-reset-request
|   |-- plans
|   |   |-- list
|   |   |-- create
|   |   |-- detail
|   |   |-- full
|   |   |-- run
|   |   |-- count
|   |   |-- run-parameters
|   |   |-- schedules
|   |   |-- export
|   |   |-- update
|   |   |-- delete
|   |   |-- share
|   |   |-- import
|   |   `-- permissions
|   |       `-- remove
|   |-- flows
|   |   |-- list
|   |   |-- count
|   |   |-- create
|   |   |-- detail
|   |   |-- update
|   |   |-- delete
|   |   |-- copy
|   |   |-- run
|   |   |-- validate
|   |   |-- parameters
|   |   |-- inputs
|   |   |-- outputs
|   |   |-- import
|   |   |-- import-dry-run
|   |   |-- export
|   |   `-- export-dry-run
|   |-- connections
|   |   |-- list
|   |   |-- count
|   |   |-- create
|   |   |-- dry-run
|   |   |-- detail
|   |   |-- status
|   |   |-- update
|   |   |-- delete
|   |   |-- permissions
|   |   |   |-- list
|   |   |   |-- create
|   |   |   |-- detail
|   |   |   `-- delete
|   |   `-- connector-metadata
|   |       |-- defaults
|   |       |-- detail
|   |       |-- publish-info
|   |       `-- overrides
|   |           |-- list
|   |           |-- create
|   |           `-- delete
|   |-- job-groups                    run artifacts, publish, pdf results, and execution support
|   |   |-- list
|   |   |-- count
|   |   |-- run
|   |   |-- publish
|   |   |-- detail
|   |   |-- cancel
|   |   |-- status
|   |   |-- inputs
|   |   |-- outputs
|   |   |-- jobs
|   |   |-- publications
|   |   |-- profile
|   |   |-- profile-results
|   |   `-- pdf-results
|   |-- output-objects               flow output and wrangling surfaces
|   |   |-- list
|   |   |-- count
|   |   |-- create
|   |   |-- detail
|   |   |-- update
|   |   |-- delete
|   |   |-- inputs
|   |   `-- wrangle-to-python
|   |-- webhook-flow-tasks           webhook task lifecycle
|   |   |-- create
|   |   |-- detail
|   |   |-- delete
|   |   `-- test
|   |-- write-settings               runtime write-setting helpers
|   |   |-- list
|   |   |-- count
|   |   |-- create
|   |   |-- detail
|   |   |-- update
|   |   `-- delete
|   |-- scheduling
|   |   |-- list
|   |   |-- detail
|   |   |-- enable
|   |   |-- disable
|   |   `-- count
|   |-- billing
|   |   |-- current-account
|   |   `-- usage-export
|   |-- ui                           experimental visual interface surface
|   |   |-- session
|   |   |   |-- status
|   |   |   |-- ensure
|   |   |   |-- attach
|   |   |   `-- inventory
|   |   |-- workflow
|   |   |   |-- open
|   |   |   |-- create
|   |   |   |-- inventory
|   |   |   |-- pane-config
|   |   |   |-- pane-results
|   |   |   |-- tool-list
|   |   |   |-- tool-select
|   |   |   |-- tool-inspect
|   |   |   |-- graph-get
|   |   |   `-- graph-put
|   |   |-- data
|   |   |   |-- list-datasets
|   |   |   |-- dataset-detail
|   |   |   |-- dataset-preview
|   |   |   |-- upload
|   |   |   `-- list-connections
|   |   |-- library
|   |   |   `-- inventory
|   |   |-- schedules
|   |   |   `-- inventory
|   |   `-- jobs
|   |       `-- inventory
|   |-- auto-insights
|   `-- desktop-exec
|-- server                        Server API, logs, import, and lower-level helpers
|   |-- api
|   |   |-- status
|   |   |-- diagnose
|   |   |-- import-swagger
|   |   `-- call
|   |-- system-info
|   |-- runtime-settings
|   |-- ayx-paths
|   |-- server-logs
|   |   |-- discover
|   |   |-- inventory
|   |   |-- summary
|   |   |-- context
|   |   |-- parse-csv
|   |   |-- service-events
|   |   |-- gallery-events
|   |   |-- tail
|   |   `-- recent
|   |-- diagnose
|   |   |-- startup
|   |   |-- logs
|   |   |-- network
|   |   |-- tls
|   |   `-- runtime-settings
|   |-- auth
|   |   |-- status
|   |   |-- diagnose
|   |   |   |-- saml
|   |   |   |-- saml-logs
|   |   |   |-- certificate
|   |   |   `-- ad-legacy
|   |   `-- simulate
|   |       `-- saml
|   |-- doctor
|   |   |-- startup
|   |   |-- logs
|   |   |-- network
|   |   `-- runtime-settings
|   |-- upgrade
|   |   |-- path
|   |   |-- precheck
|   |   |-- backup
|   |   |-- plan
|   |   |-- apply
|   |   |-- postcheck
|   |   `-- bundle
|   |-- backup-plan
|   `-- backup
|-- sqlserver                     SQL Server prechecks and migration planning
|   |-- status
|   |-- inventory
|   |-- precheck
|   |-- validate-strings
|   |-- connection-string
|   |-- migrate
|   `-- prepare
|-- tools                         workspace-aware source/target workflows
|   `-- workspace
|       |-- init
|       |-- resolve
|       |-- compare
|       |-- migrate-workflows
|       `-- check-dcm-connections
|-- update                        self-update from GitHub releases
`-- workflow                      local XML/package tooling for Desktop artifacts
    |-- inspect
    |-- unpack
    |-- validate
    |-- replace
    |-- repackage
    |-- migrate
    |-- recurse
    |-- scan
    |-- convert-cloud
    |-- publish
    `-- yxdb
```
