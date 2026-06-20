import { cp, mkdir, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const siteRoot = path.resolve(__dirname, '..');
const repoRoot = path.resolve(siteRoot, '..');
const docsRoot = path.join(repoRoot, 'docs');

// -- clean generated output dirs --
await rm(path.join(siteRoot, 'docs', 'reference'), { recursive: true, force: true });
await rm(path.join(siteRoot, 'docs', 'releases'), { recursive: true, force: true });
await mkdir(path.join(siteRoot, 'docs'), { recursive: true });
await mkdir(path.join(siteRoot, 'docs', 'reference'), { recursive: true });
await mkdir(path.join(siteRoot, 'docs', 'releases'), { recursive: true });
await mkdir(path.join(siteRoot, 'static'), { recursive: true });

// -- direct copies: public reference pages --
const copies = [
  [path.join(docsRoot, 'cli-spec.md'),                 path.join(siteRoot, 'docs', 'reference', 'cli-spec.md')],
  [path.join(docsRoot, 'command-surface.md'),           path.join(siteRoot, 'docs', 'reference', 'command-surface.md')],
  [path.join(docsRoot, 'runtime-config-contract.md'),   path.join(siteRoot, 'docs', 'reference', 'runtime-config-contract.md')],
  [path.join(repoRoot, 'CONTRIBUTING.md'),              path.join(siteRoot, 'docs', 'contributing.md')],
  [path.join(docsRoot, 'releases', 'v0.9.10.md'),       path.join(siteRoot, 'docs', 'releases', 'v0.9.10.md')],
  [path.join(docsRoot, 'releases', 'v0.9.9.md'),        path.join(siteRoot, 'docs', 'releases', 'v0.9.9.md')],
  // Swagger spec: published to static/ so Redoc can reference it at build time
  [path.join(docsRoot, 'swagger-v3.json'),              path.join(siteRoot, 'static', 'swagger-v3.json')],
];

for (const [source, destination] of copies) {
  await mkdir(path.dirname(destination), { recursive: true });
  await cp(source, destination);
}

// -- generated pages --

await writeFile(path.join(siteRoot, 'docs', 'intro.md'), `---
title: Overview
sidebar_position: 1
---

# AYX-RS Docs

The guided front door for \`ayx-rs\` — a CLI toolset for Alteryx administrators.

- **Install and onboard** in minutes with the platform install scripts.
- **Explore the generated command surface** — 180+ commands, annotated by safety posture.
- **Understand the safety model** — read-only vs. mutating, \`--apply\` gate, audit artifacts.
- **Browse the Alteryx Server API reference** — rendered directly from the V3 spec.
- **Read versioned release notes** — know exactly what shipped in the binary you are running.

Start with [Getting started](./getting-started) or jump straight to the [command surface](./reference/command-surface).
`);

await writeFile(path.join(siteRoot, 'docs', 'getting-started.md'), `---
title: Getting Started
sidebar_position: 2
---

# Getting started

## Install

Use the platform install scripts for the fastest path:

\`\`\`bash
# macOS / Linux
curl -fsSL https://raw.githubusercontent.com/RyanMerlin/ayx-rs/main/scripts/install.sh | bash
\`\`\`

\`\`\`powershell
# Windows
iwr https://raw.githubusercontent.com/RyanMerlin/ayx-rs/main/scripts/install.ps1 | iex
\`\`\`

Both scripts download the latest release binary, verify its SHA-256 checksum, and place \`ayx\` on your \`PATH\`.

## Onboard

Run the onboarding wizard to create a central profile:

\`\`\`bash
ayx onboard
\`\`\`

Then verify the active profile and connectivity:

\`\`\`bash
ayx profile current
ayx doctor
ayx one platform workspace current --output json
\`\`\`

## Next steps

- [Configuration](./configuration) — profile shape, environment files, env overrides
- [Command surface](./reference/command-surface) — full command inventory with safety annotations
- [Safety model](./safety-model) — how read-only vs. mutating commands work
`);

await writeFile(path.join(siteRoot, 'docs', 'configuration.md'), `---
title: Configuration
sidebar_position: 3
---

# Configuration

## Config home

The central config home stores profiles, environment files, and sensitive runtime artifacts.

| Platform | Default path |
|----------|-------------|
| Linux / macOS | \`~/.config/ayx\` |
| Windows | \`%AppData%\\\\ayx\` |

## Profiles

A profile is a named YAML file inside the config home. Use \`ayx profile list\` to inspect stored profiles and \`ayx profile use <name>\` to switch the active default.

\`--profile <name>\` selects a central profile for a single run without changing the default.

Use \`ayx profile migrate --profile <path>\` to import a legacy YAML file into the central store.

## Environment files

\`environments.yaml\` is the canonical multi-environment file shape. It should contain \`workspace_name\`, \`active_environment\`, and an \`environments\` map of named \`Config\` entries.

Use \`--environment <name>\` to override the active environment for a single run.

## Minimum profile fields

\`\`\`yaml
profile_name: my-profile
alteryx_one:
  base_url: https://us1.alteryxcloud.com
  account_email: admin@example.com
  oauth_client_id: <client-id>
  token_endpoint_url: https://us1.alteryxcloud.com/oauth/token
  access_token: <token>
server:
  api:
    base_url: https://your-server.example.com
    client_id: <id>
    client_secret: <secret>
\`\`\`

## Environment variable overrides

| Variable | Purpose |
|----------|---------|
| \`AYX_ONE_CLIENT_ID\` | OAuth client ID |
| \`AYX_ONE_CLIENT_SECRET\` | OAuth client secret |
| \`AYX_ONE_TOKEN_ENDPOINT_URL\` | Token endpoint |
| \`AYX_ONE_API_ACCESS_TOKEN\` | Access token |
| \`AYX_ONE_API_REFRESH_TOKEN\` | Refresh token |

See the [runtime config contract](./reference/runtime-config-contract) for the detailed resolution order.
`);

await writeFile(path.join(siteRoot, 'docs', 'safety-model.md'), `---
title: Safety Model
sidebar_position: 4
---

# Safety model

\`ayx-rs\` has an explicit safety contract across the entire command surface:

- **Read-only commands** are available without extra flags. They never modify remote state.
- **Mutating commands** require \`--apply\`. Omitting \`--apply\` prints a dry-run summary and exits cleanly.
- **Audit artifacts** — several workflow and migration commands produce structured output files so operations can be reviewed or replayed before committing.
- **Unsupported surfaces** fail explicitly rather than silently succeeding with incomplete behavior.

## Command safety annotations

Every command in the [command surface](./reference/command-surface) is annotated with:

| Field | Meaning |
|-------|---------|
| \`Safety\` | \`safe\` — read-only; \`unsafe\` — potential side effects |
| \`Mutating\` | \`true\` when the command requires \`--apply\` to take effect |

## The \`--apply\` gate

Mutating commands that touch remote resources (delete, import, patch, transfer, migrate) require \`--apply\` to execute. Without it they print what *would* happen and exit with code 0. This makes it safe to run automation scripts against production without accidentally committing changes.

\`\`\`bash
# dry-run (no changes made)
ayx one flows delete <id>

# commit the delete
ayx one flows delete <id> --apply
\`\`\`

## Doctor

\`ayx doctor\` validates config, auth, and connectivity without touching remote state. Use it to diagnose issues before running any mutating workflow.

\`\`\`bash
ayx doctor
ayx one doctor discover
\`\`\`
`);

await writeFile(path.join(siteRoot, 'docs', 'troubleshooting.md'), `---
title: Troubleshooting
sidebar_position: 2
---

# Troubleshooting

## Start with doctor

\`ayx doctor\` validates configuration, auth, and network connectivity without touching remote state.

\`\`\`bash
ayx doctor
ayx one doctor discover
\`\`\`

## Reference surfaces

| Problem | Go to |
|---------|-------|
| Command not found or unexpected behavior | [Command surface](./reference/command-surface) |
| Config resolution order or profile shape | [Runtime config contract](./reference/runtime-config-contract) |
| CLI flags and stable behavior contract | [CLI spec](./reference/cli-spec) |
| API paths and parameters | [API Reference](/reference/api/) |

## Site vs. binary disagreement

If this site and your local binary disagree, trust the binary and the checked-in release notes first. The command surface page is regenerated from the live clap tree on every CI run — check which release version you are on with \`ayx --version\` and compare it against the [release notes](./releases).
`);

await writeFile(path.join(siteRoot, 'docs', 'releases', 'index.md'), `---
title: Releases
sidebar_position: 1
---

# Releases

Release notes for each tagged version of \`ayx-rs\`. For the current behavior, use the live docs above.

- [v0.9.10](./v0.9.10)
- [v0.9.9](./v0.9.9)
`);

console.log('sync-docs: done');
