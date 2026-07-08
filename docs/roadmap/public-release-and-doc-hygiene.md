# Public Release Hygiene

Status: active

## Current Scope

- `RyanMerlin/ayx-rs` is the public source of truth for code, releases, and
  issue tracking.
- Public fixtures and generated artifacts must stay sanitized.
- Release plumbing should continue to point install, update, and publish flows
  at the public GitHub repository.

## Next Steps

- Decide whether the workspace template writer should stop emitting editable
  placeholder secrets and move fully to env/keyring-first guidance.
- Refresh `docs/public-release-checklist.md` so its required-status-check list
  matches current CI (adds Windows tests, Docs, and workflow lint).

## Exit Criteria

- No docs or scripts point to private or retired distribution channels.
- Public release checks are documented and repeatable.
- Sanitization sweeps stay green before release cuts.
