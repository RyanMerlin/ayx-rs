# Wave 0: TUI removal and agent hygiene

**Date:** 2026-09-04
**Status:** Approved in principle (ADR 0004); three implementation decisions
flagged below for the owner before the plan is written
**Release:** `0.20.0` (breaking: removes the `tui` command)
**Roadmap:** `docs/roadmap/agent-first-substrate.md`, Wave 0

## Goal

Make `0.20.0` the release that states the product thesis by subtraction and by
hygiene: `ayx` has no bundled TUI, and it behaves like a first-class agent
substrate out of the box. Everything here is small, mechanical, and
independently shippable; the wave is one release because the breaking change
and the agent-facing defaults belong in the same changelog entry.

Baseline for every path and line number below is `origin/main` at `ab1ef82`
(`v0.19.1`). Line numbers are approximate and must be re-resolved by the
implementer.

## Scope

In scope:

- **A.** Remove `ayx tui` (legacy and v2), its dependencies, tests, and doc
  references. Land `ayx one workspace detail <id>` first.
- **B1.** Output-mode auto-detection for non-interactive callers.
- **B2.** Additive envelope fields: `remediation`, `retryable`, `next`. Fix
  `docs/cli-schema.json`.
- **B3.** `--jq <filter>` (and `--raw-output`) as global flags.
- **B4.** `ayx one open <kind> <id>` web deep-links.
- **B5.** TTY-gated pickers for a curated set of commands.
- **B6.** Stretch: `--watch` on `one job-groups status`.

Out of scope: any governance command (Wave 1), `ayx agent init` / `ayx mcp
serve` (Wave 2), plan artifacts and policy (Wave 3), any change to the Headless
client, any `ayx server` change beyond what shared code forces.

## Part A: TUI removal

### A1. `ayx one workspace detail <id>` (lands first)

`GET /v4/workspaces/{id}` is dispatched only from the legacy TUI
(`ayx-rs/src/tui/one_browser.rs:62-70`, registered as `tui-workspace-detail`),
so the drift gate carves it out in `NON_ONE_SURFACE_ENDPOINTS`
(`ayx-rs/tests/one_inventory_drift.rs:115-124`). Add a real command:

- Clap: `OneWorkspaceCommand::Detail { id: String }` beside `Current` and
  `List`; `about = "Inspect a One workspace by id"`.
- Dispatch through `one_api_live_request(config, "platform",
  "workspace-detail", "GET", "/v4/workspaces/{id}", false, &[("id", &id)])`,
  matching `one_browser.rs` exactly.
- Move the endpoint from the drift-gate carve-out into
  `ayx-one-api/src/inventory.rs` as a normal row; add the endpoint-matrix row
  in `docs/one-endpoint-matrix.md` (`unverified` until probed live).
- `CATALOG_METADATA`: `read-only`, `mutating: no`.

This also closes the roadmap item in `api-surface-and-observability.md:47-51`.

### A2. Delete the TUI

Delete `ayx-rs/src/tui/` (30 files, 9,868 lines). Then remove every reference:

| Site | Action |
| --- | --- |
| `ayx-rs/src/main.rs:43` `mod tui;` | delete |
| `ayx-rs/src/main.rs:~447-450` `Command::Tui` variant | replace with the hidden deprecation stub (A3) |
| `ayx-rs/src/main.rs:~365` `OutputDescriptor::new("tui", ViewKind::Raw)` | keep for the stub, `ViewKind::Result` |
| `ayx-rs/src/main.rs:~4300` `Command::Tui => return tui::run()` | dispatch to the stub |
| `ayx-rs/src/cmd/one_platform/auth.rs:829` `crossterm::terminal::disable_raw_mode()` | Determine why the OTP login flow resets raw mode (it is a defensive reset after an interrupted TUI session). With no TUI it is dead; remove it. If a terminal-state reset is still wanted, use the picker crate's terminal helper rather than a direct `crossterm` import. |
| `Cargo.toml:40` `ratatui`, `:60` `tui-input`, `:39` `nucleo-matcher` | remove |
| `Cargo.toml:31` `crossterm` | remove the direct declaration; it returns as a transitive dependency of the picker crate (B5) |
| `ayx-rs/Cargo.toml:24-27` | remove the four `.workspace = true` lines; add the picker crate |
| `ayx-rs/tests/cli_smoke.rs:187-198` `tui_help_renders` | replace with `tui_stub_returns_remediation` (A3) |
| `ayx-rs/tests/one_inventory_drift.rs:92-100` v2 endpoint allowlist, `:115-124` workspace-detail carve-out | delete both (A1 makes the carve-out unnecessary) |
| `ayx-rs/src/cmd/catalog.rs` any `tui` row in `CATALOG_METADATA` | delete or reclassify to the stub |
| `README.md:115` (TUI operates on explicit file paths), `:310` (command list bullet) | rewrite `:115` to name only onboarding/migration; delete `:310` |
| `docs/output-format.md:36,41`, `docs/runtime-config-contract.md:13,15`, `docs/cli-spec.md:138` | drop the TUI clauses |
| `docs/command-surface.md`, `site/src/content/docs/reference/*.md` | regenerate with `cargo run -q -p xtask -- refresh-command-surface`; the release-notes pages are history and stay as written |
| `CHANGELOG.md` Unreleased | `### Removed` entry for `ayx tui` naming the replacements |

Expected result: `cargo tree -p ayx-rs -e normal` loses `ratatui*`, `tui-input`,
`nucleo-matcher`, `kasuari`, `line-clipping`, `compact_str`, `unicode-truncate`,
`instability`, `strum*`, `lru`, and their transitive-only crates. Verify the
count against the `v0.19.1` baseline (326 normal crates) in the plan.

### A3. Hidden deprecation stub (decision 2)

Keep a hidden `tui` subcommand for the `0.20.x` cycle that returns:

```json
{
  "ok": false,
  "error_code": "validation",
  "message": "ayx tui was removed in 0.20.0",
  "remediation": {
    "summary": "Use the targeted commands that replaced it",
    "commands": ["ayx onboard", "ayx one login", "ayx profile list", "ayx doctor"]
  },
  "retryable": false
}
```

Exit code 2. It is `#[command(hide = true)]` so it is absent from `--help`,
`discover`, and `catalog`. Remove it in `0.21.0`. Rationale: colleagues have
muscle memory from the internal releases, and a proper envelope beats clap's
generic "unrecognized subcommand".

### A4. Git hygiene

- Tag the last commit that still contains `ayx-rs/src/tui/` as `tui-final`
  (annotated, message pointing at ADR 0004).
- Delete the local, never-pushed branch `feat/tui-v2-phase2-cross-asset-drill`.
  Its two design docs were recovered into git history at `48567cc` and are
  summarized in ADR 0004; nothing else on it is wanted.

## Part B: agent hygiene

### B1. Output-mode auto-detection (decision 1)

Today `--output` defaults to `text` unconditionally (`main.rs:271-278`), and
the shipped skill has to tell agents to "always put `--output json` last."
Adopt the `gcx` pattern: detect the caller.

Resolution order for the effective output mode:

1. Explicit `--output <mode>` always wins.
2. `AYX_OUTPUT=<mode>` environment variable.
3. If stdout is not a terminal (`std::io::stdout().is_terminal()` is false)
   **or** an agent marker is present, `json`.
4. Otherwise `text`.

Agent markers: `AYX_AGENT=1` (explicit, documented), `CLAUDECODE` and
`AI_AGENT` (both observed set inside a live Claude Code session on 2026-09-04;
`AI_AGENT` is the Vercel `detect-agent` convention and other hosts may adopt
it). Add further host variables only after verifying them against the host; do
not guess. `CI=true` is deliberately *not* a marker on its own because
CI logs are read by humans; non-TTY stdout already covers CI pipelines.

When the resolved mode is `json` by rule 3, `--error-format` also resolves to
`json` unless explicitly set, and any human progress prose that is not already
on stderr must move there.

Implementation: `Cli.output` becomes `Option<output::OutputMode>`; add
`output::resolve_output_mode(explicit, env, stdout_is_terminal, agent_marker)
-> (OutputMode, OutputModeSource)` as a pure function with unit tests;
`--debug` logs the source. The `OutputModeSource` is not added to the envelope.

**Breaking behavior:** `ayx … | less` now shows JSON. The escape hatch is
`AYX_OUTPUT=text` (or `--output text`). Recommended: accept, because `0.20.0`
is the breaking release anyway and every script that parses output is better
off with JSON. Owner decision.

### B2. Envelope additions

`ayx-core/src/envelope.rs:126` gains three optional, additive fields.
`ayx.output.v1` is kept; the compact envelope (`output.rs:77-86`) mirrors them.

```rust
pub struct Remediation {
    pub summary: String,          // one sentence, human-readable
    pub commands: Vec<String>,    // zero or more exact commands to run next
}

pub struct Envelope {
    // existing fields …
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remediation: Option<Remediation>,   // errors only
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retryable: Option<bool>,            // errors only
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<Vec<String>>,          // successes only: suggested follow-ups
}
```

`retryable` is derived from `ErrorCode` at the dispatcher unless a command
sets it explicitly: `RateLimited`, `Network`, `Upstream`, `Incomplete` → true;
every other code → false.

`remediation` is populated at the existing classification sites in the
dispatcher first, so every command benefits without per-command work:

| Condition | `remediation.commands` |
| --- | --- |
| `ConfigMissing` | `ayx onboard`, `ayx profile list` |
| `AuthFailed` | `ayx one login` (One) / `ayx server api auth-check` (Server) |
| `WorkspaceMismatch` | `ayx one workspace switch <expected-id>`, `ayx one workspace current` |
| `PermissionDenied` with the known PAT-scope-wall endpoints | none; `summary` explains the scope wall and names `ayx one auth diagnose` |
| `Validation` from a missing selector off-TTY (B5) | the matching `… list --output json` |
| `NotFound` on a One id | the matching `… list` |

`next` is populated by list commands (`--page-token <token>` continuation when
`next_page_token` is present; `<family> detail <id>` for the first row) and by
`login`/`onboard` (`ayx one auth status`). Keep `next` short (≤3 entries); it
is a hint, not a plan.

**Schema fix.** `docs/cli-schema.json` declares `additionalProperties: false`
and omits `error_code`, so every error envelope the CLI has ever emitted fails
its own schema. Add `error_code` (enum of the 14 wire strings), `remediation`,
`retryable`, and `next`. Add a unit test that serializes a success and an error
envelope and validates both against the schema file (`jsonschema` crate, dev-
dependency only).

### B3. `--jq <filter>` and `--raw-output`

Global flags, implemented with the pure-Rust jaq crates (`jaq-core 3.1`,
`jaq-std 3.0`, `jaq-json 2.0`; MIT). Semantics:

- `--jq` forces a JSON output mode (compact `json` unless `--output json-full`
  is given) and applies the filter to the **redacted** envelope, so redaction
  cannot be bypassed by projection.
- Each filter result prints as one JSON value per line. `--raw-output` (`-r`)
  prints string results without quotes, matching `jq -r`.
- A filter compile or runtime error is a `validation` envelope (exit 2) with
  the jaq message in `data.jq_error`; the envelope goes to stderr like every
  other error.
- `--output-limit` applies before the filter (the filter sees what the user
  would have seen).

Binary-size budget: expect +1–2 MB. Measure and record in the plan.

### B4. `ayx one open <kind> <id>`

Deep-link the product web console. Base host is the profile's One base URL
(`AYX_ONE_BASE_URL` / profile field; `https://<region>.alteryxcloud.com`).

| `kind` | Path | Status |
| --- | --- | --- |
| `workspace` | `/?workspace=<name>&workspaceGid=<gid>` | pattern already parsed by `onboard.rs:1238-1239`; verify it deep-links |
| `workflow` | `/ayx-one/cloud-native/workflows/<ulid>` | noted in `one_workflows.rs:5` and `main.rs:1930`; verify |
| `flow`, `connection`, `plan`, `job-group`, `person` | unknown | ship only after each is verified in a browser; until then `open` returns `validation` with `remediation.summary` "no verified web path for <kind>; open <base-url> and search for <id>" |

Behavior: on a TTY, launch the default browser (`open` crate, MIT; falls back
to printing the URL if launching fails). Off a TTY or with `--print`, print
only the URL in `data.url` and never spawn a browser. `open` is read-only and
`CATALOG_METADATA` says so.

### B5. TTY-gated pickers (decision 3)

Pattern (from `gh`): the selector positional becomes optional; on a TTY with
input allowed, an omitted selector opens a picker over the corresponding list;
off a TTY or with `--no-input`, an omitted selector is a `validation` error
whose `remediation.commands` names the list command.

Curated set for `0.20.0` (all One, all already wired):

- `one workspace switch [id]` → picker over `workspace list` (shows name, id,
  ready-vs-needs-login from `workspace_credentials`)
- `one workflows detail|run|copy|share|delete [id]` → `workflows list`
- `one flows detail [id]` → `flows list`
- `one connections detail [id]` → `connections list`
- `one job-groups detail|cancel [id]` → `job-groups list`
- `one person detail [id]` → `person list`
- `one plans detail [id]` → `plans list` (tier-gated; picker surfaces the 404
  as a normal error)

Picker crate: `inquire` (MIT) `Select` with its fuzzy filter; it depends on
`crossterm`, which is why `crossterm` survives as a transitive dependency. Row
label is `<name>  (<id>)`; `Esc` cancels with a `validation` envelope
("selection cancelled"). The picker never appears when `--no-input`,
`AYX_NO_INPUT`, or non-TTY stdin/stdout is in effect, mirroring
`cmd/confirm.rs:31-39`.

Implementation: one helper, `cmd::select::resolve_selector(kind, given,
policy, fetch) -> Result<String>`, with the fetch closure supplied per
command so the helper stays product-agnostic. Unit-test the non-interactive
branches; the interactive branch gets one manual smoke on Windows Terminal and
one on a Linux terminal (colleagues are on Windows; the internal release is
cut there).

### B6. Stretch: `--watch` on `one job-groups status <id>`

Poll every `--interval` (default 5s) until the job group reaches a terminal
state. On a TTY, redraw one status line; off a TTY, emit one JSON Lines event
per poll and a final envelope. Bounded by `--timeout` (default 30m). Ship only
if it fits the release; otherwise it moves to Wave 1 unchanged.

## Testing and verification

- Unit: `resolve_output_mode` truth table; `retryable` derivation for all 14
  codes; `--jq` happy path, compile error, `--raw-output`; schema validation of
  success and error envelopes; `resolve_selector` non-interactive branches;
  `open` URL construction per kind and the unverified-kind error.
- Integration (`cli_smoke.rs`): `ayx tui` stub envelope and exit code; `ayx
  --help` contains no `tui`; `ayx discover --deep --output json-full` contains
  no `tui` node; `ayx one workspace detail --help`.
- Gates: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D
  warnings`, `cargo nextest run --workspace --locked`, `cargo run -q -p xtask
  -- refresh-command-surface --check`, `ayx actions validate`. Run every gate
  with an explicit per-worktree `CARGO_TARGET_DIR`.
- Live (needs a token): `one workspace detail <real id>` returns 200 and the
  endpoint-matrix row flips to `live 200`; `one open workflow <ulid> --print`
  URL resolves in a browser.
- Dependency delta: `cargo tree -p ayx-rs -e normal | wc -l` before and after,
  recorded in the PR.

## Docs to update alongside

- `docs/agent-guide.md` and `skills/ayx-cli-agent/SKILL.md`: `--output json`
  is still recommended explicitly, but non-TTY callers now get JSON by
  default; document `--jq`, `remediation`, `retryable`, `next`.
- `docs/output-format.md`: the resolution order, the new fields, `--jq`.
- `docs/cli-spec.md`: exit code for `--jq` errors; `tui` stub.
- Site `getting-started`: mention `one open` and pickers.

## Risks

- Humans piping to `less` see JSON (B1). Mitigated by `AYX_OUTPUT=text` and
  the breaking-release changelog.
- `inquire` raw-mode behavior on Windows terminals differs (ConPTY vs legacy
  console). Smoke on Windows Terminal before release.
- `jaq` adds binary size. Measure; it is well under the DuckDB-scale cost
  this wave deliberately avoids.
- Removing `disable_raw_mode()` from the OTP path could leave a terminal in a
  bad state after an interrupted login *if* the reason it was added is not what
  it appears. Read the blame before deleting.

## Decisions needed from the owner

1. **Non-TTY stdout defaults to `json`** (B1). Recommended: yes.
2. **Hidden `tui` deprecation stub for one cycle** (A3). Recommended: yes.
3. **`inquire` as the picker crate**, accepting `crossterm` as a transitive
   dependency (B5). Recommended: yes; the alternative is writing a raw-mode
   line editor by hand.
