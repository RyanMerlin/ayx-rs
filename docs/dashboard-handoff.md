# Dashboard Rewrite Notes

Historical notes for the dashboard surface and its rewrite direction.

The dashboard was once implemented in Rust and launched from the `ayx` CLI.
That implementation has been removed. The next phase, if we revisit it, is to
rebuild the surface in a true web language and keep the CLI out of the
delivery path.

## Current Scope

- overview, jobs, and workflows pages are shipped
- HTML is server-rendered
- static assets were embedded in the implementation
- the dashboard binds loopback by default
- non-loopback binding requires `--allow-remote`

## Important Constraints

- The dashboard binds loopback by default.
- Non-loopback binding requires `--allow-remote`.
- Remote mode also requires HTTP Basic auth via `AYX_DASHBOARD_PASSWORD` or `--auth-password`.
- Profile-load failures are surfaced in-page instead of crashing the server.
- Telemetry panels currently reuse the existing Rust telemetry layer so the
  browser surface stays consistent with the CLI data model.

## Follow-Up Ideas

- add a heatmap panel for weekly activity
- add an errors page
- add a real source selector in the UI
- add a richer workflow drilldown route

## Code Map

```text
dashboard/
├── app/
├── server/
├── ui/
└── assets/
```

## Verification

Use the normal workspace checks:

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
```

For design iteration, `docs/dashboard-preview.html` remains the standalone preview artifact.
