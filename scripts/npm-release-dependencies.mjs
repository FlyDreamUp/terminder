import { readFile } from 'node:fs/promises';
import path from 'node:path';

export async function readNpmReleaseDependencies(lock, packageRoot) {
  const items = [];
  for (const [entry, pkg] of Object.entries(lock.packages)) {
    // npm ci omits extraneous lock entries even when a local node_modules still has them.
    if (!entry || pkg.dev || pkg.extraneous) continue;
    const directory = path.join(packageRoot, entry);
    const manifest = JSON.parse(await readFile(path.join(directory, 'package.json'), 'utf8'));
    if (typeof manifest.name !== 'string' || !manifest.name) {
      throw new Error(`Missing package name in release manifest: ${entry}`);
    }
    if (manifest.version !== pkg.version) {
      throw new Error(`Release dependency version mismatch: ${manifest.name}, locked ${pkg.version}, installed ${manifest.version}`);
    }
    items.push({
      ecosystem: 'npm', name: manifest.name, version: pkg.version,
      license: pkg.license ?? manifest.license, repository: manifest.repository,
      sourceArchive: pkg.resolved, directory,
    });
  }
  return items;
}
