# Roadmap

This directory is the active planning surface for AYX-RS.

Use it for durable work items that should survive a single agent session.
Keep each file focused on one theme, and trim completed bullets rather than
letting stale notes accumulate.

## Conventions

- Put active work in `docs/roadmap/`.
- Put durable architecture decisions in `docs/adr/`.
- Put temporary handoffs and scratch notes in `docs/handoffs/` or `/tmp`, not
  in the active roadmap files.
- Prefer one topic per file with a small set of clear sections:
  - status
  - current scope
  - next steps
  - exit criteria
- When a topic is finished, either delete the completed bullets or move the
  outcome into release notes, changelog entries, or an ADR.

## Active Topics

- [Public release hygiene](public-release-and-doc-hygiene.md)
- [Discovery substrate and command surface](discovery-and-catalog.md)
- [Workspace and environment tooling](workspace-and-registry-tooling.md)
- [Mongo registry and remediation](mongo-registry-and-remediation.md)
- [Runtime resolver and orchestration](runtime-and-orchestration.md)
- [API surface and observability](api-surface-and-observability.md)
- [Workspace hardening](workspace-hardening.md)
- [Command surface coverage and gaps](command-surface-coverage.md)

## Relationship To Other Docs

- `docs/one-roadmap.md` remains a focused checkpoint for the One surface.
- `docs/command-surface.md` and `docs/cli-spec.md` are generated or
  near-generated surface references, not roadmap notes.
- Historical handoff material should not be expanded in place. Archive it or
  replace it with a short ADR if the decision still matters.

