# TODO

This is the working plan for evolving the AYX CLI into a production-grade, agent-friendly toolset for the Alteryx ecosystem.

## 1. Command registry
- define a compact machine-readable schema for the existing `clap` tree (name, purpose, args, output shape, safety level, mutating vs read-only).
- expose the schema through a new subcommand such as `ayx catalog list` / `ayx catalog describe <command>` so tooling can query it (JSON + CLI-friendly summary).
- annotate the schema with tactical hints (prerequisites, typical sequence, rollback, idempotency tags) during codegen or via manual metadata.
- ensure the schema is discoverable without dumping the entire manual (for example, request only the branch the agent is working on).

## 1b. Mongo query registry
- move support queries into a structured registry file so the CLI can reuse them across `mongo query`, `mongo doctor`, docs, and future plugin surfaces.
- support parameterized templates for repeated cases such as queue inspection, orphan detection, app ownership, user email/domain migration, and results correlation.
- expose a helper mode that prints the exact `mongosh` command for copy/paste or confirmation-based execution.
- keep the query registry read-only by default and separate any mutation templates into a gated remediation registry.

## 2. Tactical registry
- create a compact format (YAML/JSON) for tactics that define small playbooks: trigger patterns, guardrails, execution hints, example commands, validation steps.
- add CLI helpers (`ayx tactics list`, `ayx tactics describe <tactic>`, `ayx tactics resolve --task "<text>"`) so the agent can lazily load the tactic that matches a high-level task.
- keep tactics scoped to command families and mark their safety so mutating flows stay gated.
- store audit and validation steps inside each tactic so workflows can verify success or roll back when needed.

## 3. Workflow / skill registry
- define higher-order workflows or skills that reference commands, tactics, and validation, for example `governance-go-live` and `backup-restore`.
- capture workflow metadata (inputs, outputs, required tactics, typical CLI sequence) so the agent can plan end-to-end tasks.
- expose workflow introspection (`ayx workflows list`, `ayx workflows explain <name>`) to the orchestration layer.

## 3b. Mongo remediation workflows
- define explicit remediation workflows for controlled Mongo changes such as bulk email domain migrations, orphan cleanup, and queue/result repair.
- require a dry-run preview, confirmation gate, and audit artifact before any destructive or bulk-updating Mongo action runs.
- keep these workflows separate from the read-only query registry so support diagnostics remain safe and discoverable.

## 4. Runtime resolver and injection
- build the resolver service that, given the current task or command, returns the minimal command/tactic/workflow context an agent needs.
- integrate execution history so the resolver can decide when to re-fire a tactic versus reuse prior context.
- emit structured evidence (plan / execute / verify / rollback steps) after each run so the agent can reason about outcomes without reloading every detail.

## 5. Documentation and examples
- keep the README and `docs/cli-spec.md` aligned with the actual command tree and the agent-oriented architecture.
- add a short walkthrough that shows how an agent would query the catalog, tactics, and workflows before executing a workflow.
- keep the public getting-started path short: install, configure, validate, then execute.

## 6. Server auth
- continue expanding `ayx server auth` with SAML-first diagnosis and simulation.
- add targeted helpers for certificate validation, callback/redirect checks, and legacy AD only where it still matters operationally.
- keep the auth surface focused on evidence, simulation, and guided diagnosis rather than embedding every IdP-specific KBA procedure.

## 7. Product-scoped API branches
- keep the public CLI product-first instead of reintroducing a generic top-level `api`.
- keep Licensing as its own product branch, but prioritize Alteryx One as the next major platform branch.
- keep API-specific command trees under their product roots (`server`, `license`, `one`) while shared HTTP/auth helpers remain internal.
- extend the Codex plugin layer as the place for API KBA playbooks and multi-step orchestration.

## 8. One API hardening
- add a shared One transport helper with retries, exponential backoff, jitter, and `Retry-After` handling.
- normalize One API failures into structured envelopes with request ids, status codes, endpoint metadata, and parsed response bodies.
- add structured logging for One calls so operators can debug auth, rate limiting, and transient failures without leaking secrets.
- add ID-discovery helpers and a `one doctor` surface so workspace, plan, schedule, and billing checks can be run as a stable support suite.
- separate safe read-only paths from mutating actions and require stronger guardrails for bulk or destructive One workflows.

## 9. Shared API observability
- standardize a single JSONL API event log across Server, License, and One.
- make API request logging opt-in through `config.yaml` so operators can enable it only when needed.
- keep secrets, request bodies, and raw response bodies redacted by default.
- make Walter read the same event schema instead of inventing a second logging model.

## Current priority
- finish the integration-test pass for the core workflows.
- expand the command registry with richer command metadata and generated docs.
- keep the higher-order tactics and workflow registry in follow-up work until the command catalog is available.
- finish the Mongo query registry and doctor suite before adding any Mongo remediation/mutation workflows.
- keep Server auth focused on the SAML simulation and diagnosis primitives the KBAs actually support.
- flesh out `one platform` workflows first, then continue expanding License where the KBA set justifies it.
- harden One API execution before adding more mutating One workflows or bulk operations.
