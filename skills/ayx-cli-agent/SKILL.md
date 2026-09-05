---
name: ayx-cli-agent
description: Use the AYX CLI as an agent for Alteryx One tasks; discover commands first, parse structured results safely, and execute explicitly authorized CRUD with canary cleanup.
---

# AYX CLI Agent

Use this skill whenever an agent needs to inspect or operate AYX CLI or Alteryx
One resources.

## Discovery-first operation

Start unfamiliar work by discovering the live command tree. Put structured output
last in every command:

```text
ayx discover one --deep --output json-full
```

Walk the returned `data.tree`; do not guess command names, flags, positional
arguments, IDs, payload schemas, or endpoint paths. Use command help only after
discovery identifies the command.

For ordinary agent-readable command results, use this shape:

```text
ayx one <family> <command> ... --output json
```

Use `--output json-full` for discovery because compact `json` intentionally
omits the large command tree.

Read the standard envelope carefully:

- object payloads are commonly in `data.response`;
- normalized paginated results are commonly in `data.items`;
- inspect `data.page_envelopes[].status_code` before treating a list as live-success;
- preserve `error_code`, `status_code`, `surface`, `operation`, and request IDs in findings;
- never print or retain access tokens, passwords, cookies, or secret bodies;
- on failure, branch on `error_code` and `retryable`; when `remediation.commands`
  is present, run those commands before re-attempting the original;
- on paginated successes, `next[0]` is the exact command for the next page.

## Mutations

Before any write, run the exact command without `--apply` and inspect its dry-run
envelope. Apply only when the user has authorized that specific mutation. For
canary work, use a unique run name, capture the returned resource ID, verify the
created and updated resource, and delete by ID in cleanup. Never use a name-based
bulk delete.

Classify outcomes as `validated_live`, `validated_shape`, `blocked_by_scope`, or
`blocked_by_fixture`. A successful CLI envelope is not enough if the underlying
page status is non-2xx or the response does not prove the requested operation.

For the complete live CRUD procedure, read
[references/live-crud-protocol.md](references/live-crud-protocol.md). For the
copy/paste agent handoff, use
[docs/agent-guide.md](../../docs/agent-guide.md).

## Reporting issues

Log each reproducible issue in `docs/ayx-cli-testing-issues.md` with the command,
discovered path, endpoint, status code, expected behavior, observed behavior,
classification, and proposed fix. Distinguish CLI defects from upstream API
behavior, permission boundaries, and missing fixtures.
