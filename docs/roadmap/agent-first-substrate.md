# Agent-First Substrate And Governance

Status: active — strategy accepted 2026-09-04

## Framing

`ayx` is an agent-first administration substrate for Alteryx One and Alteryx
Server. Governance and access review is the first killer capability built on
that substrate. Both are delivered as CLI primitives that a human at a terminal,
a CI job, a shell-capable coding agent, or an MCP host can drive with the same
commands and the same envelopes.

This file tracks the four waves that follow from that framing. Each wave gets
its own design spec and implementation plan; this file holds only the
sequence, the principles, and the exit criteria. Durable decisions are in ADR
0004 (no bundled TUI) and ADR 0005 (AYX-owned MCP server shape). The external
research behind them is in
`docs/superpowers/specs/2026-09-04-agent-native-cli-field-survey.md`.

## Principles

- **The CLI is the primary agent interface.** Stable envelopes, a documented
  exit-code taxonomy, dry-run-by-default mutations, and a live machine-readable
  command tree (`discover`, `catalog`) are the contract. MCP is a thin,
  late-binding wrapper over that contract, never a second implementation.
- **Interactive affordances are TTY-gated.** Pickers, `--watch`, and `open`
  exist for humans and are inert for agents. There is no bundled TUI.
- **Errors carry remediation.** An error envelope names the next command to
  run and whether a retry can help, so an agent branches on structure instead
  of prose.
- **Mutations are plans.** Every mutating command can emit a plan artifact an
  agent produces and a human approves; `--apply` stays the runtime gate.
- **Snapshots are the audit trail.** Alteryx One exposes no audit-log API.
  Content-addressed snapshots of the governance inventory, diffed over time,
  are the honest substitute.
- **One and Server never conflate.** Governance primitives land under `one`
  first; Server gets its own `ayx server access …` later. Shared code lives in
  `ayx-core`; neither product crate reaches the other.

## Waves

| Wave | Theme | Status | Spec |
| --- | --- | --- | --- |
| 0 | Subtraction and agent hygiene: remove the TUI, `one workspace detail`, output auto-detection, `remediation`/`retryable`/`next` in the envelope, `--jq`, `one open`, pickers, a `--watch` stretch | spec written | `docs/superpowers/specs/2026-09-04-wave0-tui-removal-and-agent-hygiene-design.md` |
| 1 | Governance primitives for Alteryx One: `one access matrix\|explain\|review\|graph`, `one token audit`, `one snapshot take\|diff\|list` | design spec written; live re-verification checklist open | `docs/superpowers/specs/2026-09-04-wave1-one-access-governance-primitives-design.md` |
| 2 | Agent packaging: `ayx agent init` (host config for product MCP, generated `SKILL.md` installed per the Agent Skills spec), `ayx mcp serve` per ADR 0005, `one capabilities` (cached probe of what the current token and tier can reach) | decided (ADR 0005); spec to follow | — |
| 3 | Plans, policy, receipts: `--plan-out` on every mutating command and `ayx apply <plan>` with idempotency keys, `ayx policy check --rules`, a receipts ledger behind `ayx audit last\|since\|failed`; then curated `ayx.*` MCP tools over these | tracked; spec to follow | — |

The Headless product-MCP client track continues on its own file,
`headless-alteryx-integration.md`; its P4 "optional AYX-owned MCP server" item
is superseded by Wave 2 and ADR 0005.

## Deliberately deferred

- **Embedded SQL over snapshots** (a Steampipe-style `ayx query "select …"`
  with DuckDB). Real value for governance questions, but a 40–60 MB binary
  cost. Wave 1 ships `snapshot export --format parquet|csv` instead so users
  can bring their own engine; revisit embedding behind a feature flag if
  demand shows.
- **A separate dashboard binary.** Only if `--watch` proves insufficient for a
  real live-view need. Never bundled (ADR 0004).

## Next Steps

- Land Wave 0 as `0.20.0`. It is the breaking release (removes `ayx tui`).
- Run the Wave 1 live re-verification checklist against a real tenant before
  its implementation plan is written; several source endpoints were last
  probed in August.
- Confirm internally whether Agent Studio / AOA accepts third-party MCP
  servers. This gates the AOA positioning of Wave 2, not the build.
- Write the Wave 2 and Wave 3 design specs after Wave 1's collector exists;
  the curated MCP tools and the plan artifact both consume it.

## Exit Criteria

- The default binary contains no full-screen UI, and every interactive
  affordance is a TTY-gated primitive with a documented non-interactive path.
- A shell-capable agent can go from zero to a correct governance answer using
  only `ayx --help`, `ayx discover`, and envelopes, with no out-of-band docs.
- An MCP host can reach the whole command surface through three meta-tools
  and cannot mutate without operator consent.
- A One administrator can answer "who has access to what", "why", and "what
  changed since <date>" from the CLI, with evidence rows and explicit
  `blocked_by_scope` markers where the tenant or token cannot answer.
