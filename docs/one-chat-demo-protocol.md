# Alteryx One CLI Recording Demo Protocol

This is a **CLI-only** recording runbook for a realistic Alteryx One post-onboarding governance scenario. It is for a human audience watching a terminal recording, not a browser workflow or a post-cleanup report.

## Non-negotiable recording contract

- **Every action uses the installed `ayx` CLI only.**
- **Every command uses `-o json`; show its entire redacted JSON envelope in the terminal.** Never filter, collapse, redirect, hide, or replace command output during recording.
- Narrate each envelope: target, HTTP/page status, classification, what it proves, and what it does not prove. `ok: true` alone is not HTTP success evidence.
- Use the active OAuth API-token profile. Begin with `ayx one auth status -o json`. Never print, request, paste, or store tokens, passwords, OTPs, cookies, or secrets.
- **Never invoke a browser, Designer UI, Playwright, computer-use, a Designer URL, or browser-facing plugin.** The terminal is the recording UI, even if those capabilities exist.
- Use `ayx discover one --deep -o json` whenever the live command surface or a flag is uncertain. Never infer commands from APIs, old names, or memory.
- Run read-only commands freely. Preview every mutation, show the preview, and apply only after explicit operator approval for the exact workspace, resource, and cleanup plan.
- Never execute a workflow or plan; alter an existing connection; reset workspace configuration; assign an unknown role; or delete anything that was not created by this run.
- **Disposable canaries stay live for the whole session.** Do not delete or remove a canary (workflow copy, schedule, plan, invited member) right after the phase that created it — it needs to stay visible for a UI walkthrough. Every canary is reversed together in Phase 10, only after the operator explicitly signals the full demo is complete.

## Inputs

- Test invite email: `ryanmerlin@gmail.com`
- Target: current authenticated Alteryx One workspace; resolve and display it before every workspace-scoped mutation.
- Expected seed workspace GID: `01KMGF85WTTEJZ397MW1RBD9ZB`
- Disposable seed workflow: `01KTZB2VPA38V4K87QJTGW25BB`
- Preferred existing connection: `land-lease-intel-bq`
- Any connection-bound table must be discovered live under the read-only `gold` schema. Never invent it.

## On-camera narration pattern

Before each command:

> **Phase <n>: <purpose>.** We are using the CLI against the resolved workspace. The JSON envelope will show the target, status evidence, and result.

After each command:

> This is `<classification>`. The strongest evidence is `<status/page envelope/status field>`. It proves `<fact>`; it does not prove `<unproven relationship>`.

Use only: `passed`, `dry_run_verified`, `blocked_by_scope`, `blocked_by_contract`, `not_applicable`, `expected_validation`, or `failed`.

## Scenario

The operator has just onboarded a person and needs a complete, realistic audit of the Alteryx One footprint: users, administrators, groups, roles, workspace controls, cloud backing, connection health and access, workflow inventory and dependencies, schedules, sharing, and plans. Disposable canaries demonstrate lifecycle controls only after the audience sees the visible evidence, and every one of them — the invited member included — stays in place through the rest of the recording so it can be shown in the Alteryx One UI, not just the terminal. Phase 10 is the single point where all of them are reversed together.

## Phase 0 — establish CLI operator context

```text
ayx --version
ayx profile current -o json
ayx one auth status -o json
ayx one workspace current -o json
ayx discover one --deep -o json
```

Show the profile, authentication freshness, base URL, workspace numeric ID, GID, and name. Stop if auth is unhealthy, no workspace resolves, or the target differs.

## Phase 1 — workspace governance baseline

```text
ayx one workspace list --all -o json
ayx one workspace current -o json
ayx one workspace config get -o json
ayx one workspace config schema -o json
ayx one workspace members list -o json
ayx one workspace members admins -o json
ayx one workspace groups list -o json
ayx one workspace cloud-configs list -o json
ayx one person current -o json
ayx one person list --all -o json
ayx one role list -o json
```

For every list, show `data.page_envelopes[].status_code` when present. Record counts and returned IDs for members, administrators, groups, roles, and cloud providers. Explain the boundary: membership, administrator status, groups, IAM roles, configuration, and cloud backing are separate surfaces; never claim effective access without proof.

## Phase 2 — onboard the test user

1. Find `ryanmerlin@gmail.com` in the visible member envelope.
2. If present, show the person record and mark `not_applicable`; never duplicate an invite.
3. If absent, preview:

   ```text
   ayx one workspace members invite --email ryanmerlin@gmail.com -o json
   ```

4. Keep the dry-run envelope visible. It must name the workspace endpoint and exact email.
5. Stop with this exact checkpoint:

   > The next CLI command will send an invitation to ryanmerlin@gmail.com and add that identity to workspace <resolved workspace name/id>. Apply it?

6. After approval:

   ```text
   ayx one workspace members invite --email ryanmerlin@gmail.com --apply --yes -o json
   ayx one workspace members list -o json
   ```

7. Resolve the returned person ID. Re-check once only if provider consistency requires it.
8. Show link lookup but redact/omit the actual link and never open it:

   ```text
   ayx one workspace members invitation-link --person-id <PERSON_ID> -o json
   ```

9. Do not remove this member now. It stays a workspace member for the rest of the session so it can
   be shown in the Alteryx One UI; removal happens only in Phase 10's full demo cleanup.

## Phase 3 — new-user governance drill-down

```text
ayx one person detail <PERSON_ID> -o json
ayx one workspace members list -o json
ayx one workspace members admins -o json
ayx one role list -o json
ayx one role detail <ROLE_ID> -o json
ayx one role list-assignments <ROLE_ID> -o json
```

Resolve role IDs only from current output. A 403 assignment listing is `blocked_by_scope`: show it, explain it, and do not retry. The base protocol audits roles; role assignment is a distinct, explicitly approved governance-change scenario.

## Phase 4 — workspace backing and guardrails

```text
ayx one workspace current -o json
ayx one workspace detail <NUMERIC_WORKSPACE_ID> -o json
ayx one workspace config get -o json
ayx one workspace config schema -o json
ayx one workspace cloud-configs list -o json
```

Explain which envelope proves identity, which returns effective settings, and which identifies cloud backing/provider records. Do not reset configuration or create/update cloud configs. Preserve any unexpected authorization envelope and stop workspace-scoped mutations.

## Phase 5 — connection estate and access audit

```text
ayx one connections list --all -o json
ayx one connections detail <CONNECTION_ID> -o json
ayx one connections status <CONNECTION_ID> -o json
ayx one connections permissions list <CONNECTION_ID> -o json
```

Resolve `<CONNECTION_ID>` from the list. Prefer `land-lease-intel-bq` only if its returned vendor/provider is BigQuery. Explain that redaction protects credentials; a `SUCCESS` status proves health, not a workflow/table binding. Do not create, modify, share, rotate, or delete a connection.

## Phase 6 — workflow portfolio and dependency governance

```text
ayx one workflows list --all -o json
ayx one workflows count -o json
ayx one workflows assets --limit 25 -o json
ayx one workflows tools -o json
```

Select a returned workflow and inspect it:

```text
ayx one workflows detail <WORKFLOW_ULID> --include-dependencies -o json
ayx one workflows graph <WORKFLOW_ULID> -o json
ayx one workflows dependencies <WORKFLOW_ULID> -o json
ayx one workflows engines <WORKFLOW_ULID> -o json
```

Explain that these are cloud execution assets and `ayx designer workflow` is a separate local/on-prem surface. Do not run or alter the selected workflow.

Use the seed only when the workspace GID exactly matches `01KMGF85WTTEJZ397MW1RBD9ZB` and seed detail succeeds. Inspect dependencies; never assume them. If the seed does not name the resolved BigQuery connection and a discovered `gold` table, it is a basic workflow-governance test—not a connection-binding test.

### CLI-only workflow lifecycle canary

The supported creation path is a CLI copy of the seed. Preview:

```text
ayx one workflows copy 01KTZB2VPA38V4K87QJTGW25BB --name "ayx-rs demo <UNIQUE_RUN_ID>" -o json
```

Show its complete dry-run envelope. After approval naming the seed, target workspace, new name, and the plan to leave it live for a UI walkthrough before Phase 10 deletes it:

```text
ayx one workflows copy 01KTZB2VPA38V4K87QJTGW25BB --name "ayx-rs demo <UNIQUE_RUN_ID>" --apply --yes -o json
ayx one workflows detail <NEW_WORKFLOW_ULID> --include-dependencies -o json
ayx one workflows graph <NEW_WORKFLOW_ULID> -o json
ayx one workflows dependencies <NEW_WORKFLOW_ULID> -o json
ayx one workflows engines <NEW_WORKFLOW_ULID> -o json
```

Pause here for recording. Do not execute the copy, and do not delete it now — leave `<NEW_WORKFLOW_ULID>`
in place so it can be shown in the Alteryx One UI. Note the ID for Phase 10, where every canary created
in this session is deleted together, only after the operator signals the full demo is complete.

### Connection-bound workflow scenario

This protocol has no CLI-verified authoring body for a new connection-bound cloud workflow. If an explicitly supplied, valid exported workflow JSON visibly binds the resolved connection ID and a discovered read-only `gold` table, preview only:

```text
ayx one workflows upload <WORKFLOW_JSON_FILE> -o json
```

Without that file, mark `blocked_by_contract`. Never invent a body, create a connection, expose credentials, or use a browser.

## Phase 7 — schedule governance and controlled lifecycle

```text
ayx one doctor scheduling -o json
ayx one scheduling list --all -o json
ayx one scheduling count -o json
ayx one scheduling detail <SCHEDULE_ID> -o json
```

For a separately approved disposable schedule using a known-safe workflow and future time:

```text
ayx one scheduling create workflow <WORKFLOW_ULID> daily --name "ayx-rs demo <UNIQUE_RUN_ID>" --timezone America/Denver --hour 23 --minute 55 -o json
```

Show the typed dry-run body. After approval, create and show the lifecycle:

```text
ayx one scheduling create workflow <WORKFLOW_ULID> daily --name "ayx-rs demo <UNIQUE_RUN_ID>" --timezone America/Denver --hour 23 --minute 55 --apply --yes -o json
ayx one scheduling detail <NEW_SCHEDULE_ID> -o json
ayx one scheduling disable <NEW_SCHEDULE_ID> --apply --yes -o json
ayx one scheduling detail <NEW_SCHEDULE_ID> -o json
```

Pause on the disabled schedule. Do not delete it now — leave `<NEW_SCHEDULE_ID>` in place, disabled, so
it can be shown in the Alteryx One UI. Note the ID for Phase 10, where every canary created in this
session is deleted together, only after the operator signals the full demo is complete. Disabling
immediately (never leaving a demo schedule enabled) is still mandatory — that is what keeps a live
canary safe to leave sitting through the rest of the recording. Never delete by name.

## Phase 8 — sharing and permission review

```text
ayx one connections permissions list <CONNECTION_ID> -o json
ayx one plans permissions <PLAN_ID> -o json
ayx one plans schedules <PLAN_ID> -o json
ayx one workflows share <WORKFLOW_ULID> --to-person ryanmerlin@gmail.com --privilege read -o json
```

Explain the resolved recipient and proposed share request. Do not add `--apply`, `--send-email`, or `--include-dependencies`: this protocol does not apply a workflow share because the CLI has no paired revoke command.

## Phase 9 — plans and orchestration audit

```text
ayx one doctor plans -o json
ayx one plans list --all -o json
ayx one plans count -o json
ayx one plans detail <PLAN_ID> -o json
ayx one plans full <PLAN_ID> -o json
ayx one plans run-parameters <PLAN_ID> -o json
ayx one plans schedules <PLAN_ID> -o json
ayx one plans permissions <PLAN_ID> -o json
```

Never run an existing plan. A plan CRUD canary needs the provider-valid checked-in body `ayx-rs/tests/fixtures/one-plan-canary.json`. Preview every create/update, apply only after separate approval, capture the created ID, show creation and update, then pause for recording. Do not delete it now — leave the created plan in place so it can be shown in the Alteryx One UI. Note the ID for Phase 10, where every canary created in this session is deleted together, only after the operator signals the full demo is complete. If the fixture is absent or invalid, mark `blocked_by_contract`; never invent a plan body.

## Phase 10 — full demo cleanup and final verification

Only start this phase after the operator gives this exact checkpoint approval:

> **The demo is complete. Tear down every disposable resource created in this session.**

Every canary from Phases 2, 6, 7, and 9 has been left live up to this point so it could be shown in
the Alteryx One UI. This phase reverses all of them together, then proves the workspace is back to
its Phase 1 baseline.

### Step 1 — sweep for everything created this session

Do not rely on memory alone for IDs, especially if this phase runs in a separate session from the
ones that created the canaries. Re-resolve every candidate live:

```text
ayx one workflows list --all -o json
ayx one scheduling list --all -o json
ayx one plans list --all -o json
ayx one workspace members list -o json
```

Identify: the workflow copy named `ayx-rs demo <UNIQUE_RUN_ID>` (Phase 6), the schedule named
`ayx-rs demo <UNIQUE_RUN_ID>` (Phase 7), the plan named `ayx-rs-codex-plan-canary-20260818` from the
checked-in fixture (Phase 9, only if it was created), and the member matching
`ryanmerlin@gmail.com` (Phase 2). Never delete or remove anything else from these lists.

### Step 2 — reverse in dependency order

Plans and schedules can reference workflows, so clear them first; the invited member is independent
of every other resource, so it is removed last, closing out the onboarding narrative on camera.

```text
ayx one plans delete <PLAN_CANARY_ID> --apply --yes -o json
ayx one plans detail <PLAN_CANARY_ID> -o json

ayx one scheduling delete <NEW_SCHEDULE_ID> --apply --yes -o json
ayx one scheduling detail <NEW_SCHEDULE_ID> -o json

ayx one workflows delete <NEW_WORKFLOW_ULID> --apply --yes -o json
ayx one workflows detail <NEW_WORKFLOW_ULID> -o json
ayx one workflows detail 01KTZB2VPA38V4K87QJTGW25BB --include-dependencies -o json

ayx one workspace members remove --person-id <PERSON_ID> --apply --yes -o json
ayx one workspace members list -o json
```

Skip the plan step entirely (mark `not_applicable`) if Phase 9 was `blocked_by_contract` and no
canary was ever created. The expected result for each detail/list re-check is `not_found` or absence
for every captured canary, and continued success for the untouched seed workflow. Stop and flag on
camera if any single deletion fails — do not proceed to Step 3 with residue outstanding.

### Step 3 — final visible verification

```text
ayx one workspace current -o json
ayx one workspace members list -o json
ayx one workspace groups list -o json
ayx one workspace cloud-configs list -o json
ayx one workflows list --all -o json
ayx one scheduling list --all -o json
ayx one plans list --all -o json
```

On camera, summarize: CLI version and API-token posture; target workspace; baseline-versus-final counts (these must now match Phase 1, member count included); the onboarding result and person ID and its removal; audited governance surfaces; every disposable resource ID created across the whole session and its cleanup proof; and each scope, contract, tier, or consistency finding. Do not call the demo fully verified if a required list lacks HTTP/page-status evidence, if any created disposable resource (including the invited member) has not been shown cleaned up, or if baseline-versus-final counts do not match. Preserve raw redacted terminal envelopes and artifact paths outside Git.
