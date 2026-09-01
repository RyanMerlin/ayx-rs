# Headless MCP Architecture

Status: implementation design
Date: 2026-08-28

This document describes the AYX-RS integration with the product-owned local
Headless Alteryx MCP server. It is an AYX-RS design document, not a duplicate
of the product's internal Designer bridge design.

## Product references

- [Headless Alteryx strategy](https://alteryx.atlassian.net/wiki/spaces/intelligence/pages/4420304942/Headless+Alteryx)
- [Solution design](https://alteryx.atlassian.net/wiki/spaces/GA/pages/4419551317)
- [Local design](https://alteryx.atlassian.net/wiki/spaces/GA/pages/4098228353)
- [Tool catalog](https://alteryx.atlassian.net/wiki/spaces/~67460216/pages/4543349059)

## Scope and non-goals

In scope:

- launching and managing the product local MCP server over STDIO
- negotiating the MCP protocol and discovering the published tool contract
- invoking tools with bounded, typed, redacted results
- composing product MCP with existing One APIs and explicit local power lanes
- diagnostics, compatibility, host configuration, policy, and audit

Not in scope for this layer:

- replacing DesignerCore or the product's private bridge
- copying AOA credentials into AYX-RS profiles
- automating the Designer UI canvas
- claiming direct XML behavior is equivalent to Designer behavior
- exposing an unauthenticated product-MCP fallback

## Runtime topology

```text
Claude / Codex / Gemini / other MCP host
       |                         |
       | direct product MCP     | optional AYX-owned tools later
       v                         v
alteryx-mcp-server.exe      ayx MCP server (not a proxy)
       |
       | AOA-owned auth/licensing + private Designer bridge
       v
DesignerCore / local Designer capabilities

ayx CLI / agent substrate
       |
       +-- Product MCP client (STDIO first, HTTP later)
       +-- Existing Alteryx One API clients
       +-- Direct XML / EngineCmd power lanes
       +-- Policy, approvals, audit, artifacts, recovery
```

The host may connect directly to the product server. AYX-RS also provides a
client path for workflows that need diagnostics, cross-surface composition, or
an explicit backend choice. These are complementary connections, not a
transparent forwarding chain.

## Components

### Product MCP installation

`ProductMcpInstallation` resolves the AOA-managed executable and reports its
path provenance, publisher/signature status, version, AOA/Designer prerequisites,
and whether an explicit override was used. The implementation must not execute
the first matching executable found on `PATH` without validation.

### MCP session

`McpSession` owns one bounded server process or HTTP connection. It performs
initialization, capability negotiation, tool listing, invocation, cancellation,
and orderly shutdown. A session does not own AOA credentials and does not
persist product authentication state.

### Backend router

The router resolves an operation to `product_mcp`, `one_api`, `direct_xml`, or
`engine_cmd`. The selected backend and the reason for selection are included in
the command result and audit artifact. Mutating operations require explicit
compatibility and approval checks.

### Result and artifact layer

Small structured results may be returned inline. Large tool results, workflow
XML, logs, and anchor data are stored as permission-restricted artifacts and
referenced by path or handle. Standard logs and telemetry exclude raw XML,
tokens, connection strings, dataset contents, and unredacted paths.

## Lifecycle

1. Resolve an operation, workspace roots, profile, and requested backend.
2. For product MCP, resolve and validate the AOA-managed installation.
3. Start the server with an allowlisted executable and a minimal environment.
4. Send MCP `initialize`; verify protocol version and capabilities.
5. List or resolve the required tool and validate its input schema.
6. Invoke with an operation-specific timeout, output limit, and approval state.
7. Decode structured content and persist oversized output as an artifact.
8. On cancellation, send the protocol cancellation request, wait briefly for
   acknowledgement, then terminate the child process if necessary.
9. Emit redacted audit and diagnostic records with session and correlation IDs.
10. Shut down the session and release process/transport resources.

Transport disconnect is not treated as cancellation. A crashed child process,
malformed response, timeout, or schema mismatch is a distinct failure class.

## Trust and security boundaries

- AOA owns product authentication, licensing, and entitlement state.
- AYX profiles own direct One credentials only; they never store or export AOA
  MCP tokens.
- Tool descriptions, asset metadata, returned errors, and data samples are
  untrusted model context and are subject to size limits and redaction.
- Workflow and dataset paths are canonicalized and constrained to configured
  workspace roots.
- MCP annotations are advisory metadata, not authorization. AYX policy and
  user approval govern mutating or multi-step actions.
- Product MCP and direct XML/EngineCmd results carry different backend labels.

## Versioning and compatibility

Compatibility is based on negotiated protocol version, observed tools, and
schemas—not only on the Designer version. AYX-RS stores redacted contract
snapshots keyed by product-server version and uses them for diagnostics and
regression tests. Unsupported or partially supported capabilities fail with a
clear explanation of the missing tool/schema.

## Delivery sequence

1. Fake STDIO server and session contract tests.
2. Secure installation discovery and `headless doctor`.
3. Read-only tools list/describe/call.
4. Curated local workflow operations.
5. Explicit XML/EngineCmd comparison and power-lane controls.
6. Streamable HTTP client for the cloud Gateway.
