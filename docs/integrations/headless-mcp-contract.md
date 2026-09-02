# Headless MCP Client Contract

Status: demo client implemented; product contract integration remains active
Date: 2026-08-28

This is the contract between AYX-RS and any compatible product-owned Headless
Alteryx MCP server. Product tool names and schemas remain product-owned; this
document defines how AYX-RS discovers and consumes them.

## Core abstractions

The initial Rust implementation should hide the selected MCP library behind
internal interfaces similar to:

```text
McpConnectionSpec
  Local { executable, args, cwd, env_allowlist }
  Http  { endpoint, bearer_token_source }

McpSession
  initialize(client_info) -> ServerInfo
  list_tools(cursor) -> ToolPage
  describe_tool(name) -> ToolDefinition
  call_tool(name, input, CallOptions) -> ToolResult
  cancel(request_id) -> result
  close() -> result
```

The public CLI should expose stable AYX-RS envelopes rather than leaking a
third-party Rust MCP SDK type. The raw product tool payload remains available
through an explicit diagnostic escape hatch.

## Initialization

The client must:

- send `initialize` before any tool operation
- negotiate a supported protocol version
- record server name, version, capabilities, and transport
- report the negotiated version and leave product compatibility decisions to
  the observed contract until supported product versions are published
- send the protocol initialization notification before listing tools

Initialization output is diagnostic metadata, not a credential or a product
entitlement assertion. AOA remains the source of truth for authentication and
licensing.

## Tool discovery

`tools/list` is paginated and may change between sessions. AYX-RS should:

- support pagination and bounded page/result sizes
- preserve the product's exact tool names and schemas in snapshots
- resolve a required tool by name and required input fields
- distinguish unavailable, incompatible, and temporarily failing tools
- avoid hard-coding the complete product catalog into the CLI

The curated AYX-RS facade may map stable product tools to higher-level actions,
but it must retain the raw tool name and server version in provenance.

## Invocation and results

Every call has:

- a client correlation ID and MCP request ID
- a timeout and maximum result size
- a declared read-only or mutating policy class
- a workspace-root and artifact policy where filesystem access is involved
- an approval state for mutating operations

Results are decoded as structured content when available. Text, images, links,
and embedded resources are preserved by type. Oversized or sensitive values are
written to restricted artifacts and returned by reference. Errors are
normalized for the agent envelope while retaining a redacted raw diagnostic.

## Compatibility model

The compatibility profile is derived from:

- negotiated protocol version
- product server and Designer/AOA versions
- required tool names
- required input/output schema fields
- known behavior flags captured by contract fixtures

Feature checks must be capability-based. A Designer version is useful context,
but it is not sufficient evidence that a tool or schema is present.

## CLI-facing diagnostics

The first client-facing operations should be:

```text
ayx headless doctor
ayx mcp tools list
ayx mcp tools describe <tool>
ayx mcp call <tool> --input <json-or-file>
ayx mcp gateway abilities
ayx mcp gateway tools list --family workflow
ayx mcp gateway tools list --family dataset
ayx mcp gateway call <tool> --input <json-or-file> --apply --yes
```

JSON output should include transport, server provenance, negotiated protocol,
tool name, backend, and redacted errors. Correlation IDs, schema validation,
and product-specific corrective guidance remain follow-up contract work. Human
output should explain the next corrective action, such as installing .NET 8
Desktop Runtime, restarting an agent after an AOA update, or selecting an
explicit power-lane backend.

## Contract fixtures and tests

Commit only redacted fixtures. The fake-server suite must cover initialization
ordering, malformed JSON-RPC, pagination, stderr noise, timeout, cancellation,
child crash, schema variation, structured content, and large-result handling.
Opt-in Windows canaries may validate an installed AOA/Designer host with one
harmless inspection and one isolated mutation/run test.

## Open product-contract questions

These remain compatibility inputs rather than assumptions:

- exact local executable discovery and supported launch arguments
- published tool schemas for workflow creation, execution, and anchor data
- whether product fast-follow organization/documentation tools are available
- the product cloud Gateway endpoint, regional routing, authentication scope,
  and published workflow/dataset/ability tool names
- cancellation behavior and run-result artifact semantics

The generic Gateway client uses an explicit `--endpoint` (or
`AYX_MCP_GATEWAY_ENDPOINT`) and a bearer token from `--token-env` (or
`AYX_MCP_GATEWAY_TOKEN`) / `--token-stdin`. It does not claim product feature
availability until the Gateway reports the corresponding capabilities or tool
metadata. `abilities` is therefore a discovery view, not a replacement for
product entitlements or authorization.
