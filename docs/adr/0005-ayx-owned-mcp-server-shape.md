# ADR 0005: AYX-Owned MCP Server Shape

Status: accepted
Date: 2026-09-04

## Context

ADR 0003 made AYX-RS a first-class MCP *client* of the product-owned Headless
Alteryx servers and deferred an AYX-owned MCP *server* to "later, for
capabilities AYX-RS itself owns." The client half shipped in `v0.19.1`
(`ayx headless doctor`, `ayx mcp tools`, `ayx mcp call`, `ayx mcp gateway`).

Three things have changed since ADR 0003 was written:

1. **The hosts that matter are known.** Shell-capable coding agents (Claude
   Code, Codex CLI, Gemini CLI, the Aria fleet) can run `ayx --output json`
   directly, so the CLI plus a shipped skill is their primary interface.
   Alteryx's own agent surface (Agent Studio / AOA) and no-shell chat or IDE
   hosts can only reach `ayx` through an MCP server.
2. **Alteryx's official MCP effort is Alteryx One only.** The Agent Studio and
   Alteryx MCP Server preview (announced 2026-05-20) exposes governed One
   assets to external hosts. Nothing covers on-prem Alteryx Server, cross-
   workspace governance, access audit, or CI/offline scripting. Those are the
   capabilities AYX-RS owns.
3. **The field has converged on how not to do this.** No vendor reflects its
   command tree one-to-one into MCP tools. Measured costs are large (one
   93-tool server consumed ~55K tokens before any work). The working patterns
   are meta-tool collapse (Cloudflare Code Mode: two tools over 2,500
   endpoints; Twilio: two tools over 1,800) and explicit exposure modes (Azure
   MCP `--mode namespace|all|single`, added because hosts cap tool counts).

AYX-RS already has the seams this needs: `command_surface::visible_commands()`
is the canonical live clap walk, `ayx discover --deep` and `ayx catalog` are
derived from it and CI-enforced to agree, and `execute()` returns an
`Envelope`. The official Rust SDK (`rmcp`, 3.x) provides stdio and Streamable
HTTP transports with `schemars`-derived tool schemas.

## Decision

Add `ayx mcp serve`, an AYX-owned MCP server, and promote it from the P4 slot
in `docs/roadmap/headless-alteryx-integration.md` to Wave 2 of
`docs/roadmap/agent-first-substrate.md`.

Its shape is fixed by this ADR:

- **Three meta-tools generated from the live clap tree, never one tool per
  command.** `ayx_discover(path?)` returns the `discover` subtree;
  `ayx_describe(path)` returns one leaf's arguments, options, safety class, and
  mutation flag; `ayx_run(argv, apply=false)` executes a command in-process and
  returns the standard envelope. Because all three read the same
  `visible_commands()` walk that `discover`, `catalog`, and `xtask` read, the
  tool surface cannot drift from the CLI.
- **Curated `ayx.*` tools are allowed only for capabilities that are already
  CLI commands** and that do not map to a single leaf (for example
  `ayx.access_explain`, `ayx.snapshot_diff`, `ayx.plan_apply`). A curated tool
  is a thin adapter over the command, not a second implementation.
- **An exposure knob, `--tools meta|curated|namespace`,** so operators can fit
  the server to a host's tool cap: `meta` (default) exposes the three
  meta-tools plus curated tools; `curated` exposes only curated tools;
  `namespace` exposes one routing tool per top-level command group.
- **Mutation policy is the CLI's policy.** `ayx_run` is dry-run unless the
  call passes `apply=true` *and* the server was started with `--allow-apply`.
  `--allow-apply` is the operator's session-level consent and stands in for the
  TTY confirmation an MCP host cannot provide; every applied call is written to
  the audit ledger with the MCP correlation id. Without `--allow-apply`, an
  `apply=true` call returns `permission_denied` with remediation.
- **Stdio first, Streamable HTTP later**, mirroring the client sequence.
- **Not a proxy.** The server never enumerates product `alteryx_local.*` or
  Gateway tools as its own. `ayx_run(["mcp","call",…])` does reach a product
  server, but through the AYX client, apply gate, redaction, and envelope, and
  the result carries `backend: product_mcp` provenance. That is composition
  through the AYX policy layer, which ADR 0003 permits; a transparent
  forwarding chain is what it forbids.

Preconditions, in order: make `execute()` callable in-process as
`run(argv, ctx) -> Envelope` without `process::exit`; make the thread-local
`--apply` gate an explicit per-call parameter; then add the `rmcp` server.

## Alternatives considered

### One tool per command (reflect the clap tree)

Rejected. 399 tools would exceed every known host cap and cost tens of
thousands of tokens per session before the agent does anything.

### Hand-curated tools only

Rejected as the sole shape. It forfeits the 399-command surface that makes
`ayx` useful and recreates the drift the derived catalog eliminated. Curated
tools remain as an additive layer.

### No AYX-owned server; CLI and skill only

Rejected. It leaves AOA and no-shell hosts with no path to AYX-owned
capabilities, which is exactly the white space Alteryx's One-only MCP leaves
open. The CLI stays primary for shell-capable agents regardless.

## Consequences

- `rmcp` becomes a dependency of `ayx-rs`.
- `discover` and `catalog` output shapes become a compatibility surface for the
  server; they already carry `schema_version` and must keep it.
- Whether AOA/Agent Studio accepts third-party MCP servers is a precondition
  for the AOA positioning, not for building the server; the no-shell-host case
  stands on its own. Verify it internally before marketing the AOA angle.
- ADR 0003 is not superseded. This ADR fixes the shape and timing of the
  server ADR 0003 anticipated.

Design: Wave 2 of `docs/roadmap/agent-first-substrate.md` (spec to follow).
