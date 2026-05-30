# `ayx dashboard` Notes

Implementation notes for the local `ayx dashboard` surface.

The dashboard is a read-only operational web UI served from the `ayx` binary. It is intended for local use by operators who want a quick browser view over telemetry without introducing a separate web service.

## Current Scope

- overview, jobs, and workflows pages are shipped
- HTML is server-rendered
- static assets are embedded in the binary
- the dashboard binds loopback by default
- non-loopback binding requires `--allow-remote`

## Command Shape

```text
ayx dashboard [--profile <name>] [--bind 127.0.0.1] [--port 8765]
              [--source one|server|auto] [--poll 10]
              [--no-open] [--allow-remote] [--auth-password <value>]
```

## Important Constraints

- The dashboard binds loopback by default.
- Non-loopback binding requires `--allow-remote`.
- Remote mode also requires HTTP Basic auth via `AYX_DASHBOARD_PASSWORD` or `--auth-password`.
- Profile-load failures are surfaced in-page instead of crashing the server.
- Telemetry panels reuse the existing CLI telemetry layer so the browser surface stays consistent with the CLI data model.

## Follow-Up Ideas

- add a heatmap panel for weekly activity
- add an errors page
- add a real source selector in the UI
- add a richer workflow drilldown route

## Code Map

```text
ayx-rs/src/cmd/dashboard/
├── mod.rs
├── server.rs
├── telemetry_bridge.rs
├── handlers/
├── views/
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
