# One-Chat Demo Protocol — Chunked for Recording

Source: [`one-chat-demo-protocol.md`](./one-chat-demo-protocol.md). That file is the single source of truth —
if the two ever disagree, the source protocol wins and this file should be re-split from it.

Each chunk below is a **standalone paste**: start a fresh Claude Code session (cwd inside `ayx-rs`, active `ayx`
profile authenticated), paste the whole chunk as your prompt, and record. Every chunk re-states the recording
contract and re-resolves any IDs it needs live, so chunks do not depend on session memory of each other — only
on the *state* left behind by prior chunks.

**Canaries persist across chunks on purpose.** The workflow copy (Chunk 4), the schedule (Chunk 5), the plan
(Chunk 6), and the invited member (Chunk 2) are all left live and visible in the Alteryx One UI — nothing
deletes or removes them as part of its own chunk. Chunk 7 is the single place all of them get reversed, and it
re-discovers every candidate live (by name convention / email) rather than trusting memory, so it works even
if it's pasted into a session that never saw the earlier chunks run.

Chunks 2, 4, 5, 6, and 7 contain mutations. Each has an explicit checkpoint — do not let the apply command run
without giving verbal/typed approval on camera first. Do not run Chunk 7 until every other chunk you intend to
record, plus any UI walkthrough of the live canaries, is finished.

---

## Chunk 1 — Setup & Workspace Governance Baseline

```text
This is a CLI-only recording for an Alteryx One governance demo. Follow this contract exactly:

- Every action uses the installed `ayx` CLI only. Never invoke a browser, Designer UI, Playwright,
  computer-use, a Designer URL, or browser-facing plugin — the terminal is the recording UI even if
  those capabilities exist.
- Every command uses `-o json`; show the entire redacted JSON envelope in the terminal. Never filter,
  collapse, redirect, hide, or replace command output.
- Narrate each envelope before and after running it:
  Before: "Phase <n>: <purpose>. We are using the CLI against the resolved workspace. The JSON envelope
  will show the target, status evidence, and result."
  After: "This is <classification>. The strongest evidence is <status/page envelope/status field>. It
  proves <fact>; it does not prove <unproven relationship>." `ok: true` alone is not HTTP success evidence.
  Classification vocabulary — use only: passed, dry_run_verified, blocked_by_scope, blocked_by_contract,
  not_applicable, expected_validation, or failed.
- Never print, request, paste, or store tokens, passwords, OTPs, cookies, or secrets.
- Use `ayx discover one --deep -o json` whenever the live command surface or a flag is uncertain. Never
  infer commands from APIs, old names, or memory.
- This chunk is read-only. Do not preview or apply any mutation in it.

Run, in order, narrating each:

    ayx --version
    ayx profile current -o json
    ayx one auth status -o json
    ayx one workspace current -o json
    ayx discover one --deep -o json

Show the profile, authentication freshness, base URL, workspace numeric ID, GID, and name. Stop if auth
is unhealthy, no workspace resolves, or the target differs from expectation.

Then run, in order, narrating each:

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

For every list, show `data.page_envelopes[].status_code` when present. Record counts and returned IDs
for members, administrators, groups, roles, and cloud providers. Explain the boundary on camera:
membership, administrator status, groups, IAM roles, configuration, and cloud backing are separate
surfaces — never claim effective access without proof.

Close this chunk by summarizing on camera: CLI version, auth posture, resolved workspace identity, and
the baseline counts (members/admins/groups/roles/cloud-configs) for comparison against the final chunk.
```

---

## Chunk 2 — Onboard the Test User + Governance Drill-down

```text
This is a CLI-only recording for an Alteryx One governance demo, continuing a prior baseline chunk.
Follow this contract exactly:

- Every action uses the installed `ayx` CLI only, `-o json` on every command, full redacted envelope
  shown, no browser/Designer UI/Playwright/computer-use ever.
- Narrate each command with the same before/after pattern and classification vocabulary as before
  (passed, dry_run_verified, blocked_by_scope, blocked_by_contract, not_applicable, expected_validation,
  failed). `ok: true` alone is not HTTP success evidence.
- Preview every mutation, show the preview, and apply only after explicit operator approval for the
  exact workspace, resource, and plan. Never print/request/store secrets.
- Test invite email: `ryanmerlin@gmail.com`. Target is the current authenticated Alteryx One workspace —
  resolve and display it before the mutation.

Step 1 — find the user:

    ayx one workspace members list -o json

Look for `ryanmerlin@gmail.com` in the returned envelope.
- If present: show the person record and mark this step `not_applicable`. Never duplicate an invite.
  Skip to Step 4 using the existing person's ID.
- If absent: continue to Step 2.

Step 2 — preview the invite:

    ayx one workspace members invite --email ryanmerlin@gmail.com -o json

Keep the dry-run envelope visible. It must name the workspace endpoint and the exact email. Then stop
and ask on camera, verbatim:

    "The next CLI command will send an invitation to ryanmerlin@gmail.com and add that identity to
    workspace <resolved workspace name/id>. Apply it?"

Wait for explicit operator approval before continuing.

Step 3 — after approval, apply:

    ayx one workspace members invite --email ryanmerlin@gmail.com --apply --yes -o json
    ayx one workspace members list -o json

Resolve the returned person ID from the list. Re-check once only if provider consistency requires it.

Step 4 — show (but never open) the invitation link lookup, and redact the actual link value on camera:

    ayx one workspace members invitation-link --person-id <PERSON_ID> -o json

Do not remove this member now. It stays a workspace member for the rest of the recording session so
it can be shown in the Alteryx One UI — removal happens only in Chunk 7's full demo teardown, after
the operator signals the whole demo is complete.

Step 5 — governance drill-down on the new/found person:

    ayx one person detail <PERSON_ID> -o json
    ayx one workspace members list -o json
    ayx one workspace members admins -o json
    ayx one role list -o json
    ayx one role detail <ROLE_ID> -o json
    ayx one role list-assignments <ROLE_ID> -o json

Resolve `<ROLE_ID>` only from current command output — never invent it. A 403 on the assignment listing
is `blocked_by_scope`: show it, explain it on camera, and do not retry. Explicitly note that this base
protocol audits roles only; role *assignment* is a separate, explicitly-approved governance-change
scenario not covered here.
```

---

## Chunk 3 — Workspace Backing & Connection Estate Audit

```text
This is a CLI-only recording for an Alteryx One governance demo. Follow this contract exactly:

- Every action uses the installed `ayx` CLI only, `-o json` on every command, full redacted envelope
  shown, no browser/Designer UI/Playwright/computer-use ever.
- Narrate each command with the before/after pattern and classification vocabulary (passed,
  dry_run_verified, blocked_by_scope, blocked_by_contract, not_applicable, expected_validation, failed).
- This entire chunk is read-only. Do not create, modify, share, rotate, or delete a connection, and do
  not reset or update workspace configuration or cloud configs.
- Preferred existing connection: `land-lease-intel-bq`. Any connection-bound table must be discovered
  live under the read-only `gold` schema — never invent it.

Part A — workspace backing and guardrails:

    ayx one workspace current -o json
    ayx one workspace detail <NUMERIC_WORKSPACE_ID> -o json
    ayx one workspace config get -o json
    ayx one workspace config schema -o json
    ayx one workspace cloud-configs list -o json

Resolve `<NUMERIC_WORKSPACE_ID>` from the `workspace current` envelope. Explain on camera which envelope
proves identity, which returns effective settings, and which identifies cloud backing/provider records.
Preserve and explain any unexpected authorization envelope, and stop workspace-scoped mutations if one
appears (there should be none in this chunk).

Part B — connection estate and access audit:

    ayx one connections list --all -o json
    ayx one connections detail <CONNECTION_ID> -o json
    ayx one connections status <CONNECTION_ID> -o json
    ayx one connections permissions list <CONNECTION_ID> -o json

Resolve `<CONNECTION_ID>` from the list — prefer `land-lease-intel-bq` only if its returned vendor/provider
is actually BigQuery; otherwise pick whatever the list returns and say so on camera. Explain that
redaction protects credentials, and that a `SUCCESS` status proves connection health, not a
workflow/table binding.
```

---

## Chunk 4 — Workflow Portfolio & Lifecycle Canary

```text
This is a CLI-only recording for an Alteryx One governance demo. Follow this contract exactly:

- Every action uses the installed `ayx` CLI only, `-o json` on every command, full redacted envelope
  shown, no browser/Designer UI/Playwright/computer-use ever.
- Narrate each command with the before/after pattern and classification vocabulary (passed,
  dry_run_verified, blocked_by_scope, blocked_by_contract, not_applicable, expected_validation, failed).
- Preview every mutation, show the preview, and apply only after explicit operator approval.
- Disposable seed workflow: `01KTZB2VPA38V4K87QJTGW25BB`. Expected seed workspace GID:
  `01KMGF85WTTEJZ397MW1RBD9ZB` — only use the seed if the resolved workspace GID exactly matches this
  and the seed detail lookup succeeds.
- Never execute or alter any existing workflow, and never delete anything not created by this chunk.

Part A — portfolio inventory:

    ayx one workflows list --all -o json
    ayx one workflows count -o json
    ayx one workflows assets --limit 25 -o json
    ayx one workflows tools -o json

Select a returned workflow and inspect it:

    ayx one workflows detail <WORKFLOW_ULID> --include-dependencies -o json
    ayx one workflows graph <WORKFLOW_ULID> -o json
    ayx one workflows dependencies <WORKFLOW_ULID> -o json
    ayx one workflows engines <WORKFLOW_ULID> -o json

Explain on camera that these are cloud execution assets, and that `ayx designer workflow` is a separate
local/on-prem surface not used here. If the resolved workspace GID matches `01KMGF85WTTEJZ397MW1RBD9ZB`,
also pull the seed's own detail and inspect its real dependencies — never assume them. If the seed does
not name the resolved BigQuery connection and a discovered `gold` table, say on camera that this is a
basic workflow-governance check, not a connection-binding test.

Part B — CLI-only workflow lifecycle canary. Generate a unique run ID (e.g. a short timestamp) and
preview:

    ayx one workflows copy 01KTZB2VPA38V4K87QJTGW25BB --name "ayx-rs demo <UNIQUE_RUN_ID>" -o json

Show the complete dry-run envelope. Then stop and get explicit operator approval, naming out loud: the
seed ID, the target workspace, the new name, and the plan to leave it live for a UI walkthrough before
Chunk 7 deletes it. Only after approval:

    ayx one workflows copy 01KTZB2VPA38V4K87QJTGW25BB --name "ayx-rs demo <UNIQUE_RUN_ID>" --apply --yes -o json
    ayx one workflows detail <NEW_WORKFLOW_ULID> --include-dependencies -o json
    ayx one workflows graph <NEW_WORKFLOW_ULID> -o json
    ayx one workflows dependencies <NEW_WORKFLOW_ULID> -o json
    ayx one workflows engines <NEW_WORKFLOW_ULID> -o json

Pause here on camera for recording. Do not run the copy workflow itself, and do not delete it now —
leave `<NEW_WORKFLOW_ULID>` in place so it can be shown in the Alteryx One UI. Note the ID; it gets
deleted, along with every other canary from this session, in Chunk 7's full demo teardown.

Part C — connection-bound workflow scenario (mention only, do not attempt unless the operator has a
file ready): this protocol has no CLI-verified authoring body for a brand-new connection-bound cloud
workflow. Only if the operator explicitly supplies a valid exported workflow JSON that visibly binds the
resolved connection ID and a discovered read-only `gold` table, preview only:

    ayx one workflows upload <WORKFLOW_JSON_FILE> -o json

Without that file, mark this `blocked_by_contract` on camera and move on. Never invent a body, create a
connection, expose credentials, or use a browser.
```

---

## Chunk 5 — Schedule Governance & Lifecycle Canary

```text
This is a CLI-only recording for an Alteryx One governance demo. Follow this contract exactly:

- Every action uses the installed `ayx` CLI only, `-o json` on every command, full redacted envelope
  shown, no browser/Designer UI/Playwright/computer-use ever.
- Narrate each command with the before/after pattern and classification vocabulary (passed,
  dry_run_verified, blocked_by_scope, blocked_by_contract, not_applicable, expected_validation, failed).
- Preview every mutation, show the preview, and apply only after explicit operator approval. Never
  delete a schedule by name, and never leave a demo schedule enabled at the end of this chunk.

Part A — schedule governance baseline:

    ayx one doctor scheduling -o json
    ayx one scheduling list --all -o json
    ayx one scheduling count -o json
    ayx one scheduling detail <SCHEDULE_ID> -o json

Resolve `<SCHEDULE_ID>` from the list output.

Part B — disposable schedule lifecycle canary. This needs a separately approved known-safe workflow ID
and a future time. Resolve a safe `<WORKFLOW_ULID>` live (e.g. from `ayx one workflows list --all -o json`
if not already known) and generate a unique run ID. Preview:

    ayx one scheduling create workflow <WORKFLOW_ULID> daily --name "ayx-rs demo <UNIQUE_RUN_ID>" --timezone America/Denver --hour 23 --minute 55 -o json

Show the typed dry-run body. Then stop and get explicit operator approval, naming out loud: the
workflow, the schedule name, the time, and the plan to disable it immediately and leave it live for a
UI walkthrough before Chunk 7 deletes it. Only after approval:

    ayx one scheduling create workflow <WORKFLOW_ULID> daily --name "ayx-rs demo <UNIQUE_RUN_ID>" --timezone America/Denver --hour 23 --minute 55 --apply --yes -o json
    ayx one scheduling detail <NEW_SCHEDULE_ID> -o json
    ayx one scheduling disable <NEW_SCHEDULE_ID> --apply --yes -o json
    ayx one scheduling detail <NEW_SCHEDULE_ID> -o json

Pause on camera on the disabled schedule. Do not delete it now — leave `<NEW_SCHEDULE_ID>` in place,
disabled, so it can be shown in the Alteryx One UI. Note the ID; it gets deleted, along with every
other canary from this session, in Chunk 7's full demo teardown. Disabling immediately is still
mandatory — that's what makes it safe to leave live through the rest of the recording.
```

---

## Chunk 6 — Sharing & Plans Audit

```text
This is a CLI-only recording for an Alteryx One governance demo. Follow this contract exactly:

- Every action uses the installed `ayx` CLI only, `-o json` on every command, full redacted envelope
  shown, no browser/Designer UI/Playwright/computer-use ever.
- Narrate each command with the before/after pattern and classification vocabulary (passed,
  dry_run_verified, blocked_by_scope, blocked_by_contract, not_applicable, expected_validation, failed).
- Preview every mutation, show the preview, and apply only after explicit operator approval.

Part A — sharing and permission review (preview-only, no apply in this section):

    ayx one connections permissions list <CONNECTION_ID> -o json
    ayx one plans permissions <PLAN_ID> -o json
    ayx one plans schedules <PLAN_ID> -o json
    ayx one workflows share <WORKFLOW_ULID> --to-person ryanmerlin@gmail.com --privilege read -o json

Explain the resolved recipient and the proposed share request on camera. Do not add `--apply`,
`--send-email`, or `--include-dependencies` — this protocol never applies a workflow share, because the
CLI has no paired revoke command.

Part B — plans and orchestration audit:

    ayx one doctor plans -o json
    ayx one plans list --all -o json
    ayx one plans count -o json
    ayx one plans detail <PLAN_ID> -o json
    ayx one plans full <PLAN_ID> -o json
    ayx one plans run-parameters <PLAN_ID> -o json
    ayx one plans schedules <PLAN_ID> -o json
    ayx one plans permissions <PLAN_ID> -o json

Never run an existing plan. A plan CRUD canary requires the provider-valid checked-in body
`ayx-rs/tests/fixtures/one-plan-canary.json`. If present and valid: preview every create/update, apply
only after separate operator approval, capture the created ID, show creation and update, then pause on
camera for recording. Do not delete it now — leave the created plan in place so it can be shown in the
Alteryx One UI. Note the ID; it gets deleted, along with every other canary from this session, in
Chunk 7's full demo teardown. If the fixture is absent or invalid, mark this `blocked_by_contract` on
camera and do not invent a plan body.
```

---

## Chunk 7 — Full Demo Teardown & Final Verification

```text
This is a CLI-only recording for an Alteryx One governance demo, closing out the whole session. Follow
this contract exactly:

- Every action uses the installed `ayx` CLI only, `-o json` on every command, full redacted envelope
  shown, no browser/Designer UI/Playwright/computer-use ever.
- Narrate each command with the before/after pattern and classification vocabulary (passed,
  dry_run_verified, blocked_by_scope, blocked_by_contract, not_applicable, expected_validation, failed).
- Preview every mutation, show the preview, and apply only after explicit operator approval.
- Only start this chunk after the operator gives this exact approval: "The demo is complete. Tear down
  every disposable resource created in this session." Do not run it before every other chunk you intend
  to record, plus any UI walkthrough of the live canaries, is finished.

Every canary from earlier chunks — the invited member (Chunk 2), the workflow copy (Chunk 4), the
schedule (Chunk 5), and the plan (Chunk 6) — has been left live so it could be shown in the Alteryx One
UI. This chunk reverses all of them together, then proves the workspace is back to Chunk 1's baseline.

Step 1 — sweep for everything created this session. Do not rely on memory alone for IDs, especially if
this chunk is pasted into a session that never saw the earlier chunks run — re-resolve every candidate
live:

    ayx one workflows list --all -o json
    ayx one scheduling list --all -o json
    ayx one plans list --all -o json
    ayx one workspace members list -o json

Identify: the workflow copy named `ayx-rs demo <UNIQUE_RUN_ID>`, the schedule named
`ayx-rs demo <UNIQUE_RUN_ID>`, the plan named `ayx-rs-codex-plan-canary-20260818` (from the checked-in
fixture, only if Chunk 6 created it), and the member matching `ryanmerlin@gmail.com`. Never delete or
remove anything else from these lists.

Step 2 — reverse in dependency order: plans and schedules can reference workflows, so clear them
first; the invited member is independent of everything else, so it's removed last, closing out the
onboarding narrative on camera.

    ayx one plans delete <PLAN_CANARY_ID> --apply --yes -o json
    ayx one plans detail <PLAN_CANARY_ID> -o json

    ayx one scheduling delete <NEW_SCHEDULE_ID> --apply --yes -o json
    ayx one scheduling detail <NEW_SCHEDULE_ID> -o json

    ayx one workflows delete <NEW_WORKFLOW_ULID> --apply --yes -o json
    ayx one workflows detail <NEW_WORKFLOW_ULID> -o json
    ayx one workflows detail 01KTZB2VPA38V4K87QJTGW25BB --include-dependencies -o json

    ayx one workspace members remove --person-id <PERSON_ID> --apply --yes -o json
    ayx one workspace members list -o json

Skip the plan step (mark `not_applicable`) if Chunk 6 never created one. Expect `not_found` or absence
for every captured canary, and continued success for the untouched seed workflow. Stop and flag on
camera if any single deletion fails — do not proceed to Step 3 with residue outstanding.

Step 3 — final visible verification:

    ayx one workspace current -o json
    ayx one workspace members list -o json
    ayx one workspace groups list -o json
    ayx one workspace cloud-configs list -o json
    ayx one workflows list --all -o json
    ayx one scheduling list --all -o json
    ayx one plans list --all -o json

On camera, summarize: CLI version and API-token posture; target workspace; baseline-versus-final counts
(these must now match Chunk 1's numbers, member count included); the onboarding result and person ID
and its removal; every audited governance surface across all chunks; every disposable resource ID
created in this session and its cleanup proof; and each scope, contract, tier, or consistency finding
hit along the way. Do not call the demo fully verified if a required list lacks HTTP/page-status
evidence, if any created disposable resource (including the invited member) has not been shown cleaned
up, or if baseline-versus-final counts do not match. Preserve raw redacted terminal envelopes and
artifact paths outside Git.
```
