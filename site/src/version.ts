import { existsSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';

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

export const ayxVersion = workspaceVersion;
export const ayxVersionLabel = `v${workspaceVersion}`;
