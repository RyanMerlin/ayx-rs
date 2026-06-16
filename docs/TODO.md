# TODO

This is the working plan for evolving AYX-RS into a production-grade, agent-friendly toolset for the Alteryx ecosystem.

Completed items are removed here rather than left to rot in the plan.

## 1. Public release hygiene
- complete: the `ayx-rs` public release cutover is done and GitHub `RyanMerlin/ayx-rs` is the public source of truth.
- complete: public fixtures and generated artifacts are being kept sanitized so they do not leak real environment state.
- complete: the release workflow for building, packaging, and publishing the public binary is documented and wired.

## 1a. Central profile and doctor hardening
- move remaining inline-secret guidance toward environment-variable or native keychain storage instead of YAML.
- expand `ayx doctor --fix` from local state/bootstrap repair into deeper safe remediation where the action is deterministic and auditable.
- add profile/workspace export and import flows that keep shareable configuration separate from machine-local secrets.
- make the TUI reuse the same profile resolver, doctor checks, and active-profile state for central profiles while keeping explicit file editing confined to onboarding/editor flows instead of inventing a second runtime config model.

## 1b. Discovery substrate completion
- keep `ayx discover` as the primary live-tree entry point and grow it toward capability/tactic/workflow drill-down.
- make the command → capability → tactic → workflow ladder complete and first-class.
- expose stable public discovery surfaces for the capability registry once the contract is settled, instead of advertising internal execution paths too early.
- keep `ayx catalog`, `ayx doctor`, the One inventory, and the generated command surface synchronized so humans and agents always see the same surface truth.

## 1c. Discovery shape for v0.9.8
- keep `ayx discover` as the only live-tree entry point humans and agents should reach for first.
- add path drill-down for capability, tactic, and workflow leaves only when those surfaces are actually wired and tested.
- keep `catalog` as a supporting registry index until discovery exposes the same stable concepts directly.
- if `catalog` ever becomes redundant, deprecate it with a compatibility window or alias rather than removing it abruptly.
- do not add a second federated or multi-binary discovery model; `ayx` should remain a single canonical command surface.

## 2. Command registry
- keep the machine-readable command catalog aligned with the live `clap` tree.
- extend the catalog with richer command metadata for safety, mutating vs read-only behavior, and agent-friendly discovery.
- keep the catalog discoverable by branch so tooling can load only the surface it needs.

## 3. Workspace and environment tooling
- finish `ayx tools` cross-environment commands so source/target workflows become first-class for migrations and comparisons.
- add concrete `tools` subcommands for workflow migration, DCM connection comparison, and environment validation.
- add tests and examples for `environments.yaml` resolution with multiple environments and explicit active-environment overrides.
- decide whether workspace generation should live only in `onboard` or also in a dedicated `workspace init` path for automation.

## 4. Mongo query registry
- move support queries into a structured registry file so the CLI can reuse them across `mongo query`, `mongo doctor`, docs, and future plugin surfaces.
- support parameterized templates for repeated cases such as queue inspection, orphan detection, app ownership, user email/domain migration, and results correlation.
- expose a helper mode that prints the exact `mongosh` command for copy/paste or confirmation-based execution.
- keep the query registry read-only by default and separate any mutation templates into a gated remediation registry.

## 5. Tactical registry
- create a compact format (YAML/JSON) for tactics that define small playbooks: trigger patterns, guardrails, execution hints, example commands, validation steps.
- add CLI helpers (`ayx tactics list`, `ayx tactics describe <tactic>`, `ayx tactics resolve --task "<text>"`) so the agent can lazily load the tactic that matches a high-level task.
- keep tactics scoped to command families and mark their safety so mutating flows stay gated.
- store audit and validation steps inside each tactic so workflows can verify success or roll back when needed.

## 6. Workflow / skill registry
- define higher-order workflows or skills that reference commands, tactics, and validation, for example `governance-go-live` and `backup-restore`.
- capture workflow metadata (inputs, outputs, required tactics, typical CLI sequence) so the agent can plan end-to-end tasks.
- expose workflow introspection (`ayx workflows list`, `ayx workflows explain <name>`) to the orchestration layer.

## 7. Mongo remediation workflows
- define explicit remediation workflows for controlled Mongo changes such as bulk email domain migrations, orphan cleanup, and queue/result repair.
- require a dry-run preview, confirmation gate, and audit artifact before any destructive or bulk-updating Mongo action runs.
- keep these workflows separate from the read-only query registry so support diagnostics remain safe and discoverable.

## 8. Runtime resolver and injection
- build the resolver service that, given the current task or command, returns the minimal command/tactic/workflow context an agent needs.
- integrate execution history so the resolver can decide when to re-fire a tactic versus reuse prior context.
- emit structured evidence (plan / execute / verify / rollback steps) after each run so the agent can reason about outcomes without reloading every detail.

## 9. Server auth
- continue expanding `ayx server auth` with SAML-first diagnosis and simulation.
- add targeted helpers for certificate validation, callback/redirect checks, and legacy AD only where it still matters operationally.
- keep the auth surface focused on evidence, simulation, and guided diagnosis rather than embedding every IdP-specific support procedure.

## 10. Product-scoped API branches
- keep the public CLI product-first instead of reintroducing a generic top-level `api`.
- keep Licensing as its own product branch, but prioritize Alteryx One as the next major platform branch.
- keep API-specific command trees under their product roots (`server`, `license`, `one`) while shared HTTP/auth helpers remain internal.
- extend the Codex plugin layer as the place for API playbooks and multi-step orchestration.

## 11. One API hardening
- add a shared One transport helper with retries, exponential backoff, jitter, and `Retry-After` handling.
- normalize One API failures into structured envelopes with request ids, status codes, endpoint metadata, and parsed response bodies.
- add structured logging for One calls so operators can debug auth, rate limiting, and transient failures without leaking secrets.
- add ID-discovery helpers and a `one doctor` surface so workspace, plan, schedule, and billing checks can be run as a stable support suite.
- separate safe read-only paths from mutating actions and require stronger guardrails for bulk or destructive One workflows.

Status:
- split the API layer into product-specific crates: `ayx-server-api` for Server V3 and `ayx-one-api` for One.
- moved One transport out of `ayx-rs` and into `ayx-one-api`; existing One live calls now reuse the shared helper there.
- added a machine-readable One surface inventory in `ayx-one-api` with `implemented`, `partial`, and `documented_only` buckets.
- wired live One commands for managed IAM workspace/role, connections and connector metadata current/defaults/overrides, flow lifecycle/package handling, job groups and job publishing, output objects, webhook flow tasks, write settings, plan, plans, scheduling, billing, current user, people, apiAccessTokens, and workspace listing/configuration/transfer.
- flattened the `ayx-rs` One dispatcher into focused modules so platform, job groups, output objects, webhook flow tasks, write settings, scheduling, billing, UI, auto-insights, and desktop-exec each route through a dedicated handler.
- the inventory now treats `misc` as implemented and the remaining surface gaps live in the partial buckets rather than documented-only.
- retired the placeholder `one platform` branches for `group`, `sso`, `audit`, `session`, `oauth-client`, `env-param`, `pdh`, `app`, and `health`; `user`, `person`, `token`, and workspace-list/configuration flows remain wired.

## 12. Shared API observability
- standardize a single JSONL API event log across Server, License, and One.
- make API request logging opt-in through `config.yaml` so operators can enable it only when needed.
- keep secrets, request bodies, and raw response bodies redacted by default.
- make the orchestration layer read the same event schema instead of inventing a second logging model.

## 13. Workspace hardening
- make workspace identity explicit at the start of every workflow that mutates state.
- fail closed when the requested workspace does not match the active browser or CLI context.
- add a preflight check that resolves the current workspace and records it in evidence bundles.
- treat stale workflow tabs and stale cached browser state as suspect until the workspace is validated.
- keep the workflow guidance layer responsible for orchestration, but make `ayx-rs` expose the deterministic workspace validation and workflow-open primitives.

## Current priority
- finish the progressive discovery substrate so agents can move from command discovery to capability/tactic/workflow discovery without guessing.
- harden the new central profile system with better secret storage and profile import/export ergonomics.
- finish `ayx tools` source/target workflows for workspace-aware migration planning and comparison.
- keep the command catalog aligned with the live CLI after the dispatcher split and version bump.
- keep the generated docs and start-here examples aligned whenever the One surface changes.
- finish the Mongo query registry and doctor suite before adding any Mongo remediation/mutation workflows.
- keep Server auth focused on the SAML simulation and diagnosis primitives the documented support cases actually support.
- continue hardening One transport and the live smoke coverage before adding more bulk or destructive workflows.
- keep License expansion gated on the documented support case set.
- track the live One smoke checkpoint and next-phase sequencing in `docs/one-roadmap.md`.

## Next phase
- add native keychain-backed secret storage for interactive operator use while keeping environment variables first-class for automation.
- add TUI surfaces on top of the new central profile resolver and top-level doctor system.
- keep the live One inventory and docs synchronized as transport hardening continues.
- document any newly wired One branch in the inventory, catalog tests, and README start-here examples before moving on.
