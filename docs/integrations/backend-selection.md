# Headless Backend Selection and Provenance

Status: implementation design
Date: 2026-08-28

AYX-RS has multiple ways to interact with Alteryx. They are complementary, but
they do not all have the same semantics, authentication boundary, or support
guarantee. This document defines how the CLI chooses and reports a backend.

## Backends

| Backend | Owner | Best use | Important limitation |
| --- | --- | --- | --- |
| `product_mcp` | AOA/Designer | Designer-compatible local inspect, mutate, validate, run, and anchor operations | Requires compatible AOA/Designer installation and product auth/licensing |
| `one_api` | AYX-RS integration | Cloud control plane, bulk/admin operations, diagnostics, and published REST capabilities | Does not imply arbitrary visual workflow authoring or local Designer semantics |
| `direct_xml` | AYX-RS power lane | Offline inspection, experimentation, unsupported product versions, and capabilities not exposed by MCP | XML mutation is not automatically equivalent to DesignerCore behavior |
| `engine_cmd` | AYX-RS power lane | Explicit local execution through `AlteryxEngineCmd.exe` | Requires local engine installation; lifecycle and output behavior differ from MCP |

## Selection policy

An operation may request a backend explicitly:

```text
--backend=product_mcp
--backend=one_api
--backend=direct_xml
--backend=engine_cmd
```

`--backend=auto` is permitted only when the operation declares that the
candidate backends are semantically interchangeable enough for automatic
selection. Auto-selection must report its decision and rationale.

For product-semantic mutations, `auto` prefers a compatible product MCP
implementation. If it is unavailable, the CLI must fail with a remediation
message or require an explicit non-product backend. It must not silently turn a
Designer-compatible operation into direct XML editing.

For cloud control-plane operations, `one_api` remains the default unless a
compatible cloud MCP Gateway operation is explicitly selected. For offline
file operations, `direct_xml` may remain the natural default. For local
execution, `engine_cmd` is explicit until a product MCP run contract is
available and validated.

## Provenance

Every agent-readable result and audit record should include:

```text
backend: product_mcp | one_api | direct_xml | engine_cmd
backend_version: product/server/API/engine version when known
selection: explicit | auto
selection_reason: stable machine-readable reason
compatibility: verified | partial | unknown | unsupported
```

Human output should make the choice visible, for example:

```text
backend: direct_xml (explicit)
compatibility: partial — XML mutation is not DesignerCore execution
artifact: C:\...\preview\workflow.xml
```

## Safety requirements

- Mutating operations use the existing review/apply model where possible.
- Direct XML writes produce previews, backups or atomic replacement, path-root
  checks, and audit records.
- EngineCmd runs have bounded output capture, timeout, cancellation, and a
  run-result artifact.
- Product MCP calls never receive AYX One profile secrets.
- Backend selection never bypasses a product authentication or entitlement
  check implicitly.
- Raw XML, tokens, connection strings, and data payloads are excluded from
  standard logs and telemetry.

## Backend comparison

When behavior is intentionally compared, the result should identify the
operation, input artifact hash, backend versions, output artifact references,
and known semantic differences. Comparison is diagnostic; it does not make a
power-lane result product-certified.

## Future routing extensions

The router can later add product cloud MCP, LiveQuery, or specialized catalog
providers. New backends must define their owner, auth boundary, capability
contract, compatibility evidence, and provenance fields before entering
`auto` selection.
