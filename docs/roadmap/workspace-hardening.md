# Workspace Hardening

Status: active

## Current Scope

- Mutating workflows should always make workspace identity explicit.
- Stale tabs, stale cached state, and mismatched workspace contexts should fail
  closed.

## Next Steps

- Record the successful resolved workspace into the mutation envelope and the
  shared API event log (the safety check is wired, but a successful preflight
  is not persisted as evidence). Add workspace fields to the API event log.

## Exit Criteria

- Mutating commands validate workspace identity before doing work.
- Workspace drift is obvious in evidence and logs.
- Operators can tell which workspace a run touched without guessing.
