# AYX CLI

`ayx` is a Rust workspace for Alteryx administrators and automation agents.

It is designed to be:
- fast: a single native binary with no interpreter dependency
- secure: explicit `--apply` gates, audit artifacts, and conservative defaults
- portable: Windows, Linux, and macOS release targets
- agent-friendly: structured envelopes, predictable command output, and a future command/tactics/workflow registry for tools like Codex or Claude

The current focus is Alteryx Server and Gallery administration workflows:
- Mongo inventory, backup, and restore
- Server API reads and controlled mutations
- upgrade planning and post-checks
- system discovery and log analysis helpers
- Server diagnosis workflows for startup, runtime settings, and network triage

## Quick start

1. Install Rust, then build the CLI from source:

```powershell
cargo install --locked --path .
```

2. Create or copy `config.yaml` into the working directory.

3. Validate the environment:

```powershell
ayx mongo status --profile config.yaml
ayx server api status --profile config.yaml
ayx server
```

4. Use `--output json` when another tool should consume the result.

## What the CLI gives you

- `mongo` for embedded and managed Mongo operations
  - `mongo query` for read-only collection queries
  - `mongo doctor` for the built-in support query suite
- `server api` for the Server web API
- `server` for environment inspection, logs, Swagger import, and lower-level API calls
- `server diagnose` for operator-facing Server troubleshooting flows
- `server auth` for SAML-first auth inspection, diagnosis, simulation, and narrow legacy AD checks
- `server doctor` for prescriptive troubleshooting workflows built on top of diagnose
- `upgrade` for upgrade path planning, prechecks, backup, apply simulation, and postchecks
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

Minimum expectations:
- `profile_name`
- `mongo.mode`
- `mongo.databases.gallery_name`
- `mongo.databases.service_name`
- `server_api.base_url`, `server_api.client_id`, and `server_api.client_secret`
- `alteryx_one.account_email` when using ownership-transfer and related automation

Sensitive values live in `.env` and are expanded automatically from `config.yaml`.
Use `.env.example` as the shareable template.

Embedded Mongo discovery looks for `RuntimeSettings.xml` in the standard Alteryx locations first, then falls back to the configured path if provided.

## Release and install

The GitHub Actions workflow at [`.github/workflows/build-release.yml`](.github/workflows/build-release.yml) builds Windows, Linux, and macOS binaries and now runs format, clippy, and tests before packaging.

Release archives:
- Windows: `ayx-x86_64-pc-windows-msvc.zip`
- Linux: `ayx-x86_64-unknown-linux-gnu.tar.gz`
- macOS Intel: `ayx-x86_64-apple-darwin.tar.gz`
- macOS Apple Silicon: `ayx-aarch64-apple-darwin.tar.gz`

Install scripts:
- `scripts/install.ps1`
- `scripts/install.sh`

## Vision

The long-term goal is not just a CLI. It is a secure, portable operator for the Alteryx ecosystem that can also serve as a tool and skill substrate for agents.

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

