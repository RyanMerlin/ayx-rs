---
title: Agent Studio assets
description: Manage the Agent Studio assets used by Alteryx MCP Gateway demos.
sidebar:
  order: 7
---

`ayx one agent-assets` exposes the Agent Studio setup operations recovered from
the authenticated workspace UI. These commands use private-preview service
routes and are separate from the public One OpenAPI inventory.

## Inspect and create agents

Agent creation and updates use the same JSON shape as the Agent Studio form:

```json
{
  "name": "MCP demo agent",
  "description": "A small demo agent",
  "prompt": "Help me inspect approved Alteryx assets.",
  "questions": ["What should I inspect?"],
  "skills": {"insightsMcp": null}
}
```

```bash
ayx one agent-assets agents list
ayx one agent-assets agents create --body agent.json
ayx --apply one agent-assets agents create --body agent.json
ayx one agent-assets agents detail <agent-id>
```

Submit a prompt to an agent and print its response. This starts a new
conversation; applying the request requires `--apply` because the agent may
call configured tools or workflows:

```bash
ayx --apply one agent-assets agents prompt <agent-id> \
  --prompt "Summarize the registered demo dataset."
```

Do not put secrets or sensitive data in a prompt. The CLI currently uses the
non-streaming Copilot response path.

Updates and deletion are also preview-first and require `--apply` to mutate:

```bash
ayx --apply one agent-assets agents update <agent-id> --body agent.json
ayx --apply one agent-assets agents delete <agent-id>
```

## Register MCP assets

Datasets must be enabled for Agent Studio Insights. Workflows can be registered
as Apps shortcuts; registration is asynchronous and the CLI waits for the
tool-creation job to complete.

```bash
ayx one agent-assets datasets list
ayx --apply one agent-assets datasets set <dataset-id> --enable
ayx one agent-assets workflows list
ayx --apply one agent-assets workflows enable <workflow-id>
ayx --apply one agent-assets workflows disable <workflow-id>
```

These setup operations do not grant permissions. The caller still needs access
to the underlying dataset, workflow, and Agent Studio features.
