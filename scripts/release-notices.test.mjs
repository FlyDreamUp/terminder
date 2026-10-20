import assert from 'node:assert/strict';
import { mkdtemp, mkdir, rm, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { readNpmReleaseDependencies } from './npm-release-dependencies.mjs';

async function fixture(t) {
  const root = await mkdtemp(path.join(os.tmpdir(), 'terminder-notices-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  return root;
}
async function install(root, entry, manifest) {
  const directory = path.join(root, entry);
  await mkdir(directory, { recursive: true });
  await writeFile(path.join(directory, 'package.json'), JSON.stringify(manifest));
}

for (const yamlInstalled of [false, true]) {
  test(`excludes extraneous yaml when ${yamlInstalled ? 'left over locally' : 'absent after npm ci'}`, async t => {
    const root = await fixture(t);
    const manifest = { name: '@example/runtime', version: '1.0.0', license: 'MIT', repository: 'https://example.invalid/runtime' };
    const lock = { packages: {
      '': { name: 'app' },
      'node_modules/@example/runtime': { version: '1.0.0', license: 'MIT', resolved: 'https://example.invalid/runtime.tgz' },
      'node_modules/@example/runtime/node_modules/shared': { version: '2.0.0', devOptional: true, resolved: 'https://example.invalid/shared.tgz' },
      'node_modules/dev-tool': { version: '1.0.0', dev: true },
      // The CI failure came from a stale lock entry without a dev flag.
      'node_modules/yaml': { version: '2.9.1', extraneous: true },
    } };
    await install(root, 'node_modules/@example/runtime', manifest);
    await install(root, 'node_modules/@example/runtime/node_modules/shared', { name: 'shared', version: '2.0.0', license: 'ISC' });
    if (yamlInstalled) await install(root, 'node_modules/yaml', { name: 'yaml', version: '2.9.1', license: 'ISC' });
    const items = await readNpmReleaseDependencies(lock, root);
    assert.deepEqual(items, [
      { ecosystem: 'npm', name: '@example/runtime', version: '1.0.0', license: 'MIT', repository: manifest.repository, sourceArchive: 'https://example.invalid/runtime.tgz', directory: path.join(root, 'node_modules/@example/runtime') },
      { ecosystem: 'npm', name: 'shared', version: '2.0.0', license: 'ISC', repository: undefined, sourceArchive: 'https://example.invalid/shared.tgz', directory: path.join(root, 'node_modules/@example/runtime/node_modules/shared') },
    ]);
  });
}

test('missing release dependencies still fail instead of producing incomplete notices', async t => {
  const root = await fixture(t);
  const lock = { packages: { 'node_modules/required': { version: '1.0.0' } } };
  await assert.rejects(readNpmReleaseDependencies(lock, root), { code: 'ENOENT' });
});

test('invalid release manifests still fail', async t => {
  const root = await fixture(t);
  const entry = 'node_modules/required';
  await install(root, entry, { name: 'required', version: '1.0.0' });
  await writeFile(path.join(root, entry, 'package.json'), '{invalid');
  await assert.rejects(readNpmReleaseDependencies({ packages: { [entry]: { version: '1.0.0' } } }, root), SyntaxError);
});
