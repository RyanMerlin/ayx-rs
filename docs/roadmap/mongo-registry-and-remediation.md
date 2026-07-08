# Mongo Registry And Remediation

Status: active

## Current Scope

- Support queries should live in a structured registry.
- Read-only diagnostics and write-capable remediation workflows should stay
  separate.

## Next Steps

- Add dedicated orphan-detection and results-correlation templates (the
  current set has no orphan-detection template and no correlation beyond
  generic results health).
- Mutation templates are preview-first and require `--accept-mutation-risk`
  + `--apply`, but actual execution is still disabled and has no audit-artifact
  path analogous to backup/restore. Decide whether to ship a real auditable
  execution path.

## Exit Criteria

- Support diagnostics are discoverable without hand-maintained snippets.
- Remediation flows are auditable and separate from read-only queries.
- The registry can be reused by CLI, docs, and future plugin surfaces.
