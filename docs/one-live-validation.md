# One Live Validation

This document tracks the live validation strategy for the wired Alteryx One surface.

## Coverage Model

- `validated_live`: a real request returned from the One API host and the response was asserted.
- `validated_shape`: request construction, dry-run behavior, or envelope formatting was asserted without a live mutation.
- `blocked_by_auth`: the environment could not acquire usable live credentials.
- `blocked_by_scope`: the endpoint exists, but the current workspace/role does not have permission to exercise it.

## Surface Inventory

Test the currently wired One families in the CLI and API layers:

- platform / auth / workspace / person / token / role
- plans
- flows
- connections
- job-group
- output-object
- webhook-flow-task
- write-setting
- scheduling
- billing
- doctor / inventory / status helpers

## Validation Criteria

- One representative live read or discovery call per family.
- One edge case per family when the API supports it.
- For list endpoints: verify pagination or empty-result handling where possible.
- For mutating endpoints: prefer dry-run or a reversible safe case before any real mutation.
- Every result must record the command, endpoint family, status bucket, and whether it was truly live.
- Current smoke coverage includes invalid-id failures for representative detail commands and pagination-boundary checks for the major list families.

## Pressure Test Level

Use the default "happy path + one edge" matrix:

- happy path: prove the live endpoint is reachable and returning an expected envelope
- edge path: exercise invalid id, empty page, pagination boundary, or permission failure

Escalate to broader matrices only for families that are known to be flaky or stateful.

## Live Validation Hygiene

- Use `cargo nextest run` for all repo and smoke validation going forward.
- Keep One-only live tests on a minimal profile that still satisfies the config model, but avoid mixing in unrelated Server storage assumptions when validating the One cloud API.
- If auth fails, classify it as an environment blocker first. Only treat the surface as broken after a confirmed live request reaches the One host and returns a backend error.

## Current Harness

The current smoke harness lives in `ayx-rs/tests/one_live_smoke.rs` and already:

- uses the live CLI binary
- short-circuits cleanly when auth acquisition is unavailable
- validates the most important read paths across the One surface
- reports the surface and operation names in the envelope assertions

## Follow-Up

As more endpoints are confirmed, add them to the live matrix and keep the coverage grouped by family so the report stays readable.
