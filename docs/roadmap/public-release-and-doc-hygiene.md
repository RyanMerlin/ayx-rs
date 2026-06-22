# Public Release Hygiene

Status: active

## Current Scope

- `RyanMerlin/ayx-rs` is the public source of truth for code, releases, and
  issue tracking.
- Public fixtures and generated artifacts must stay sanitized.
- Release plumbing should continue to point install, update, and publish flows
  at the public GitHub repository.

## Next Steps

- Move any remaining inline-secret guidance toward environment variables or
  native keychain storage.
- Keep `config.yaml`, examples, workflows, and install scripts free of private
  references.
- Keep the public release checklist in sync with the release workflow.

## Exit Criteria

- No docs or scripts point to private or retired distribution channels.
- Public release checks are documented and repeatable.
- Sanitization sweeps stay green before release cuts.

