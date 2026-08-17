---
title: Delete
description: Permanently delete a cloud-native workflow from the CLI.
sidebar:
  order: 4
---

:::caution[Irreversible]
Deleting a cloud-native workflow cannot be undone. The Alteryx One API exposes no restore or trash
endpoint for this resource, so there is nothing for the CLI to call to bring one back. Confirm the
id and the workflow name before you add `--apply`.
:::

## Quick reference

| Command | Key options | What it does |
|---|---|---|
| `ayx one workflows delete <id>` | `--profile`, `--env` | Delete a cloud-native workflow |

Every leaf also accepts the global `--output`, `--apply`, `--verbose`, `--debug`, `--no-verify-tls`, and `--yes` flags. Use `--output json` for automation, `--env <ENVIRONMENT_FLAG>` to select a named environment, and `--profile <name>` on the leaves that expose it.

## How it works

The CLI resolves the workflow from the assets listing *before* prompting and *before* sending
anything. Two consequences worth knowing:

- The confirmation names the workflow rather than showing a bare ULID — you're confirming
  "delete workflow 'Revenue Model' (id='01M...')", not a string you have to look up separately.
- An unknown or empty id is rejected with a clean `not_found` before any mutating request leaves
  the machine.

## Dry run and apply

```bash
# Dry-run: preview the request; nothing is sent
ayx one workflows delete <workflow-ulid>

# Apply the delete
ayx one workflows delete <workflow-ulid> --apply
```

The dry-run envelope's `would_send` is `null` for this command — there is no request body to
preview, unlike `copy` or `share`. Don't key automation off `would_send` here; it carries no
information beyond confirming the command didn't fire.

## Confirmation

With `--apply` and no `--yes`, the CLI prompts before sending the request:

```
About to delete workflow 'Revenue Model' (id='01M...') on profile 'default'. This is destructive and may affect live workflows or users. Review carefully before proceeding.
Type 'yes' to proceed:
```

Only a literal `yes` (case-insensitive) proceeds. Off a TTY without `--yes`, the command refuses
outright rather than hanging on a prompt no one can answer:

```
destructive operation requires confirmation. Re-run with --yes (non-interactive) or attach a TTY for the interactive prompt.
```

## Automation

```bash
# CI / pipes: --apply plus --yes, since there's no TTY to read a prompt from
ayx one workflows delete <workflow-ulid> --apply --yes
```

The live verification for this command used the same pattern any automation should: delete, then
confirm removal three separate ways rather than trusting the `200` alone — the id drops out of
`workflows list --all`, `workflows count` decreases by one, and `workflows detail <id>` returns
`not_found`.

## Related

- [Workflows](/one/workflows/)
- [Inspect](/one/workflows/inspect/)
- [Copy & share](/one/workflows/share/)
- [Safety model](/safety-model/)
- [Output & automation](/output-automation/)
