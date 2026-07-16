# AYX-RS

`ayx` is a command-line tool suite for Alteryx administrators, automation, and agentic workflows.

It is designed to operate across the Alteryx surface and enable sophisticated operations.
- administrator-friendly: clear command surfaces for common Alteryx operations
- automation-friendly: a single native binary with predictable output and no interpreter dependency
- secure: explicit `--apply` gates, audit artifacts, and conservative defaults
- portable: Windows, Linux, and macOS
- agent-friendly: structured envelopes and a growing command/actions/workflow registry

> Status: `ayx` ships a stable CLI, a machine-readable command catalog, a first-class live `discover` entry point, and a live One surface inventory. The registry layer is still growing, but `catalog` remains the machine-readable view rather than a replacement for the live tree.

**Created by Ryan Merlin.**

`ayx` is an independent, open-source project.  It is not affiliated with, authorized, maintained, sponsored, or endorsed by Alteryx, Inc.  "Alteryx", "Alteryx One", and "Alteryx Server" are trademarks of Alteryx, Inc.; those names are used here only to describe the systems this tool operates against.

## Quick start

`RyanMerlin/ayx-rs` is the canonical public home for source, releases, and self-update.
If you mirror this repository elsewhere, treat those copies as non-canonical.

1. Install the binary with a one-liner.
   By default the installers verify `SHA256SUMS`; set `AYX_VERIFY_SIGSTORE=1`
   to additionally verify the published sigstore bundle when `cosign` is
   available:

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

```bash
ayx onboard
```

`ayx onboard` is the fastest path to a working setup: it reuses existing values on later runs, masks stored secrets in its summary, and auto-discovers embedded Server settings when `RuntimeSettings.xml` is available.  For automation or agents, add `--non-interactive` to validate an existing profile without prompting.  If you'd rather hand-edit YAML or wire up multiple environments, see [Configuration](#configuration) below.

3. Run a first quick query:

```bash
ayx profile current
ayx one platform workspace current
ayx one flows list
ayx server api status
```

4. Build from source if you want to hack on it locally:

```bash
cargo install --locked --path ayx-rs
```

5. Use `--output json` when another tool should consume the result. For `designer workflow yxdb`, pair `--csv <path>` with top-level `--output json` if you want both export and structured metadata.

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
For promotion-style workflows with multiple Server instances, keep one environment per instance and use `tools workspace resolve` to make source/target selection explicit. `tools workspace compare` and the cross-environment migration helpers (`migrate-workflows`, `check-dcm-connections`) resolve and summarize both environments today but do not yet compare or migrate anything (preview / not yet implemented).

Minimum expectations:
- `profile_name`
- `alteryx_one.base_url` for the One API host
- `alteryx_one.account_email` when using ownership-transfer and related automation
- `alteryx_one.oauth_client_id` and `alteryx_one.token_endpoint_url` for One OAuth token posture
- `alteryx_one.access_token` when using One API commands
- `alteryx_one.refresh_token` when you want to keep the token pair together locally
- `alteryx_one.client_secret` or `alteryx_one.client_secret_ref` when you use service-principal
  client credentials; pair them with `alteryx_one.oauth_client_id` and let `token_endpoint_url`
  derive from the base URL when possible.
- `AYX_ONE_CLIENT_ID`, `AYX_ONE_CLIENT_SECRET`, `AYX_ONE_TOKEN_ENDPOINT_URL`, `AYX_ONE_API_ACCESS_TOKEN`,
  and `AYX_ONE_API_REFRESH_TOKEN` are the generic env overrides used by the loader.
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
Central profile files, workspace files, runtime state, audit artifacts, and
observability logs are all treated as sensitive local artifacts and are written
with owner-only permissions on supported platforms.

Embedded Mongo discovery looks for `RuntimeSettings.xml` in the standard Alteryx locations first, then falls back to the configured path if provided.

## Release and install

Release artifacts are built for Linux, macOS, and Windows from GitHub Actions.
The public release channel is this repository's GitHub Releases page.

Release archives:
- Linux: `ayx-x86_64-unknown-linux-gnu.tar.gz`
- macOS Intel: `ayx-x86_64-apple-darwin.tar.gz`
- macOS Apple Silicon: `ayx-aarch64-apple-darwin.tar.gz`
- Windows: `ayx-x86_64-pc-windows-msvc.zip`

Install scripts:
- `scripts/install.ps1`
- `scripts/install.sh`

Verification:
- `SHA256SUMS` is verified by default.
- Sigstore bundle verification is available by setting `AYX_VERIFY_SIGSTORE=1`
  before running the installer, provided `cosign` is available on PATH.
- Release artifacts also publish `.sigstore` bundles and GitHub provenance
  attestations for operators who want stronger supply-chain verification.

Repo governance and pre-launch checks live in `docs/public-release-checklist.md`.

The installers prefer dedicated bin directories such as `~/.local/bin` so
they do not get shadowed by tool-managed PATH entries like `mise` installs.

`ayx update` only updates the release binary that is currently on PATH. If you
are running a source build (`cargo run`) or a tool-managed shim, update that
copy first or switch PATH to the release install before invoking `ayx update`.

## Vision

The long-term goal is not just a CLI. It is a secure, portable operator for the Alteryx ecosystem that can also serve as a tool and skill substrate for agents and non-technical operators.

That means:
- a stable command catalog
- a progressive discovery ladder from commands to capabilities to actions to workflows
- an action registry for repeatable playbooks
- workflow/skill descriptions for multi-step operations
- structured evidence after every run
- documentation that stays aligned with the actual binary

Start with:

```powershell
ayx discover
ayx catalog list
ayx catalog describe mongo/backup
ayx catalog describe designer.workflow.context
ayx one doctor discover
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
- `ayx discover [path] [--deep]` is the progressive live entry point for the actual `clap` command tree.
- `ayx catalog list --tag designer --format full` surfaces capability ids, schemas, safety, and provider type alongside the existing command catalog.
- `ayx catalog describe <command-or-capability>` resolves either a legacy command path/name or a capability id such as `designer.tool.add`.
- `catalog` remains the registry view for commands and capabilities; it is not the primary discovery entry point anymore.
- Capability ids, validation metadata, and executor wiring already exist inside the registry layer so we can progressively expose deeper discovery without changing the ids.

## Development

Run checks locally:

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo nextest run --workspace --locked
```

## Documentation

The published docs surface lives in `site/` and is built with Astro/Starlight.
It is designed to ship with versioned docs so the current command surface,
release notes, and configuration references stay aligned with tagged releases.

Build it locally with:

```powershell
cd site
npm ci
npm run build  # or: npm run dev  (for live reload)
```

On push to `main` the site is automatically deployed to Cloudflare Pages via `.github/workflows/docs-deploy.yml`.

For the One surface specifically, the live validation plan is documented in `docs/one-live-validation.md`.

## Fixtures

The repository includes a `docs/fixtures/RuntimeSettings.xml` fixture for offline validation of embedded discovery paths.

## Top-level Commands

- `profile` — central profile registry and active profile management
- `one` — Alteryx One platform branch and API surface
- `tools` — cross-environment tools for `environments.yaml` source/target workflows (`compare` and the migration helpers are preview / not yet implemented)
- `secret` — keyring secret inspection and maintenance
- `designer` — Alteryx Designer / Server artifact tooling; `designer workflow` handles `.yxmd`, `.yxmc`, `.yxzp`, and `.yxdb`
- `server` — Server discovery, logs, auth, diagnose, doctor, upgrade, and low-level API calls
- `mongo` — embedded and managed Mongo inventory, backup, restore, query, and doctor helpers (`mutate` is preview / not yet implemented)
- `sqlserver` — SQL Server status, prechecks, connection helpers, and migration planning
- `onboard` — interactive first-run setup for `config.yaml` or `environments.yaml`
- `tui` — interactive TUI for profile selection, editing, credentials, and connectivity checks
- `catalog` — machine-readable command registry
- `audit` — audit artifact management, retention, and cleanup
- `actions` — action registry with safety, validation, and rollback notes
- `workflows` — workflow registry for higher-order skill chains
- `license` — licensing portal branch and API surface
- `whoami` — show the active profile, account email, workspace, and environment
- `doctor` — configuration, auth, network, and product health diagnostics
- `update` — self-update from GitHub releases
- `completions` — generate shell completion scripts
- `telemetry` — operational telemetry for jobs, workflows, plans, and errors
- `discover` — progressive live discovery of the CLI tree

For the full, always-current command tree, run `ayx discover --deep` or see the docs command reference / `docs/command-surface.md`.
