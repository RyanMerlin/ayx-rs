# ADR 0001: Docs Lifecycle And File Roles

Status: accepted
Date: 2026-06-20

## Context

The repository had accumulated a mix of active planning notes, historical
handoff files, preview artifacts, fixtures, and legacy reference material in
the same area of the tree.

That made it unclear which files were supposed to be maintained, which were
temporary, and which were safe to delete.

## Decision

Use the following layout and keep it strict:

- `docs/roadmap/` for active work items
- `docs/adr/` for durable architecture decisions
- `docs/handoffs/` for temporary session notes that should not accumulate
- `docs/fixtures/` for documentation-adjacent sample artifacts used by docs or
  offline validation
- `docs/legacy/` is retired and should remain absent unless a future ADR
  explicitly brings it back

Temporary handoff files should be deleted or archived when the work is picked
up. They should not remain as evergreen docs.

Preview artifacts that are no longer part of an active design workflow should
be removed rather than kept indefinitely.

## Consequences

- New work has an obvious home.
- Durable decisions are explicit and searchable.
- Fixture files are separated from user-facing examples.
- Legacy material is not a default junk drawer.
