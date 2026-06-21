# Mongo Registry And Remediation

Status: active

## Current Scope

- Support queries should live in a structured registry.
- Read-only diagnostics and write-capable remediation workflows should stay
  separate.

## Next Steps

- Add templates for queue inspection, orphan detection, app ownership, user
  migration, and results correlation.
- Expose a copy/paste mode that prints the exact `mongosh` command.
- Gate any mutation template behind explicit confirmation and audit output.

## Exit Criteria

- Support diagnostics are discoverable without hand-maintained snippets.
- Remediation flows are auditable and separate from read-only queries.
- The registry can be reused by CLI, docs, and future plugin surfaces.

