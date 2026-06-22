# Workspace Hardening

Status: active

## Current Scope

- Mutating workflows should always make workspace identity explicit.
- Stale tabs, stale cached state, and mismatched workspace contexts should fail
  closed.

## Next Steps

- Add preflight checks that resolve and record the active workspace.
- Treat the workspace model as part of the evidence bundle, not an implicit
  assumption.
- Keep orchestration logic responsible for flow control, but expose
  deterministic workspace validation primitives in `ayx-rs`.

## Exit Criteria

- Mutating commands validate workspace identity before doing work.
- Workspace drift is obvious in evidence and logs.
- Operators can tell which workspace a run touched without guessing.

