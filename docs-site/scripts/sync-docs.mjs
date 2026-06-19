import { cp, mkdir, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const siteRoot = path.resolve(__dirname, '..');
const repoRoot = path.resolve(siteRoot, '..');
const docsRoot = path.join(repoRoot, 'docs');

const copies = [
  ['cli-spec.md', 'reference/cli-spec.md'],
  ['command-surface.md', 'reference/command-surface.md'],
  ['runtime-config-contract.md', 'reference/runtime-config-contract.md'],
  ['public-release-checklist.md', 'operations/public-release-checklist.md'],
  ['one-live-validation.md', 'operations/one-live-validation.md'],
  ['CONTRIBUTING.md', 'contributing.md'],
];

await rm(path.join(siteRoot, 'docs', 'reference'), { recursive: true, force: true });
await rm(path.join(siteRoot, 'docs', 'operations'), { recursive: true, force: true });
await rm(path.join(siteRoot, 'docs', 'releases'), { recursive: true, force: true });
await mkdir(path.join(siteRoot, 'docs'), { recursive: true });
await mkdir(path.join(siteRoot, 'docs', 'reference'), { recursive: true });
await mkdir(path.join(siteRoot, 'docs', 'operations'), { recursive: true });
await mkdir(path.join(siteRoot, 'docs', 'releases'), { recursive: true });

for (const [sourceRelative, destRelative] of copies) {
  const source = sourceRelative === 'CONTRIBUTING.md'
    ? path.join(repoRoot, sourceRelative)
    : path.join(docsRoot, sourceRelative);
  const destination = path.join(siteRoot, 'docs', destRelative);
  await mkdir(path.dirname(destination), { recursive: true });
  await cp(source, destination);
}

await cp(path.join(docsRoot, 'releases', 'v0.9.10.md'), path.join(siteRoot, 'docs', 'releases', 'v0.9.10.md'));
await cp(path.join(docsRoot, 'releases', 'v0.9.9.md'), path.join(siteRoot, 'docs', 'releases', 'v0.9.9.md'));

const releasesIndex = `---
title: Releases
sidebar_position: 1
---

# Releases

Use the latest docs for current behavior and these release notes for the exact public surface shipped in each tagged version.

- [v0.9.10](./v0.9.10)
- [v0.9.9](./v0.9.9)
`;

await writeFile(path.join(siteRoot, 'docs', 'releases', 'index.md'), releasesIndex);

const intro = `---
title: Overview
sidebar_position: 1
---

# AYX-RS Docs

This docs surface is the guided front door for \`ayx-rs\`:

- learn the install and onboarding flow
- inspect the generated command surface
- understand config, profiles, and environment loading
- read the release notes for the version you are on
- find the release checklist and contributing rules

Start with [Getting started](../getting-started) or jump straight to the [command surface](../reference/command-surface).
`;

await writeFile(path.join(siteRoot, 'docs', 'intro.md'), intro);

const gettingStarted = `---
title: Getting Started
sidebar_position: 2
---

# Getting started

Use the install scripts for the fastest path, then run \`ayx onboard\` to create a central profile.

\`\`\`bash
curl -fsSL https://raw.githubusercontent.com/RyanMerlin/ayx-rs/main/scripts/install.sh | bash
\`\`\`

\`\`\`powershell
iwr https://raw.githubusercontent.com/RyanMerlin/ayx-rs/main/scripts/install.ps1 | iex
\`\`\`

Then validate the active profile:

\`\`\`powershell
ayx onboard
ayx profile current
ayx one platform workspace current --output json
\`\`\`
`;

await writeFile(path.join(siteRoot, 'docs', 'getting-started.md'), gettingStarted);

const configuration = `---
title: Configuration
sidebar_position: 3
---

# Configuration

The default config home stores profiles, environment files, and sensitive runtime artifacts.

- Linux/macOS: \`~/.config/ayx\`
- Windows: \`%AppData%\\\\ayx\`

See the [runtime config contract](./reference/runtime-config-contract) for the detailed shape and the [release checklist](./operations/public-release-checklist) for the storage and safety rules.
`;

await writeFile(path.join(siteRoot, 'docs', 'configuration.md'), configuration);

const troubleshooting = `---
title: Troubleshooting
sidebar_position: 2
---

# Troubleshooting

Use these surfaces when a command, profile, or release path looks wrong:

- [Public release checklist](./operations/public-release-checklist)
- [Live One validation notes](./operations/one-live-validation)
- [Command surface](./reference/command-surface)
- [CLI spec](./reference/cli-spec)

If the site and the binary disagree, trust the generated command surface and the checked-in release notes first.
`;

await writeFile(path.join(siteRoot, 'docs', 'troubleshooting.md'), troubleshooting);
