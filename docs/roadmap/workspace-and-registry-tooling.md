# Workspace And Registry Tooling

Status: active

## Current Scope

- `ayx tools` should own source/target workspace workflows for migrations and
  comparisons.
- Workspace generation and environment resolution need a clear home.
- The Mongo query registry should be structured so CLI commands, docs, and
  support flows can reuse it.

## Next Steps

- Finish the source/target workflows: `tools workspace compare`,
  `migrate-workflows`, and `check-dcm-connections` still return explicit
  scaffolds rather than completed cross-environment logic. Add
  validation-example coverage for them.

## Exit Criteria

- Workspace-aware automation can resolve environments deterministically.
- Query templates are reusable, safe by default, and easy to inspect.
- Migration helpers are first-class CLI features rather than ad hoc scripts.
