import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { dirname, resolve } from 'node:path';

const cargoTomlPath = [
  resolve(process.cwd(), '../Cargo.toml'),
  resolve(process.cwd(), 'Cargo.toml'),
].find(existsSync);

if (!cargoTomlPath) {
  throw new Error('Unable to locate root Cargo.toml');
}

const cargoToml = readFileSync(cargoTomlPath, 'utf8');
const workspacePackage = cargoToml.match(
  /(?:^|\n)\[workspace\.package\]\s*\n([\s\S]*?)(?=\n\[|$)/
)?.[1];
const workspaceVersion = workspacePackage?.match(/(?:^|\n)version\s*=\s*"([^"]+)"/)?.[1];

if (!workspaceVersion) {
  throw new Error('Unable to read workspace package version from Cargo.toml');
}

const releasePattern = /^v(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?\.md$/;

function compareReleaseVersionsDescending(a: string, b: string): number {
  const av = releasePattern.exec(a);
  const bv = releasePattern.exec(b);
  if (!av || !bv) return 0;

  for (const index of [1, 2, 3]) {
    const difference = Number(bv[index]) - Number(av[index]);
    if (difference !== 0) return difference;
  }

  const apre = av[4] ?? null;
  const bpre = bv[4] ?? null;
  if (apre === bpre) return 0;
  if (!apre) return -1;
  if (!bpre) return 1;
  return bpre.localeCompare(apre, undefined, { numeric: true });
}

const releaseDir = resolve(dirname(cargoTomlPath), 'docs', 'releases');
const latestRelease = existsSync(releaseDir)
  ? readdirSync(releaseDir)
      .filter((filename) => releasePattern.test(filename))
      .sort(compareReleaseVersionsDescending)[0]
  : undefined;
const publicVersion = latestRelease?.slice(0, -3);

export const ayxVersion = workspaceVersion;
export const ayxVersionLabel = publicVersion ?? `v${workspaceVersion}`;
