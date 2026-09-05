# AYX-RS CLI Spec

This document is the stable contract for how `ayx` behaves. It is intentionally
shorter than a full command inventory so it does not drift every time a command
surface grows.

For the live command tree, use:

- `ayx --help`
- `ayx <group> --help`
- `ayx discover --deep`
- `ayx catalog list`
- `ayx catalog describe <command-or-capability>`
- `docs/command-surface.md` after running `cargo run -q -p xtask -- refresh-command-surface`

## Product Identity

- Binary name: `ayx`
- Primary source of truth: `RyanMerlin/ayx-rs` on GitHub
- Supported operator surfaces: local CLI, structured catalog
- Supported release targets: Linux, macOS, Windows

## Runtime Model

`ayx` is central-profile-first.

- Runtime commands resolve the active profile from the ayx config home.
- `--profile <name>` selects a central profile by name for one run.
- `AYX_PROFILE=<name>` is the environment-variable equivalent.
- Filesystem paths are not valid runtime profile selectors.
- Explicit file paths are reserved for onboarding, migration, and editor-style
  flows such as `ayx profile migrate --profile <path>`.

Multi-environment workflows use `environments.yaml`.

- `--environment <name>` overrides the active environment for one run.
- Workspace-style source/target resolution belongs in the `tools workspace`
  family, not in ad hoc path-based command flags.

## First Run

The shortest supported setup path is:

1. Install from the public GitHub release channel.
2. Run `ayx onboard`.
3. Confirm the active profile with `ayx profile current`.
4. Run a read-only command such as `ayx one workspace current` or
   `ayx server api status`.

Legacy YAML import remains supported through:

- `ayx profile migrate --profile <path>`

## Output Contract

`ayx` supports:

- `--output text`
- `--output json`
- `--output json-full`
- `--output yaml`
- `--output table`

`json` is the versioned compact presentation contract (`schema_version:
"ayx.output.v1"`) and is capped at 20 projected list rows unless overridden
with `--output-limit`; use `0` for no compact-list cap. `json-full` and YAML
serialize the full, recursively redacted envelope. Raw-field scripts must use
`json-full`.

Two more global flags post-process the rendered result: `--jq <FILTER>` runs a
jq filter (pure-Rust `jaq`) over the JSON output and prints one value per
line, forcing `--output json` unless `--output json-full` is given; `--raw-output`
/ `-r` (requires `--jq`) prints string results unquoted. A filter that fails to
parse, compile, or run exits 2 (`validation`), matching every other
`validation`-class failure.

The full envelope contract is:

- `ok`
- `message`
- `timestamp_utc`
- `data`
- `error_code` on failures (`snake_case`: `config_missing`, `auth_failed`, `permission_denied`, `not_found`, `validation`, `conflict`, `rate_limited`, `network`, `upstream`, `workspace_mismatch`, `internal`)

Successful envelopes are written to stdout. Error envelopes are written to
stderr. Examples should place `--output` after the complete command path;
leading placement remains supported for compatibility.

Commands may also emit artifact paths, warnings, or audit metadata inside the
envelope payload.

## Safety Model

The CLI is conservative by default.

- Read-only commands run without an extra safety flag.
- Mutating commands require `--apply`.
- When `--apply` is omitted on a mutating command, the CLI should return a
  dry-run style response instead of silently performing the write.
- Audit artifacts are expected for destructive or operationally significant
  workflows.

## Configuration Contract

The runtime config shape is `ayx-core::profile::Config`.

Minimum practical expectations depend on the product surface in use, but the
common baseline is:

- `profile_name`
- `server.api.base_url`, `server.api.client_id`, `server.api.client_secret`
  for Server API usage
- `server.storage.kind`
- `server.storage.mongo.mode`
- `server.storage.mongo.databases.gallery_name`
- `server.storage.mongo.databases.service_name`
- `alteryx_one.account_email` for One ownership and identity workflows
- One OAuth/token fields when using One API families

Sensitive values should prefer keyring-backed refs or environment variables over
inline plaintext config.

## Command Families

The CLI is product-first. The stable top-level commands and families are:

- `catalog`
- `doctor`
- `license`
- `mongo`
- `onboard`
- `profile`
- `one`
- `server`
- `secret`
- `audit`
- `actions`
- `actions workflows`
- `telemetry`
- `whoami`
- `designer workflow`
- `tools`
- `sqlserver`
- `update`
- `completions`
- `discover`

The exact leaf inventory can expand, but the design rules are stable:

- product surfaces stay grouped under product roots
- read-only and mutating actions are visibly distinct
- catalog-capable features should be discoverable through `ayx catalog`
- command help should be the authoritative source for exact flags and leaf names

## Catalog Contract

`ayx discover` is the structured, progressive discovery surface for humans and agents — the source of truth for flags, positional arguments, aliases, and nested tree traversal.

- `ayx discover [path] [--deep]` walks the live CLI tree.
- `ayx catalog list --scope all` (the default) enumerates every visible command plus every registered capability as a complete, flattened, machine-readable index. Command identity and summary are sourced live from clap; commands without a curated metadata entry show up honestly as `unclassified` rather than a fabricated classification.
- `ayx catalog list --scope curated` is the compatibility view: the same command set narrowed to entries that carry a full metadata classification (output/safety/mutating/prerequisites/notes), for clients that need only the previously curated projection.
- `ayx catalog describe <id>` continues to accept either a legacy command name/path or a capability id.
- `catalog` remains the derived registry/compatibility view for commands and capabilities, not a hand-maintained one.
- If `catalog` (or its `curated` scope) is ever deprecated, it should get a clear compatibility window or alias path rather than vanishing before discovery exposes an equivalent stable registry surface.
- The discovery ladder grows from commands to capabilities to actions and
  workflows without changing the public ids.

Capability ids should remain more stable than help text or internal module
layouts.

## Mongo and Server Rules

- Embedded Mongo discovery should prefer standard `RuntimeSettings.xml`
  locations, then explicit configured overrides.
- Managed Mongo workflows should use the configured managed connection settings
  and native Mongo tooling.
- Server API workflows use the active central profile and should preserve
  structured HTTP/result reporting for automation.
- Backups, restores, and ownership-transfer workflows should keep writing audit
  evidence.

## Alteryx One Rules

- One command families should validate workspace context before mutating.
- Workspace identity must be treated as runtime state, not inferred from stale
  browser context or copied URLs.
- Structured API and auth diagnostics should remain available even when a
  product surface is only partially implemented.

## Update and Release Contract

- `ayx update` targets the GitHub release channel by default.
- Release artifacts publish platform-specific archives plus checksums.
- Install/update instructions should prefer the published release binary over a
  source-build shim when self-update is expected to work.

## Validation Contract

Repo-level validation guidance should stay aligned with CI:

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo nextest run --workspace --locked
```

## Non-goals for This Doc

This spec intentionally does not duplicate:

- every leaf command
- every payload schema
- every API endpoint path
- every implementation detail of module layout

Those details belong in command help, the catalog surface, targeted handoff
docs, or generated references.
