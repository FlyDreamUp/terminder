// Use one editable vector for the in-app brand, taskbar, executable and installer.
import { execFileSync } from 'node:child_process';
import { copyFile, mkdtemp, rm } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';

const root = path.resolve(import.meta.dirname, '..');
const temporary = await mkdtemp(path.join(os.tmpdir(), 'terminder-icons-'));
try {
  execFileSync(process.execPath, [
    path.join(root, 'apps/desktop/node_modules/@tauri-apps/cli/tauri.js'),
    'icon', path.join(root, 'apps/desktop/src/assets/terminder.svg'),
    '--output', temporary,
  ], { cwd: root, stdio: 'inherit' });
  await copyFile(path.join(temporary, 'icon.ico'), path.join(root, 'apps/desktop/src-tauri/icons/icon.ico'));
} finally {
  await rm(temporary, { recursive: true, force: true });
}
