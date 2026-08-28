# Headless Alteryx Integration

Status: active — initial inventory and sequencing pass

This is the AYX-RS roadmap for integrating with Alteryx's product-owned
Headless Alteryx MCP capabilities while keeping AYX-RS independent from the
AOA/Nexus implementation. It captures the current product direction and the
work needed for AYX-RS to become a first-class agent substrate.

## Source and context

The source material reviewed for this pass:

- [Headless Alteryx strategy](https://alteryx.atlassian.net/wiki/spaces/intelligence/pages/4420304942/Headless+Alteryx)
- [Headless Alteryx solution design](https://alteryx.atlassian.net/wiki/spaces/GA/pages/4419551317)
- [Milestones and capability matrix](https://alteryx.atlassian.net/wiki/spaces/intelligence/pages/4623335685)
- [Phase 1 local MVP](https://alteryx.atlassian.net/wiki/spaces/intelligence/pages/4419584222)
- [Local AOA MCP server design](https://alteryx.atlassian.net/wiki/spaces/GA/pages/4098228353)
- [Local MCP tool catalog](https://alteryx.atlassian.net/wiki/spaces/~67460216/pages/4543349059)
- [Phase 1 telemetry plan](https://alteryx.atlassian.net/wiki/spaces/intelligence/pages/4475060361)

The milestone dates below are product planning signals, not AYX-RS release
commitments. The initial review was performed on 2026-08-28.

## Strategy boundary

Product-owned MCP remains authoritative for product execution semantics:

- AOA authentication, licensing, entitlements, updates, and DesignerCore
- the local `alteryx-mcp-server.exe` process and its public `alteryx_local.*`
  contract
- the private Designer bridge and Designer-compatible workflow mutation/run
  behavior
- the Alteryx One MCP Gateway and its cloud toolsets

AYX-RS remains an independent client and substrate:

- discover, negotiate, inspect, and invoke product MCP tools
- compose local MCP, cloud MCP, direct One API, XML, and engine operations
- provide agent setup, compatibility diagnostics, policy, approvals, audit,
  recovery, and stable result envelopes
- retain direct XML/reverse-engineered capabilities where they unlock useful
  behavior outside the product surface

AYX-RS should not transparently proxy or reimplement the product MCP catalog.
An AYX-owned MCP server may be added later for distinctly AYX-owned
orchestration tools, but it should not replace direct product MCP registration.

## Product roadmap to track

| Product milestone | Intended capability | AYX-RS implication |
| --- | --- | --- |
| Milestone 0 (completed 2026-06-30) | Skills-based local workflow generation/execution fallback | Preserve and improve the direct XML/engine lane |
| Milestone 1 (target 2026-07-28) | Local MCP build, run, inspect, troubleshoot, revise | Implement the product MCP client and local workflow facade |
| Milestone 2 (release-ready 2026-09-02) | Cloud workflow/dataset discovery and reuse using keyword metadata | Add cloud MCP transport and map existing One APIs to agent intents |
| Milestone 2 fast follow (2026-08-31) | Semantic and lineage-enhanced discovery | Add capability/schema-based feature detection; do not assume availability |
| Milestone 3 planning target (2026-09-30) | Connections, LiveQuery, sharing, scheduling, plans, orchestration | Add cross-surface composition and operational workflows |

The product pages contain unresolved scope contradictions that must be treated
as explicit compatibility questions: semantic search is both listed in
Milestone 2 and footnoted as after Milestone 2; LiveQuery is promised in the
Milestone 3 narrative but marked unavailable in the capability matrix; trust
signals and Orchestrator plans remain unresolved.

## Current AYX-RS position

### Existing strengths to reuse

- Secure profile, keyring, workspace selection, token refresh, and redaction
  infrastructure in `ayx-core`.
- Structured envelopes, dry-run/apply gates, audit artifacts, retries,
  pagination, and opt-in JSONL API observability.
- Local workflow inspection, XML validation, replacement, recursion,
  migration, packaging, YXDB parsing, and desktop-to-cloud conversion.
- Cloud-native workflow listing/detail/dependencies/tools/engines/copy/share;
  Designer Cloud flow lifecycle and execution; connections and connector
  metadata; datasets; plans; scheduling; job groups; and output objects.
- Machine-readable `discover`, `catalog`, capability schemas, and action/workflow
  registry contracts for agent consumption.

### Gaps that matter to Headless

- No MCP client or STDIO/Streamable HTTP lifecycle implementation.
- No product MCP installation discovery, compatibility report, tool snapshot,
  agent configuration generator, or `alteryx-mcp-server.exe` integration.
- No local Designer bridge/CoreCLR integration and no real local workflow run;
  the current designer capability run is offline validation/context capture.
- Capability mutations directly rewrite XML and may return complete XML. This is
  a useful power lane, but it is not equivalent to DesignerCore semantics and
  must be labeled and governed as such.
- No Copilot deterministic utility client (`knowledge/search`, tool schemas,
  configuration generation, workflow condensation).
- No complete `alteryx_local.*` tool facade, including data-at-anchor and the
  Phase 1 fast-follow organization/documentation tools.
- No cloud MCP Gateway client, solution-planning layer, semantic/lineage asset
  search, ranking/trust selection, or cloud workflow authoring facade.
- Dataset support is read-oriented; cloud-native workflow commands do not yet
  expose the product's build/run/repair lifecycle through MCP.
- No MCP-specific lifecycle telemetry, provider/skill/session correlation, or
  product-compatible funnel reporting.
- Capability execution through `catalog run` does not yet have the same explicit
  apply/approval posture as normal mutating CLI commands.

## Initial sequencing and priority

Priority is intentionally coarse. Each item becomes a separate leaf plan item
after the old roadmap notes are cleaned up.

### P0 — Contract, boundary, and safety foundation

- Record an ADR for the direct-product-MCP integration model, explicit power
  lanes, backend provenance, and the rule against transparent proxying.
- Define a public AYX-RS backend model such as `product_mcp`, `one_api`,
  `direct_xml`, and `engine_cmd`; make `auto` decisions visible in output.
- Create a separate MCP/AOA credential boundary. AYX-RS must not copy or expose
  AOA Designer tokens, refresh tokens, or local auth state.
- Obtain redacted `initialize`, `tools/list`, `get_info`, and representative
  result fixtures for supported AOA/Designer releases.
- Add a protocol test harness with a scripted fake STDIO MCP server covering
  handshake ordering, malformed messages, pagination, stderr noise, timeout,
  cancellation, and process crashes.
- Define bounded result/artifact handling, path-root policy, redaction rules,
  and mutation approval behavior before exposing a raw call escape hatch.

### P1 — First-class product MCP client

- Implement an internal `McpSession` abstraction with STDIO transport first:
  initialize/version negotiation, capabilities, tool discovery, invocation,
  structured content, cancellation, orderly shutdown, and child cleanup.
- Add `ayx headless doctor` for AOA executable discovery, publisher/signature
  and version provenance, handshake, compatibility, and public tool inventory.
- Add `ayx mcp tools list`, `tools describe`, and a permanently supported raw
  `call` command for product tools.
- Generate and validate host-native Claude/Codex/Gemini configuration that
  registers the product MCP directly.
- Document and diagnose the product installation prerequisites: AOA/AI Services,
  .NET 8 Desktop Runtime, Designer 26.2+, the AOA-managed `PATH` entry, and
  agent restart after installation/update. Include managed desktop, Citrix,
  VDI, and non-admin failure modes from the product help flow.
- Add redacted tool-contract snapshots keyed by product/server version and
  schema-based compatibility checks.
- Add provider, skill version, correlation, latency, normalized error, and
  backend provenance fields to AYX-RS MCP call/audit records.
- Keep product skill boundaries visible: `alteryx-designer` executes approved
  asset internals; `alteryx-asset-discovery` finds context; future
  `alteryx-solution-planning` chooses reuse versus create; `alteryx-livequery`
  handles warehouse execution; and `alteryx-operations` handles post-build
  scheduling, sharing, and monitoring.
- Track the Phase 1 funnel metrics separately from LLM quality: plugin-qualified
  sessions, local connection success, intended discovery/schema/build/run path,
  workflow build success, workflow run success, and workflow creator source.
  The product preview targets are baseline-first, then 85%+ connection success,
  50%+ intended-path completion, and 70%+ build/run success.
- Mirror the product event vocabulary at the AYX integration boundary where
  useful: `mcp_server_booted`, `mcp_session_started`, `mcp_tool_called`, and
  `mcp_tool_completed` with provider, skill/server versions, hashed session and
  correlation IDs, tool category, success, latency, retry, and normalized error
  fields. `mcp_session_ended` is currently documented by Product as not planned.

### P2 — Curated local Headless workflows and power lanes

- Add a curated local facade for product tools where the contract is stable:
  create, inspect, metadata, add/edit/remove tools, connection replacement,
  validate, run, and data-at-anchor.
- Keep direct XML and reverse-engineered operations as an explicit backend for
  offline use, unsupported product versions, experimentation, and capabilities
  not exposed by MCP. Preserve current file-first behavior and add clear
  warnings/limitations rather than silently substituting semantics.
- Add explicit `AlteryxEngineCmd.exe` discovery, version checks, invocation,
  cancellation/timeout, bounded output capture, and run-result artifacts.
- Align capability mutations with the CLI's review/apply model, including
  previews, path policy, audit records, and no raw XML in standard output unless
  explicitly requested.
- Add compatibility and round-trip fixtures for XML mutations, engine runs,
  product MCP mutations, and differences between the backends.
- Add organization/documentation tools when the product fast-follow contract is
  confirmed: containers, annotations, workflow description, and positions.

### P3 — Cloud MCP and asset-oriented composition

- Implement Streamable HTTP MCP support for the Alteryx One MCP Gateway after
  the STDIO client contract is stable.
- Map existing One API primitives into agent-oriented operations while retaining
  direct API paths for admin, bulk, diagnostic, and unsupported MCP functions.
- Add first-pass keyword asset discovery for workflows and datasets, then
  capability-gated semantic and lineage search.
- Track the staged discovery matrix explicitly: workflow and dataset search in
  the first cloud milestone; semantic workflow/dataset search and lineage fast
  follow; macro and measures/insights discovery later; third-party catalog asset
  discovery only when its upstream contract is available.
- Add an explicit solution-planning result: reuse, extend, replace, or create;
  asset responsibilities, conceptual inputs/outputs, assumptions, and evidence.
- Add cloud workflow build/run/inspect/repair adapters when the Gateway exposes
  the required contracts; do not infer arbitrary authoring from list/detail APIs.
- Add connection and dataset discovery/creation adapters as product contracts
  become available, with trust/ranking signals treated as optional capabilities.
- Track XML-AMP workflow building, CCM/DCM connection discovery and creation,
  SDLC package promotion, and trust/ranking signals (labels, promotion,
  schedules, usage, recency, approvals) as independently negotiated product
  capabilities rather than assuming the milestone narrative implies support.

### P4 — Operational AI infrastructure

- Compose validated local/cloud capabilities into resumable plans with
  idempotency keys, correlation IDs, compensating actions where possible, and
  durable audit artifacts.
- Integrate sharing, scheduling, plan execution, job monitoring, and session or
  result sharing behind explicit policy and approval gates.
- Add LiveQuery adapters only after the product contract is confirmed for each
  supported warehouse; capability negotiation must handle partial coverage.
- Add the product's broader operational surfaces when exposed: third-party
  catalog assets, workflow/session sharing, reusable plans, and orchestration
  across multiple systems. Treat the current Orchestrator-plan milestone as an
  open design item until its contract is published.
- Add an optional AYX-owned MCP server only for AYX-owned orchestration tools
  such as promotion plans, environment drift, and audited remediation. It must
  not be a transparent Designer-tool proxy.
- Add opt-in product canary tests on installed Windows AOA/Designer hosts and
  keep direct One API tests as a separate auth/trust path.

## Cross-cutting risks and controls

- **Executable hijacking:** resolve the AOA-managed install location where
  possible; validate publisher/signature/version and show provenance.
- **Credential leakage:** keep AOA MCP auth separate from AYX One profiles; do
  not log tokens, connection strings, DCM data, or raw workflow XML.
- **Tool poisoning:** treat tool descriptions, asset metadata, errors, and data
  samples as untrusted model context; enforce size limits and typed decoding.
- **Filesystem escape:** canonicalize explicit workflow paths and enforce
  configured roots; reject traversal and record the selected root in audits.
- **Approval bypass:** MCP annotations are advisory. AYX policy and user
  approval must govern multi-step and mutating operations.
- **Run lifecycle:** propagate cancellation, wait for acknowledgement, and
  terminate failed child processes; a transport disconnect is not cancellation.
- **Version drift:** negotiate tools and schemas dynamically, snapshot contracts,
  feature-gate by observed capability, and fail clearly on incompatibility.
- **Product/reverse-engineered divergence:** maintain separate backend labels and
  tests so direct XML behavior is powerful without being misrepresented as
  DesignerCore behavior.
- **Telemetry boundary:** record only technical lifecycle facts. Never collect
  raw prompts, full workflow XML, plain filesystem paths, dataset contents,
  connection strings, DCM secrets, or inferred LLM reasoning quality. The
  product telemetry design places events at the MCP server boundary and treats
  the Designer bridge as telemetry-free.
- **Update lifecycle:** signed AOA-owned MCP and bridge binaries must be
  discoverable and updateable; update helpers need to stop running MCP/bridge
  processes before replacing loaded binaries.
- **No unauthenticated local fallback:** product MCP tools require valid AOA
  authentication/licensing/entitlements, including licensed offline activation.
  AYX-RS direct XML/engine lanes are separate explicit modes, not an implicit
  bypass of product authorization.

## Exit criteria for this roadmap slice

- Product MCP, direct API, XML, and engine responsibilities are documented and
  independently testable.
- A user can diagnose and invoke the product local MCP from `ayx` without
  exposing AOA credentials or requiring a transparent proxy.
- A user can choose or inspect the backend used for a workflow operation.
- Multi-step local/cloud operations produce reviewable, redacted, resumable
  artifacts.
- Product contract drift and direct-backend regressions are visible in tests and
  diagnostics rather than discovered through opaque agent failures.
