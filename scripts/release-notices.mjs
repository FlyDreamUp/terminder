// Inventory the exact locked dependencies; preserve upstream license/notice text verbatim.
import { execFileSync } from 'node:child_process';
import { readFile, readdir, mkdir, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { readNpmReleaseDependencies } from './npm-release-dependencies.mjs';
const root = path.resolve(import.meta.dirname, '..');
const out = path.join(root, 'target/release-notices');
const metadata = JSON.parse(execFileSync('cargo', ['metadata', '--locked', '--format-version=1', '--filter-platform=x86_64-pc-windows-msvc'], { cwd: root, encoding: 'utf8', maxBuffer: 32 * 1024 * 1024 }));
const lock = JSON.parse(await readFile(path.join(root, 'apps/desktop/package-lock.json'), 'utf8'));
const supplements = JSON.parse(await readFile(path.join(root, 'docs/third_party/notices/sources.json'), 'utf8'));
const items = [];
for (const pkg of metadata.packages.filter(pkg => pkg.source)) {
  items.push({ ecosystem: 'cargo', name: pkg.name, version: pkg.version, license: pkg.license, repository: pkg.repository, sourceArchive: `https://crates.io/api/v1/crates/${pkg.name}/${pkg.version}/download`, directory: path.dirname(pkg.manifest_path), licenseFile: pkg.license_file });
}
items.push(...await readNpmReleaseDependencies(lock, path.join(root, 'apps/desktop')));
items.sort((a, b) => `${a.ecosystem}/${a.name}/${a.version}`.localeCompare(`${b.ecosystem}/${b.name}/${b.version}`));
const text = ['Terminder license', await readFile(path.join(root, 'LICENSE'), 'utf8'), '\nTerminder third-party notices', 'Generated from Cargo.lock and package-lock.json. Cargo inventory includes build/test dependencies as a conservative superset; npm inventory includes production dependencies.', 'This inventory preserves declared license expressions and available upstream notices; release review must resolve missing metadata.\n'];
const inventory = [];
for (const item of items) {
  const entries = await readdir(item.directory, { withFileTypes: true });
  const files = entries.filter(e => e.isFile() && /^(licen[sc]e|copying|copyright|notice)([-_.]|$)/i.test(e.name)).map(e => e.name);
  if (item.licenseFile && !files.includes(item.licenseFile)) files.push(item.licenseFile);
  text.push(`\n${'='.repeat(72)}\n${item.ecosystem}: ${item.name} ${item.version}\nLicense: ${item.license ?? 'UNKNOWN'}\nSource: ${item.sourceArchive}\n`);
  for (const file of files.sort()) text.push(`--- ${file} ---\n${await readFile(path.resolve(item.directory, file), 'utf8')}\n`);
  let supplement = null;
  if (!files.length) {
    supplement = supplements.find(s => s.ecosystem === item.ecosystem && s.name === item.name && s.version === item.version);
    if (!supplement) throw new Error(`Missing upstream notice: ${item.name} ${item.version}`);
    const bytes = await readFile(path.join(root, 'docs/third_party/notices', supplement.file));
    if (createHash('sha256').update(bytes).digest('hex') !== supplement.sha256) throw new Error(`Notice checksum mismatch: ${supplement.file}`);
    text.push(`--- ${supplement.file} ---\nSource: ${supplement.source}\n${supplement.note ?? ''}\n${bytes.toString('utf8')}\n`);
    files.push(`supplement:${supplement.file}`);
  }
  const { directory: _directory, licenseFile: _licenseFile, ...record } = item;
  inventory.push({ ...record, noticeFiles: files, supplement });
}
await mkdir(out, { recursive: true });
await writeFile(path.join(out, 'THIRD-PARTY-NOTICES.txt'), text.join('\n'));
await writeFile(path.join(out, 'dependency-inventory.json'), JSON.stringify(inventory, null, 2));
console.log(`Recorded ${inventory.length} locked dependencies in ${out}`);
