# Runtime Resolver And Orchestration

Status: active

## Current Scope

- Tactical registries should describe small playbooks.
- Higher-level workflows should compose tactics, commands, validation, and
  rollback behavior.
- The runtime resolver should return the minimum context an agent needs for a
  task.

## Next Steps

- Add CLI helpers for listing, describing, and resolving tactics.
- Define workflow metadata for inputs, outputs, and command sequences.
- Emit structured evidence after each run so the agent can reason about
  outcomes without reloading everything.

## Exit Criteria

- Tactics and workflows are machine-readable and lazily loaded.
- Validation and rollback steps live beside the flow they protect.
- The resolver can support end-to-end tasks without inventing a second model.

