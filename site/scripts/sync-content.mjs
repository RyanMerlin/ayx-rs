// Pulls the generated/maintained reference docs out of repo-root `docs/` and
// writes them into the Starlight content collection with the frontmatter
// Starlight needs. The source files are the single source of truth; everything
// under src/content/docs/reference/ and the release notes are GENERATED here.
import { readFile, writeFile, mkdir } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const siteRoot = path.resolve(__dirname, '..');
const repoRoot = path.resolve(siteRoot, '..');
const docsRoot = path.join(repoRoot, 'docs');
const outRoot = path.join(siteRoot, 'src', 'content', 'docs');

// [sourceAbsPath, destRelativeToOutRoot, title, sidebarOrder]
const sources = [
  [path.join(docsRoot, 'command-surface.md'), 'reference/command-surface.md', 'Command Surface', 1],
  [path.join(docsRoot, 'cli-spec.md'), 'reference/cli-spec.md', 'CLI Spec', 2],
  [path.join(docsRoot, 'runtime-config-contract.md'), 'reference/runtime-config-contract.md', 'Runtime Config Contract', 3],
  [path.join(docsRoot, 'releases', 'v0.9.10.md'), 'releases/v0.9.10.md', 'v0.9.10', 1],
  [path.join(docsRoot, 'releases', 'v0.9.9.md'), 'releases/v0.9.9.md', 'v0.9.9', 2],
];

// Drop a single leading "# Title" so it doesn't duplicate the Starlight page title.
function stripLeadingH1(body) {
  const lines = body.split('\n');
  let i = 0;
  while (i < lines.length && lines[i].trim() === '') i++;
  if (i < lines.length && /^#\s+/.test(lines[i])) {
    lines.splice(i, 1);
    if (i < lines.length && lines[i].trim() === '') lines.splice(i, 1);
  }
  return lines.join('\n');
}

for (const [src, dest, title, order] of sources) {
  const raw = await readFile(src, 'utf8');
  const body = stripLeadingH1(raw).replace(/^\s+/, '');
  const rel = path.relative(docsRoot, src).split(path.sep).join('/');
  const frontmatter =
    `---\n` +
    `title: "${title.replace(/"/g, '\\"')}"\n` +
    `sidebar:\n  order: ${order}\n` +
    `---\n\n` +
    `<!-- GENERATED from docs/${rel} by site/scripts/sync-content.mjs — edit the source, not this file. -->\n\n`;
  const out = path.join(outRoot, dest);
  await mkdir(path.dirname(out), { recursive: true });
  await writeFile(out, frontmatter + body.trimEnd() + '\n');
  console.log(`  synced docs/${rel} -> ${dest}`);
}

console.log('sync-content: done');
