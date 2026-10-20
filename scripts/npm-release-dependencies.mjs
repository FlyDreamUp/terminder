import { readFile } from 'node:fs/promises';
import path from 'node:path';

export async function readNpmReleaseDependencies(lock, packageRoot) {
  const items = [];
  for (const [entry, pkg] of Object.entries(lock.packages)) {
    // npm ci omits extraneous lock entries even when a local node_modules still has them.
    if (!entry || pkg.dev || pkg.extraneous) continue;
    const directory = path.join(packageRoot, entry);
    const manifest = JSON.parse(await readFile(path.join(directory, 'package.json'), 'utf8'));
    items.push({
      ecosystem: 'npm', name: manifest.name, version: pkg.version,
      license: pkg.license ?? manifest.license, repository: manifest.repository,
      sourceArchive: pkg.resolved, directory,
    });
  }
  return items;
}
