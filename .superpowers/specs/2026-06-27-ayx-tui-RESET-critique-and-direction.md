# ayx TUI — RESET: Critique, New Direction, Parked

> **STATUS: PARKED 2026-06-27.** The v2 TUI rebuild (k9s asset-browser, Phases 0/1/3
> merged + Phase 2 on branch `feat/tui-v2-phase2-cross-asset-drill`, unmerged) is
> **paused, not abandoned.** Direction is being reset before any further code. This doc
> is the living capture so we don't lose the reasoning. **Append new issues here.**

## TL;DR of the reset

- The v2 rebuild bet the TUI's center of gravity on **read-only browsing of Alteryx One
  assets**. That bet is wrong. It's the least valuable, most CLI-redundant capability,
  and it's the first (empty) screen a user sees.
- **New intended purpose:** `ayx tui` should primarily be an **admin + agent tool** —
  value for **admin tasks, governance, access control ("who has access to what", view
  access), and agent-drivable operations.** Asset browsing is at best a minor tab, not
  the home screen.
- The legacy TUI (default `ayx tui`, no env var) is **untouched and still fully
  featured** (Profiles, Alteryx One auth/diagnose, Surface Inventory, Workspace Config,
  Connectivity, Server, Help). Nothing was lost; v2 is opt-in behind `AYX_TUI_V2=1`.
- **Do not write more v2 code** until the open questions below are answered and the
  codex Alteryx One admin/governance research (in flight) is reviewed.

## New direction (owner, 2026-06-27)

> "It should primarily be an admin and agent CLI tool. I would much rather it show value
> for admin tasks, governance, who has access to what, view access."

Implications to work through on resume:
- Reframe around **governance & access**: users, roles, permissions, workspace
  membership, sharing/entitlements, audit, "who can see/do what", view/usage access.
- **Agent-drivable**: the same operations must be clean for an AI agent to invoke, not
  just a human at a TUI. (Aria fleet is a first-class consumer.)
- TUI earns its place only where it beats the CLI: stateful multi-step flows (auth),
  live dashboards (running jobs), cross-referencing (access matrices), bulk actions.

---

## The issue list (living — keep appending)

### Product / scope failures

1. **Inverted value hierarchy.** Made read-only asset browsing the home screen — the
   least valuable capability and the most redundant with the CLI (`ayx one flows list`
   already does it). The first screen is the weakest feature.
2. **Dead-end UX.** Header shows `Workspace: (no workspace)` with **no affordance to set
   one**. Surfaces a broken state you can't act on. The "workspaces" tab is browse-only —
   you can look at workspaces but not select one. Auth isn't reachable. Not-set-up = a
   permanently empty, inert TUI.
3. **Empty states lie.** "connections · 0 — no matches" with no filter applied. Conflates
   not-authed / no-workspace / genuinely-zero into one meaningless word. Good empty states
   diagnose and offer the next action.
4. **"Logged in but broken" contradiction.** Header shows the user's email AND
   `(no workspace)` at once. The per-workspace-cred model is never surfaced; reads as a
   broken contradiction.
5. **Duplicates the CLI instead of complementing it.** Everything v2 does is a flag away
   on the CLI. A TUI must do what the CLI is bad at; v2 rebuilt the CLI's strength.
6. **k9s analogy is a category error.** k9s suits hundreds of live resource types +
   constant actions. This domain has ~5 nouns, modest counts, 0 actions. Heavyweight
   pattern, lightweight domain.
7. **Phasing front-loaded the wrong risk.** Built the low-risk/low-value spine first;
   deferred the risky/valuable parts (auth, workspace switching, mutating actions). Months
   of work demo as "an empty grid." Should lead with the riskiest valuable thing.
8. **Rewrite threw away working knowledge.** Legacy "half worked" with 3 concrete bugs
   (drill freeze, no scroll, per-keystroke API spam). Response was a ground-up rebuild
   instead of fixing those 3 bugs. Second-system trap; now 4 phases in with *less*
   function than the start.
9. **Two TUIs behind an undocumented env var.** `ayx tui` (real) vs `AYX_TUI_V2=1`
   (empty). Divergent UIs, double maintenance, the good one hidden, no in-UI signal which
   you're in (the version-in-help add is a band-aid on this).

### UX / interaction failures

10. **1–5 tab order is arbitrary** (internal enum order, not task order). Owner wants:
    workspaces → flows → jobs → connections → people. Workspaces (the entry point) is
    currently buried at 5.
11. **Ctrl+K collides with VSCode** (chord leader in the integrated terminal). Worse, a
    fuzzy command palette is over-engineered for 5 static tabs and 0 actions — a palette
    is a power-user accelerator for a *large* surface.
12. **Split-pane "detail" is wasted space.** The right ~40% reformats the same columns
    already in the row. Reactive detail only pays off when detail is richer than the row.
13. **The `/` filter is visually unacceptable** (owner, 2026-06-27). A little text appears
    near the top of the list border; hard to see, no clear state, no obvious way to know
    you're filtering, interact, or escape. **RESEARCH TODO:** find how leading designs
    (Telescope/Helix, fzf, Slack/Linear/VSCode quick-open, k9s, lazygit) present in-list
    search/filter — a clearly-visible input affordance, live result feedback, obvious
    differentiation between filtered/unfiltered, and easy enter/escape. The whole UI needs
    this standard: **visually easy to see, differentiate, interact with, and escape from.**

### Engineering (what's actually fine)

- The TEA spine (pure reducer + async worker + `ResourceKind` registry + monotonic-token
  staleness) is **clean and well-tested** (456/456 workspace tests). The shell is
  reusable for whatever the TUI should be. The problem is the product thesis, not the
  implementation.

---

## Open questions to answer before resuming (do not skip)

1. **Who opens `ayx tui`, and to do what?** An admin doing auth/governance/access work,
   or someone browsing assets? (Owner: admin + agent.)
2. **What does the TUI do that the CLI can't?** That's the only thing worth building.
3. **Rebuild vs. iterate legacy?** Does an admin/governance TUI justify a fresh build, or
   is it faster to extend the legacy TUI (which already has auth/profiles/connectivity/
   server) toward governance?
4. **What admin/governance/access surface does Alteryx One actually expose** (API +
   permissions model), and how much of it does `ayx-one-api` already wrap? → codex
   research in flight (see companion doc
   `2026-06-27-alteryx-one-admin-governance-research.md`).

## Current state / pointers

- **Branch:** `feat/tui-v2-phase2-cross-asset-drill` — Phase 2 (cross-asset drill)
  complete + dual-reviewed, **not PR'd**. Leave parked.
- **v2 code:** `ayx-rs/src/tui/v2/` (gated `AYX_TUI_V2`). Legacy: `ayx-rs/src/tui/`.
- **Prior design spec (the bet being reset):**
  `.superpowers/specs/2026-06-26-ayx-tui-rearchitecture-design.md`.
- **Phase plans:** `.superpowers/plans/2026-06-2*-ayx-tui-phase*.md`.

## Resume checklist

1. Read the codex Alteryx One admin/governance research output.
2. Answer the 4 open questions with the owner.
3. Decide: reframe v2 around governance/access · iterate legacy · or fresh design.
4. Do the `/`-search (issue #13) UX research before any new input/search work.
5. Only then write code.
