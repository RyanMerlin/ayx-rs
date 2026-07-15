# Runtime Resolver And Orchestration

Status: active

## Current Scope

- Action registries should describe small playbooks.
- Higher-level workflows should compose actions, commands, validation, and
  rollback behavior.
- The runtime resolver should return the minimum context an agent needs for a
  task.

## Next Steps

- Add explicit workflow-level input/output schema fields to the registry
  (today only id/title/summary/safety/tags/sequence/success-criteria and
  inferred params exist).

## Exit Criteria

- Actions and workflows are machine-readable and lazily loaded.
- Validation and rollback steps live beside the flow they protect.
- The resolver can support end-to-end tasks without inventing a second model.
