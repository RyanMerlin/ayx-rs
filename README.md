# AYX-RS

`ayx` is a Rust workspace for Alteryx administrators and automation agents.

It is designed to be:
- fast: a single native binary with no interpreter dependency
- secure: explicit `--apply` gates, audit artifacts, and conservative defaults
- portable: Windows, Linux, and macOS release targets
- agent-friendly: structured envelopes, predictable command output, and a future command/tactics/workflow registry for tools like Codex or Claude

The current focus is Alteryx Server and Gallery administration workflows, with
Licensing and Alteryx One surfaces being added in product-scoped branches:
- Mongo inventory, backup, and restore
- Server API reads and controlled mutations
- upgrade planning and post-checks
- system discovery and log analysis helpers
- Server diagnosis workflows for startup, runtime settings, and network triage
- Licensing portal diagnostics and API surface
- Alteryx One platform as the next major product branch

## Quick start

1. Install the binary with a one-liner:

```bash
curl -fsSL https://raw.githubusercontent.com/RyanMerlin/ayx-cli/main/scripts/install.sh | bash
```

On Windows PowerShell, use:

```powershell
iwr https://raw.githubusercontent.com/RyanMerlin/ayx-cli/main/scripts/install.ps1 | iex
```

2. Create `config.yaml` and set the minimum credentials:

```yaml
profile_name: demo
mongo:
  mode: embedded
server_api:
  base_url: https://your-server.example.com
  client_id: your-client-id
  client_secret: your-client-secret
alteryx_one:
  account_email: you@example.com
```

If you want the CLI to guide you through setup instead of editing YAML by hand, run:

```powershell
ayx onboard --profile config.yaml
```

The onboarding flow reuses existing values on later runs, masks stored secrets in its summary, and auto-discovers embedded Server runtime settings when `RuntimeSettings.xml` is available.
For automation or agents, add `--non-interactive` to validate an existing profile without prompting.

For multi-environment setups, use a `workspace.yaml` file with named environments and select the active one with `--environment <name>`.
`ayx onboard --workspace` writes a starter `workspace.yaml` with `dev` and `prod` entries.

3. Run a first quick query:

```powershell
ayx server api status --profile config.yaml
ayx mongo status --profile config.yaml
ayx catalog list
```

4. Build from source if you want to hack on it locally:

```powershell
cargo install --locked --path .
```

5. Use `--output json` when another tool should consume the result. For `workflow yxdb`, pair `--csv <path>` with top-level `--output json` if you want both export and structured metadata.

If you want the shortest path from zero to useful output, start with:

```powershell
ayx server api status --profile config.yaml --output json
ayx mongo inventory --profile config.yaml --output json
```

## What the CLI gives you

- `mongo` for embedded and managed Mongo operations
  - `mongo query` for read-only collection queries
  - `mongo doctor` for the built-in support query suite
- `server api` for the Server web API
- `server` for environment inspection, logs, Swagger import, and lower-level API calls
- `server diagnose` for operator-facing Server troubleshooting flows
- `server auth` for SAML-first auth inspection, diagnosis, simulation, and narrow legacy AD checks
- `server doctor` for prescriptive troubleshooting workflows built on top of diagnose
- `workflow` for local `.yxmd`, `.yxmc`, `.yxzp`, and `.yxdb` package/XML tooling
  - `workflow scan --rules docs/workflow-recurse.example.yaml` to preflight a migration
  - `workflow recurse --rules docs/workflow-recurse.example.yaml` for recursive migrations
  - `workflow yxdb --input <file> [--csv <path>]` to inspect and export YXDB data
  - `workflow yxdb --input <file> --csv <path> --output json` to export CSV and return a structured JSON envelope
  - `workflow publish` to hand a repackaged workflow back to the Server API
- `--environment <name>` to pick the active environment from a workspace file when multiple named environments are present
- `tools` for workspace-aware source/target workflows and future cross-environment automation
- `license` for the Licensing portal and API branch
- `one` for the Alteryx One platform branch
- `sqlserver` for SQL Server status, prechecks, connection-string helpers, and migration planning
- `onboard` for guided first-run profile setup and subsequent value reuse
- `server upgrade` for upgrade path planning, prechecks, backup, apply simulation, and postchecks
- `catalog` for machine-readable command discovery
- `update` for GitHub release self-update

The tool returns a consistent envelope model so humans and agents can parse success, failure, and artifact paths in the same way.

## Safety model

- Read-only commands are available without extra flags.
- Mutating commands require `--apply`.
- Several workflows also produce audit artifacts so operations can be reviewed or replayed.
- Unsupported command families currently fail explicitly instead of pretending to succeed.

## Configuration

`ayx` loads `config.yaml` by default.
`workspace.yaml` is the canonical multi-environment file. It should contain `workspace_name`, `active_environment`, and an `environments` map of named `Config` entries. Use `--environment <name>` to override the active environment for a single run.

Minimum expectations:
- `profile_name`
- `mongo.mode`
- `mongo.databases.gallery_name`
- `mongo.databases.service_name`
- `server_api.base_url`, `server_api.client_id`, and `server_api.client_secret`
- `alteryx_one.account_email` when using ownership-transfer and related automation
- `alteryx_one.oauth_client_id` and `alteryx_one.token_endpoint_url` for One OAuth token posture
- `alteryx_one.access_token` when using One API commands
- `alteryx_one.refresh_token` when you want to keep the token pair together locally
- `observability.api_logging.enabled` when you want shared JSONL API request logging across Server, License, and One
- `observability.api_logging.path` to control where the shared API event log is written
- `observability.api_logging.redact_bodies` stays on by default so secrets are not written to the log

Sensitive values live in `.env` and are expanded automatically from `config.yaml`.
Use `.env.example` as the shareable template.

Embedded Mongo discovery looks for `RuntimeSettings.xml` in the standard Alteryx locations first, then falls back to the configured path if provided.

## Release and install

The GitHub Actions workflow at [`.github/workflows/build-release.yml`](.github/workflows/build-release.yml) builds Windows, Linux, and macOS binaries and now runs format, clippy, and tests before packaging.

The workflow supports manual `workflow_dispatch` runs and tagged releases (`v*`), so you can publish a release artifact on demand or from a version tag.

Release archives:
- Windows: `ayx-x86_64-pc-windows-msvc.zip`
- Linux: `ayx-x86_64-unknown-linux-gnu.tar.gz`
- macOS Intel: `ayx-x86_64-apple-darwin.tar.gz`
- macOS Apple Silicon: `ayx-aarch64-apple-darwin.tar.gz`

Install scripts:
- `scripts/install.ps1`
- `scripts/install.sh`

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
ayx catalog describe --command mongo/backup
ayx license api status
ayx one platform workspace current
ayx one platform auth status
ayx one platform auth diagnose
ayx one plans list
ayx one scheduling list
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

## Development

Run checks locally:

```powershell
cargo fmt --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

## Fixtures

The repository includes a `RuntimeSettings.xml` fixture for offline validation of embedded discovery paths.

## Upgrade knowledge

The upgrade routing and issue annotations from the archived Omni repo are preserved in:

- `ayx-server/knowledge/upgrade/version_paths.yaml`
- `ayx-server/knowledge/upgrade/known_issues.yaml`

These files drive upgrade path planning and version-specific warnings in the CLI.

## Preserved legacy artifacts

The old `ayxm` repo is being archived, but a few reference files are kept here so the migration is auditable:

- `docs/legacy/AYX_CLI_COMMANDS.yaml`
- `docs/legacy/mongo_schema.py`

These are reference artifacts only. They are not runtime dependencies of the Rust CLI.

