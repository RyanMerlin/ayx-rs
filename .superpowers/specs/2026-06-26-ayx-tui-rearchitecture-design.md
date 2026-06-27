# ayx TUI Rearchitecture — Design Spec (post-v0.11.1)

## Problem

The `ayx` TUI (the "Visual interface", launched from `ayx one ui` / the `tui`
entrypoint) was recently pulled from the public docs nav because it "half works
and needs love." The honest diagnosis from a full-codebase review:

1. **Wrong center of gravity.** The TUI is architecturally a *profile/config
   manager* with a bolted-on read-only `one_browser` that dumps pretty-printed
   YAML. The product we want is a *resource browser*: switch workspace, browse
   assets (flows, jobs, connections, people), drill into detail, run actions.
2. **Monolithic, untestable structure.** `tui/app.rs` (2737 lines) conflates
   state types + event handling + disk/network effects; `tui/mod.rs` (1813
   lines) is rendering bound directly to `App` field layout. An in-code comment
   already flags this: *"Stage 3 of the audit roadmap will split this module."*
3. **Concrete correctness bugs.**
   - Drill-down (list → detail) bypasses the async worker and blocks the render
     thread on the API call — the terminal appears frozen (`app.rs:1821`).
   - Detail panels render raw YAML truncated at 18 lines with **no scroll**
     (`mod.rs:1457`).
   - Every `j`/`k` in the browser fires a live API call (`app.rs:1776`).
   - Text editing is append-only — no cursor movement, no mid-string edits.
   - No dirty-state guard: switching profile/workspace silently discards unsaved
     config edits.
4. **Shallow asset coverage.** The browser covers Workspaces / Flows /
   Connections only. **Jobs, Schedules, People, Roles are absent from the TUI**
   even though `ayx-one-api` has typed list+detail+pagination for all of them.

The backend is **not** the problem. `ayx-one-api` is the strongest part of the
codebase, the email-OTP auth win is solid and demo-ready, and the CLI surface
over every asset is comprehensive. **This is a TUI-layer rebuild, not backend
work.**

## Goal

A k9s-grade resource browser with VSCode-grade controls, on a clean
unidirectional architecture, reusing the ready backend and the auth win.

The canonical user story:

> Switch profile/workspace → browse all workspace assets (flows, connections,
> jobs, people) in fast lists with drill-down to detail → run actions
> (run a flow, cancel a job, enable/disable a connection).

## Scope

**In scope:** Full rearchitecture of the `tui/` layer. Resource browsing for
Flows, Jobs/runs, Connections, People (and Workspaces as the switcher source).
Multi-workspace context switching including inline login to an un-authed
workspace. A modern command-palette + contextual-footer control scheme.
Demotion of config/credential editing to a secondary screen.

**Out of scope:** Backend/API changes (the API layer is ready). Schedules
(enterprise-tier only, 404s on lower-tier workspaces — excluded to avoid demo
fragility; the `ResourceKind` registry makes adding it later trivial). Roles
list (no `GET /v4/authorization/roles` endpoint is wired). Mouse support.

**Decisions locked with the owner:**
- Ambition: **rearchitect to the k9s model**, dropping the current structure
  where it fights the vision. Quality-driven, no hard deadline.
- Workspace switching: **multi-workspace** is in scope (the headline feature).
- Asset priority: **Flows, Jobs/runs, Connections, People.** Schedules dropped.
- Controls: **reject the `:` modeline.** Use a VSCode/Linear-style command
  palette + always-on plain-language footer.
- Footer hint bar: **always-on** (owner deferred to recommendation).
- Phasing: **correctness-order** — browser solid before switching (owner
  deferred to recommendation).

## Architecture

### Data flow — The Elm Architecture (unidirectional)

```
Event ─▶ Action ─▶ update(&mut AppState) ─▶ [Effect] ─▶ worker ─▶ TaskResult ─▶ Action ─▶ …
                       (pure-ish reducer)              (async I/O thread)        (e.g. DataLoaded)
```

The render loop **never blocks on I/O**. That single rule eliminates the
drill-down freeze and the per-keystroke API storm. This is the model gh-dash
uses and ratatui's recommended component architecture.

### Module layout

Keep the already-clean modules (`worker.rs`, `forms.rs`, `render_helpers.rs`,
`theme.rs`, `store.rs`, and the `one_browser` dispatch logic). Rebuild the two
monoliths (`app.rs`, `mod.rs`) into focused components:

```
tui/
  mod.rs          slim: terminal setup + main loop (drain → draw → input)
  model/
    context.rs    Context: active profile · workspace · user identity (header data)
    nav.rs        NavStack(Vec<View>) + View enum  ← drill-down / back / breadcrumb
    state.rs      AppState root (Context + NavStack + per-view state + toast)
  action.rs       Action enum (user intents) + update() reducer
  effect.rs       Effect enum + executor (wraps the existing worker)
  resource/
    mod.rs        ResourceKind trait + registry   ← the k9s engine
    flow.rs  job.rs  connection.rs  person.rs  workspace.rs
  view/
    list.rs       generic table list + reactive detail panel (replaces YAML dump)
    detail.rs     scrollable detail (fixes 18-line truncation)
    switcher.rs   workspace/profile switcher (the multi-workspace UX)
    palette.rs    Ctrl+K fuzzy command palette
    filter.rs     / in-list filter
    help.rs       ? contextual help overlay
    footer.rs     always-on contextual hint bar
    config.rs     demoted config/credentials editor (reuses forms.rs)
  worker.rs  theme.rs   (kept; worker extended to a generic effect executor)
```

### The k9s engine — `ResourceKind` trait

The trait that makes adding an asset *data*, not a new screen:

```rust
trait ResourceKind {
    fn name(&self) -> &str;             // "flows"
    fn aliases(&self) -> &[&str];       // ["flow", "fl"]  → palette/jump matching
    fn columns(&self) -> &[Column];     // table headers + JSON accessors
    fn row(&self, item: &Value) -> Row; // → cells + status color
    fn list_effect(&self, ctx: &Context) -> Effect;
    fn detail_effect(&self, id: &str) -> Effect;
    fn actions(&self) -> &[ActionDef];  // run / cancel / enable-disable, per kind
    fn children(&self) -> &[ChildKind]; // flow→runs, job→flow (cross-asset drill)
}
```

Five impls (Flow, Job, Connection, Person, Workspace) + one registry. The list
view, detail view, palette router, and action footer all read from the trait —
written once, work for every asset. Coverage becomes additive.

### Effects & the worker

The existing `worker.rs` (clean, channel-based, `RequestId` stale-result
dropping) is extended from a fixed `BackgroundTask` set into a generic effect
executor: every `Effect` (FetchList, FetchDetail, RunFlow, CancelJob,
LoginWorkspace, SaveConfig, …) is dispatched to the worker thread and its result
folded back into the loop as an `Action`. The drill-down path is rerouted
through this executor — fixing the UI freeze.

## Control scheme (three layers)

Reject the `:` modeline. Serve beginners and experts from the same bindings via
three layers:

### Layer 1 — Always-on contextual footer (discoverability hero)

One row, always visible, **plain-language labels**, changes per focused view:

```
 Flows · 23                                          ⟳ loading…
─────────────────────────────────────────────────────────────────
 ↵ Open   r Run   e Enable/Disable   / Filter   ^K Palette   ? Help
```

Labels are spelled out (`↵ Open`, not `↵`). On a job-detail view the footer
becomes `c Cancel · l Logs · ↵ History · ⎋ Back`. A first-time user reads the
bottom row and knows what to do without speculative keypresses.

### Layer 2 — Command palette (universal accelerator + escape hatch)

A Cmd+K-style **fuzzy** palette. Unlike `:`, you type **plain language** and
everything is fuzzy-matched together — workspaces, resource types, items, and
actions — in one ranked, *categorized* list. No sigils required.

```
┌ ^K Command Palette ─────────────────────────────┐
│ run dai|                                          │
├───────────────────────────────────────────────────┤
│  ACTIONS                                           │
│  ▸ Run flow: daily-etl                      r     │  ← inline keybinding teaches the direct key
│    Run flow: daily-report                         │
│  RESOURCES                                         │
│    Browse Flows                             f     │
│  WORKSPACES                                        │
│    Switch to: production                          │
└───────────────────────────────────────────────────┘
```

- Open with nothing typed → pre-populated with recent actions + the 5 common
  entry points (Switch Workspace, Browse Flows/Jobs/Connections/People).
- Fuzzy ranking via `nucleo-matcher` (the Helix/Telescope matcher).
- The VSCode `>` prefix is an **optional** "commands-only" filter — never
  required (requiring it is what made `:` feel like masochism).

**Opener key: `Ctrl+K`.** The research's `Ctrl+P` recommendation is wrong *for
this environment*: zellij binds `Ctrl+P` to pane-mode and eats it before the TUI
sees it. `Ctrl+K` is free under zellij defaults and is the muscle-memory the
eng/product audience already has from Linear/Slack/Notion/GitHub.
Config-overridable. `?` opens contextual help.

### Layer 3 — Direct keys

Frequent actions get single keys (`r` run, `e` enable) — but every one is shown
in the footer and inline in the palette, so they're learned by seeing, never by
reading a manual.

### Implementation crates (all permissive — Apache-2.0-safe)

| Need | Crate | License | Notes |
|------|-------|---------|-------|
| Fuzzy matching | `nucleo-matcher` 0.3 | MIT | Helix's matcher; 2 small deps |
| Single-line input | `tui-input` 0.11 (+crossterm) | MIT | Proper cursor editing; kills append-only bug everywhere |
| Loading spinner | `throbber-widgets-tui` 0.11 | Zlib | Tracks ratatui ^0.30; 0 extra deps |
| Lists / tables / overlays | ratatui 0.30 built-ins | MIT | `List`/`Table` + state, `Clear`+rect for popups |
| Modal dialogs (optional) | `tui-popup` 0.4 | MIT/Apache | Only if repositionable modals are needed |

~10 transitive deps total. No GPL, no C FFI.

## Multi-workspace context switching

**Hard constraint** (from the API mapping): browsing a workspace's assets
requires being **authed into it** — credentials live per-workspace in
`AlteryxOneProfile.workspace_credentials` (`ayx-core/src/profile.rs`). The
switcher handles two states explicitly:

```
┌ Switch Workspace ───────────────────────────────┐
│ ● production            ready                     │  ← authed → instant switch
│ ● marketing-analytics   ready                     │
│ ○ staging               needs login               │  ← not authed → triggers OTP flow
│ ＋ Log in to a workspace…                          │  ← sentinel (gws-cli pattern)
└───────────────────────────────────────────────────┘
```

- **Authed workspace** → instant context switch (set `expected_workspace_id`);
  header updates; current list refetches async; toast "Switched to production".
- **Un-authed workspace / ＋ Log in** → inline **email-OTP flow**:
  suspend ratatui → run the existing `email_otp_login()` (prompts for the
  6-digit code) → resume + `drain_pending_events()` → store the new
  `WorkspaceCredential` → switch. **Reuses the auth win; no new auth code.**
- **Profile** (outer layer, `ayx profile use`) is also a palette command —
  each profile bundles its own workspace credentials.
- **Persistent header** always shows `Profile · Workspace · User` — the
  non-negotiable guard against acting in the wrong workspace.
- **Dirty-state guard**: if the config editor has unsaved edits, confirm before
  any switch (fixes the current silent-discard bug).

## Layout

```
┌ Profile: wyatt · Workspace: Marketing Analytics · ryan@alteryx.com ─────────┐  context header (always)
│ flows › "ETL Pipeline v3" › runs                                            │  breadcrumb = nav stack
├─────────────────────────────────────────────────┬──────────────────────────┤
│ NAME            STATUS    UPDATED      OWNER      │ ETL Pipeline v3          │  list (left)
│▸ETL Pipeline    ● ok      2026-06-20   ryan@      │ id: fl_abc…              │  + reactive detail (right,
│ Sales Rollup    ● failed  2026-06-19   dana@      │ last run: ● failed       │   updates on cursor move —
│ Nightly Sync    ◌ running 2026-06-21   ops@       │ schedule: daily 02:00    │   lazygit/gh-dash pattern)
├──────────────────────────────────────────────────┴──────────────────────────┤
│ ↵ open · r run · / filter · ^K palette · ? help · ⎋ back                     │  contextual action footer
└──────────────────────────────────────────────────────────────────────────────┘
```

- **Reactive detail panel** (split list/detail): detail updates as the cursor
  moves; `Enter` is for drilling *deeper*. Costs horizontal space on narrow
  terminals — acceptable; collapses below a width threshold.
- **Status colors**: green = ok/succeeded, yellow = pending/running/disabled,
  red = failed/error — always paired with a status *word* (never color alone).

## Phasing (quality-ordered)

Six phases, each independently testable, ordered by dependency.

| Phase | Delivers | Proves |
|---|---|---|
| **0 — Foundations** | New module skeleton; `AppState`/`Action`/`Effect`/`update` reducer; worker → generic effect executor; context header + slim main loop; add deps. Wire **Flows list only** through the new spine. Existing tests stay green. | The architecture works end-to-end on one asset. |
| **1 — Browser core** | `ResourceKind` trait + registry; generic table list + reactive detail panel; scrollable detail (kills truncation); async list+detail via worker (kills the freeze); `/` filter; contextual footer. Flows fully done. | The k9s engine. |
| **2 — All assets** | `ResourceKind` impls for Connections, Jobs, People, Workspaces; cross-asset drill (flow→runs, job→flow); nav stack + breadcrumb; status colors. | Coverage is additive/data-driven. |
| **3 — Palette & discoverability** | `Ctrl+K` fuzzy palette (unified results); `?` help overlay; `tui-input` replaces append-only editing everywhere. | The ergonomics. |
| **4 — Multi-workspace** | Workspace switcher (ready vs needs-login); inline OTP login via suspend/resume; profile switching; dirty-state guards; toasts. | The headline capability. |
| **5 — Actions & polish** | Run flow / cancel job / enable-disable; confirmation prompts; empty-vs-error states; throbbers; final pass; re-add the Visual interface to docs. | Production-grade. |

## Testing strategy

- **Reducers are pure-ish and unit-testable**: `update(state, action)` asserts
  on resulting state + emitted effects without a terminal. This is the core win
  of the TEA split — the current monolith has a `new_without_worker` escape
  hatch precisely because it can't be tested otherwise.
- **`ResourceKind` impls tested in isolation**: given a JSON fixture, assert
  `columns`/`row`/`actions` output (fixtures already exist under `docs/fixtures`).
- **Worker/effect executor**: tested against `httpmock` (already a dep) for
  list/detail/run round-trips with stale-result dropping.
- **Render smoke tests**: ratatui `TestBackend` snapshot of each view at a fixed
  size, plus a narrow-terminal case for the collapsing detail panel.
- **Verification in the caller phase**: each phase ends with
  `cargo nextest run --workspace --locked` + `cargo clippy --workspace
  --all-targets -- -D warnings` + `cargo fmt --all --check` green, and a manual
  smoke of the live TUI against a real workspace.

## Risks & mitigations

- **Rewrite risk on a 4500-line surface.** Mitigate: keep the clean modules;
  rebuild only the two monoliths; Phase 0 lands the spine with one asset and
  existing tests green before broad change; each phase is independently
  shippable.
- **Per-workspace auth friction surprises users.** Mitigate: the switcher shows
  ready vs needs-login explicitly; login is inline, not a context-exit.
- **Suspend/resume terminal corruption** around the OTP browser/stdin flow.
  Mitigate: reuse gws-cli's proven sequence — restore terminal, run external,
  reinit ratatui, `drain_pending_events()`.
- **Narrow terminals** break the split panel. Mitigate: width threshold that
  collapses the detail panel into an Enter-to-open full view.
```
