# `ayx dashboard` — Handoff

Checkpoint for the next session. v1 of the local web dashboard shipped behind the `ayx dashboard` subcommand. This doc covers what's done, what's deferred, what to verify, and how to pick up.

## Status

**v1 landed.** Overview + Jobs + Workflows pages, server-rendered HTML + htmx polling, single binary (htmx + CSS embedded via `rust-embed`). All workspace tests pass; `cargo fmt --check` and `cargo clippy --workspace --all-targets -- -D warnings` are clean.

## What it does

```
ayx dashboard [--profile config.yaml] [--bind 127.0.0.1] [--port 8765]
              [--source one|server|auto] [--poll 10]
              [--no-open] [--allow-remote]
```

- Binds loopback by default; non-loopback bind requires `--allow-remote` (the dashboard has no auth — Alteryx tokens live in process memory).
- Auto-opens the browser unless `--no-open`.
- Profile-load failure at startup is non-fatal: chrome + `/healthz` + static assets render regardless; per-panel handlers surface telemetry errors as in-page cards.

## Routes

| Route | Kind | Notes |
|---|---|---|
| `GET /` | Full page | Overview summary cards + running-jobs + top-workflows panels |
| `GET /jobs` | Full page | Tabs: Running / History / Top |
| `GET /jobs/running` | htmx partial | `hx-trigger="load, every Ns"` |
| `GET /jobs/history` | htmx partial | Default `since=7d` |
| `GET /jobs/top` | htmx partial | Top-N by duration |
| `GET /workflows` | Full page | Tabs: Top / Performance / Errors |
| `GET /workflows/top` | htmx partial | Sort by `runs` |
| `GET /workflows/performance` | htmx partial | p50/p95/p99/max |
| `GET /workflows/errors` | htmx partial | Recent failures w/ owner |
| `GET /workflows/:id` | Full page | Drilldown — currently filters `performance` items client-side; v2 should call a dedicated endpoint |
| `GET /healthz` | Plain text | `ok` |
| `GET /static/*` | rust-embed | `htmx.min.js` (50 KB, v2.0.4), `app.css`, `favicon.svg` |

Query params on every panel route mirror CLI flags: `source`, `since`, `top`, `all`, `max_pages`.

## Code map

```
ayx-rs/src/cmd/dashboard/
├── mod.rs                       # clap surface, tokio runtime bootstrap, parse_bind, open_browser
├── server.rs                    # Router, AppState, Embed<Assets>, healthz, static_handler, tests
├── telemetry_bridge.rs          # PanelQuery, build_args, run_blocking / run_envelope
├── handlers/
│   ├── mod.rs                   # html() helper, err_card() for partials
│   ├── overview.rs              # GET /
│   ├── jobs.rs                  # GET /jobs + 3 partials
│   └── workflows.rs             # GET /workflows + 3 partials + :id drilldown
├── views/
│   ├── mod.rs                   # layout(), error_card(), s/s_at helpers
│   ├── overview.rs              # summary cards + embedded htmx panels
│   ├── jobs.rs                  # running_table, history_table, top_table, status_class
│   └── workflows.rs             # top_table, performance_table, errors_table, drilldown
└── assets/
    ├── htmx.min.js              # vendored htmx 2.0.4 (50,917 bytes)
    ├── app.css                  # ~150-line dark theme, no framework
    └── favicon.svg              # 32×32 mark
```

### Wire-up
- `ayx-rs/Cargo.toml` — added `axum 0.7`, `tokio` (rt-multi-thread, macros, signal, net), `tower 0.5`, `tower-http 0.6` (trace, compression-gzip), `maud 0.26`, `rust-embed 8`, `mime_guess 2`. `reqwest` moved to dev-dependencies too for the async client used in unit tests.
- `ayx-rs/src/cmd/mod.rs:14` — `pub mod dashboard;`
- `ayx-rs/src/main.rs:217` — `Command::Dashboard(cmd::dashboard::DashboardCommand)` variant
- `ayx-rs/src/main.rs:4368` — dispatch arm calling `cmd::dashboard::execute(...)`

### Reused (do not refactor)
- `ayx_core::profile::Config` / `load_profile_with_env` (main.rs:5127) — startup probe
- `ayx_core::envelope::Envelope` — every handler unwraps `envelope.data: serde_json::Value`
- `cmd::telemetry::{summary, jobs, workflows}` — called unchanged via `tokio::task::spawn_blocking`
- `cmd::telemetry::source::pick` — re-runs per request inside the blocking telemetry funcs
- `cmd::telemetry::TelemetryArgs` — built per request by `telemetry_bridge::build_args`

The CLI text/json/yaml render path (`render::render_text`) is **not** reused; dashboard goes straight to maud.

## Tests

Live under `cmd::dashboard::server::tests`:
- `healthz_returns_ok` — bare async fn check
- `router_serves_static_assets_and_healthz` — binds 127.0.0.1:0, exercises `/healthz`, `/static/htmx.min.js`, `/static/app.css`, and a 404

Telemetry-dependent routes are not unit-tested (would require a live profile). Manual smoke is the contract there.

```
cargo test --workspace
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

All green on commit 512d156 + uncommitted dashboard changes.

## Manual verification done

- `cargo build -p ayx-rs` clean
- `ayx dashboard --profile config.yaml --port 8765 --no-open` — server starts, prints listen URL on stderr
- With the local `config.yaml` (which is missing One `client_id`/`client_secret`) the startup warns but continues; `/healthz` returns `ok`, all pages return 200, telemetry panels show an in-page error card
- Layout chrome (header / tabs / source pill / footer) renders on every page including error states
- htmx.min.js served with correct content-type
- Graceful shutdown on Ctrl-C (tokio signal handler)

## Known limitations / open items

1. **Source toggle is read-only.** The "source: auto" pill displays the startup default but has no UI control to change it per session — users must pass `?source=one` in the URL or change `--source` at launch. Phase 2: turn it into a real `<select>` that sets a cookie via `hx-post /partials/source-pill` (route stubbed in plan, not yet wired).
2. **Workflow drilldown is a thin filter.** `/workflows/:id` calls `workflows::performance` and client-side filters by `flow_id`. Phase 2: add a dedicated function returning per-workflow runs + perf + errors in one envelope.
3. **No heatmap / errors page / queue page / permissions page.** All deferred. `weekly::run_counts` is the data feed; an SVG heatmap via maud is the cheapest path (no JS dep).
4. **Profile is re-loaded on every request.** Each telemetry call goes through `load_profile_with_env` again. Acceptable for v1; if it ever shows up in latency, hoist the `Config` into `AppState` and add lower-level fns that take `&Config` directly.
5. **No auth even with `--allow-remote`.** Documented in `mod.rs`. If we ever ship this for real remote use, gate behind a generated token in the URL or basic auth.
6. **`--port 0` not exercised.** Ephemeral-port binding works (tests use it on the router directly), but the CLI's startup banner prints the configured port before bind. If we want `--port 0` to be useful, refactor `serve()` to print the bound `local_addr()` after `TcpListener::bind`.
7. **`reqwest` async client added to dev-deps only.** Tests use it; if any production code ever needs the async client (e.g., SSE), promote it to `[dependencies]`.

## Static Preview

For design iteration, use the standalone file at `docs/dashboard-preview.html`.
It is intentionally separate from the CLI binary so the production routes stay
clean while you iterate on layout in a browser.

## Next-session starting point

Pick from phase 2:
- **Heatmap on the overview page.** `weekly::run_counts` returns a 168-bucket matrix; render as a 24×7 SVG `<g>` of `<rect>` cells with opacity = count/max. Add a new `/heatmap` partial and embed on `/`.
- **Errors page.** Mirror the jobs page pattern; `errors::recent` returns the envelope directly.
- **Permissions explorer.** Subject ↔ resource matrix using the `permissions::summary` envelope.
- **Live source switcher.** Real `<select>` in `layout()` that re-renders panels via htmx `hx-include`.

If picking one, **heatmap** is the highest-value low-effort win and the data is already plumbed.

## Reference

- Plan file: `~/.claude/plans/phase-3-reflective-dragon.md` (the v1 plan — kept the codename, ignore the "phase-3" prefix; this is really Phase 4 of the telemetry storyline)
- Prior phases (commits): `ee9574a` (P1 telemetry One), `4ea846d` (P2 Server), `512d156` (P3 permissions)
- Dashboard module entry: `ayx-rs/src/cmd/dashboard/mod.rs`
- htmx docs (local): `ayx-rs/src/cmd/dashboard/assets/htmx.min.js` is htmx 2.0.4 — see https://htmx.org/docs/ for `hx-get`, `hx-trigger`, `hx-swap`
