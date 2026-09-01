# ADR 0003: Headless Alteryx MCP Integration Boundary

Status: accepted
Date: 2026-08-28

## Context

Alteryx is delivering a product-owned local MCP server through AOA/AI
Services. The server owns Designer-compatible workflow semantics, licensing,
authentication, and the private Designer bridge. AYX-RS already has useful
direct One API, local XML, package, and EngineCmd capabilities, and it is also
the repository's agent-oriented orchestration and audit substrate.

We need to integrate with the product implementation without duplicating its
tool catalog or making Claude, Codex, and other MCP hosts depend on an AYX-RS
forwarding process. We also intentionally want to retain direct XML and other
reverse-engineered capabilities when they unlock behavior that the product
surface does not expose.

## Decision

AYX-RS is a first-class MCP **client and integration substrate**. The
product-owned MCP server remains authoritative for product execution
semantics.

Product MCP owns:

- AOA authentication, licensing, entitlements, updates, and local installation
- DesignerCore and Designer-compatible workflow mutation and run behavior
- the local `alteryx-mcp-server.exe` process and its `alteryx_local.*` contract
- the Alteryx One MCP Gateway and its cloud toolsets when those are published

AYX-RS owns:

- MCP process discovery, negotiation, invocation, diagnostics, and host setup
- composition across product MCP, direct One APIs, XML, and EngineCmd
- backend selection, policy, review/apply gates, audit, recovery, and result
  envelopes
- agent-facing command and capability discovery
- explicit direct XML and reverse-engineered power lanes

AYX-RS must not transparently proxy or reimplement the product MCP catalog. An
AYX-owned MCP server may be added later for capabilities that AYX-RS itself
owns, such as audited promotion plans, environment drift, or remediation. It
must use an unmistakably AYX-owned namespace and must not be a pass-through
facade for Designer tools.

When a product MCP tool exists and is compatible, it is the preferred path for
the corresponding product operation. Direct API, XML, and EngineCmd paths
remain available as explicit alternatives for control-plane, bulk,
diagnostic, offline, unsupported-version, or experimental use cases.

## Alternatives considered

### Transparent AYX-RS MCP proxy

Rejected. It duplicates MCP lifecycle and authentication complexity, obscures
product diagnostics, increases schema-drift risk, and makes native host
configuration less direct.

### Reimplement Designer semantics in AYX-RS

Rejected as the default. It would create a second execution engine and make
behavior diverge from Designer. Direct XML and EngineCmd remain supported as
explicit power lanes, but their provenance and limitations must be visible.

### Product MCP only, with no AYX integration layer

Rejected. It would leave cross-surface orchestration, One API coverage,
policy, audit, recovery, backend choice, and agent setup fragmented.

## Consequences

Positive:

- Product owns the behavior that must remain Designer-compatible.
- External MCP hosts can register the product server directly.
- AYX-RS can add value across local, cloud, and reverse-engineered surfaces.
- Backend behavior can be tested and explained independently.

Costs and constraints:

- The MCP client must handle product version and schema drift.
- AOA credentials and AYX One credentials remain separate trust domains.
- Direct XML/EngineCmd operations need explicit warnings, policy, audit, and
  compatibility tests.
- An `auto` backend choice must be observable and must not silently substitute
  non-equivalent semantics for a mutating operation.

## Implementation implications

The first implementation slice is an internal `McpSession` abstraction with a
STDIO transport, secure product-server discovery, read-only diagnostics, and
redacted contract snapshots. Curated workflow operations come after the
observed product contract is stable. Streamable HTTP support follows for the
cloud Gateway.

See:

- [Headless MCP architecture](../integrations/headless-mcp-architecture.md)
- [Headless MCP contract](../integrations/headless-mcp-contract.md)
- [Backend selection](../integrations/backend-selection.md)
