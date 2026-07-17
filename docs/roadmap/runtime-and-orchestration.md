# Runtime Resolver And Orchestration

Status: active

## Current Scope

- Action registries should describe small playbooks.
- Higher-level workflows should compose actions, commands, validation, and
  rollback behavior.
- The runtime resolver should return the minimum context an agent needs for a
  task.

## Next Steps

- ~~Add explicit workflow-level input/output schema fields to the registry
  (today only id/title/summary/safety/tags/sequence/success-criteria and
  inferred params exist).~~ **Done.** Every action and workflow can declare a
  JSON-Schema-subset `input_schema`/`output_schema`, validated for grammar and
  cross-composition consistency at registry load time, enforced against the
  caller's params before any step runs, and checked against the finished run
  record on every plan or `--apply`. The agent-facing contract: `ayx actions
  describe <id>` and `ayx actions workflows explain <id>` return a validated,
  machine-readable schema (plus `input_schema_source`: `declared` or
  `inferred`) an agent can read before constructing `--param` values, and
  every `run` is checked against that same contract on the way in and the way
  out — a bad or unknown parameter fails immediately with a `validation`
  error_code, before any subprocess fires.

## Exit Criteria

- Actions and workflows are machine-readable and lazily loaded.
- Validation and rollback steps live beside the flow they protect.
- The resolver can support end-to-end tasks without inventing a second model.
