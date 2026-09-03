# Headless MCP Terra hostile review

Date: 2026-09-03

Scope: the v0.19.1 demo slice in `ayx-rs/src/headless.rs`,
`ayx-rs/src/cmd/headless.rs`, and the Agent Studio follow-up in
`ayx-rs/src/cmd/one_agent_assets.rs`, covering local product MCP over STDIO,
the generic authenticated MCP Gateway over Streamable HTTP, Agent Studio asset
registration, and the disposable-agent prompt path.

## Evidence

| Area | Result | Evidence |
| --- | --- | --- |
| Local executable boundary | Pass | An explicit file path is required; PATH is never searched implicitly. |
| Local process environment | Pass | Only the documented runtime allowlist is inherited. |
| Local stdout/stderr handling | Pass | Both streams are consumed on separate bounded readers; stderr is redacted before display. |
| Gateway endpoint boundary | Pass with follow-up | Only `http`/`https` endpoints are accepted and embedded credentials are rejected. A future profile allowlist should constrain untrusted automation inputs. |
| Credential handling | Pass | Gateway bearer tokens come from an environment variable or stdin, are not command-line arguments, are not placed in envelopes, and are absent from request errors. |
| Request/response bounds | Pass | Requests, local lines, input files, HTTP bodies, JSON messages, tool results, session IDs, and tool pagination are bounded. |
| Protocol ordering | Pass | `initialize` precedes discovery/calls and the initialized notification is sent before `tools/list`; numeric response IDs are matched. |
| Gateway transport | Pass for generic contract | JSON and SSE response bodies, `Mcp-Session-Id`, notification 202 responses, and bearer authorization are covered by tests. |
| Mutation safety | Pass | Raw calls are dry-run by default; applied calls require `--apply` and the existing TTY/`--yes` confirmation policy. |
| Agent Studio prompt safety | Pass with live-validation block | Prompt submission is preview-first, applied prompts require `--apply`, prompt input is non-empty and capped at 32 KiB, and prompt text is sent in the JSON body rather than a URL. The disposable-agent live attempt stopped before sending because the saved `local-dev` refresh token returned HTTP 400 from PingAuth. |
| Agent Studio asset scope | Pass with follow-up | Agent CRUD, dataset Insights registration, and workflow Apps-shortcut registration are explicitly labeled private-preview and inventoried in the endpoint matrix; no public OpenAPI support is claimed. |
| Product authorization | Not claimed | Product entitlements, Gateway scopes, tool permissions, and AOA authentication remain the product's responsibility. |
| Schema/contract validation | Follow-up | The demo preserves and describes the observed tool schema but does not yet validate input/output schemas or product versions. |
| Cancellation/telemetry | Follow-up | Child cleanup and HTTP timeouts exist; protocol cancellation, MCP correlation fields, and MCP lifecycle telemetry remain to be added. |

## Conclusion

No release-blocking source finding remains for the demo client slice. Live
Agent Studio prompt validation remains an environment follow-up until the
profile is reauthenticated. The release must continue to describe Gateway
support as contract-generic: workflow,
dataset, ability, and analytic-app views are derived from metadata observed
from the endpoint, not proof of product entitlement or a substitute for
curated product facades.
