# ADRs

Architecture Decision Records capture durable decisions that should outlive a
single roadmap cycle.

Use an ADR when the repo needs an explicit answer to a design question that
would be expensive to rediscover later.

## When To Use An ADR

- repo architecture
- command surface boundaries
- data model or storage decisions
- release or compatibility policy
- long-lived tooling conventions

## ADR Format

Keep each ADR short and concrete:

- context
- decision
- alternatives considered
- consequences
- status
- date

## Maintenance Rule

An accepted ADR should not be rewritten into a different decision later.
If the repo changes direction, add a new ADR that supersedes the old one.

## Current ADRs

- [ADR 0001: Docs Lifecycle And File Roles](0001-docs-lifecycle.md)
