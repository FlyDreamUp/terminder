// Explicit diagnostic build only. No test server, mock or config override is in the shipping build.
import { chromium } from 'playwright';
import { spawn, execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { mkdir, writeFile, readFile } from 'node:fs/promises';
import path from 'node:path';
import os from 'node:os';
import assert from 'node:assert/strict';
import { setTimeout as delay } from 'node:timers/promises';
const exec = promisify(execFile);
const root = path.resolve(import.meta.dirname, '../../..');
const executable = path.resolve(process.argv[2] ?? path.join(root, 'target/release/terminder-desktop.exe'));
const fixture = path.resolve(process.argv[3] ?? path.join(root, 'target/debug/pty-fixture.exe'));
const output = path.join(root, 'target/native-smoke', new Date().toISOString().replace(/[:.]/g, '-'));
const port = 9427;
async function until(check, timeout = 15000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) { const result = await check(); if (result) return result; await delay(100); }
  throw new Error('Native smoke deadline exceeded');
}
async function endpoint() { try { return await (await fetch(`http://127.0.0.1:${port}/json/version`, { signal: AbortSignal.timeout(500) })).json(); } catch { return null; } }
assert.equal(process.platform, 'win32', 'Native WebView2 smoke requires Windows');
assert.equal(await endpoint(), null, 'Diagnostic port is already owned by another process');
await mkdir(path.join(output, 'config'), { recursive: true });
await writeFile(path.join(output, 'config/config.toml'), `schema_version = 1
default_profile = "test-shell"
[session]
confirm_close_running = false
[terminal]
theme = "dark"
renderer = "dom"
[[profiles]]
id = "test-shell"
name = "Native contract fixture"
platforms = ["windows"]
program = ${JSON.stringify(fixture)}
args = ["echo"]
cwd_policy = "fixed"
cwd = ${JSON.stringify(output)}
`);
const child = spawn(executable, [], { cwd: root, env: { ...process.env, TERMINDER_TEST_DATA_ROOT: output, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-address=127.0.0.1 --remote-debugging-port=${port}` }, stdio: ['ignore', 'pipe', 'pipe'] });
child.stdout.resume();
let stderr = ''; child.stderr.on('data', bytes => { if (stderr.length < 32768) stderr += bytes.toString(); });
let exit = null; child.on('error', error => { stderr += error.message; exit = { code: null, signal: 'spawn-error' }; }); child.on('exit', (code, signal) => { exit = { code, signal }; });
let browser;
const report = { diagnosticBuild: true, environment: { os: os.release(), architecture: process.arch, node: process.version }, executable, started: new Date().toISOString(), checks: [], errors: [], output };
try {
  await until(async () => { if (exit) throw new Error(`App exited early: ${JSON.stringify(exit)} ${stderr}`); return endpoint(); }, 30000);
  report.environment.webview = (await endpoint()).Browser;
  browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
  const page = await until(() => browser.contexts().flatMap(c => c.pages()).find(p => p.url().includes('tauri.localhost')));
  page.on('pageerror', error => report.errors.push(error.message));
  await page.getByText('运行中', { exact: true }).waitFor({ timeout: 20000 });
  const initial = await page.evaluate(() => window.__TAURI_INTERNALS__.invoke('list_sessions', {}));
  assert.equal(initial.length, 1); assert.equal(initial[0].phase, 'running'); report.checks.push('real profile -> ConPTY -> raw Channel -> xterm');
  await page.locator('.xterm-helper-textarea').focus();
  await page.keyboard.type('native-contract-');
  const cdp = await page.context().newCDPSession(page);
  // Exercise the composition path; insertText alone has no keyboard/composition events and
  // xterm intentionally ignores that path when its accessibility support is enabled.
  await cdp.send('Input.imeSetComposition', { text: '中文', selectionStart: 2, selectionEnd: 2 });
  await cdp.send('Input.insertText', { text: '中文' });
  await page.keyboard.press('Enter');
  await page.getByRole('listitem').filter({ hasText: 'ECHO:native-contract-中文' }).first().waitFor(); report.checks.push('raw UTF-8 input and output roundtrip');
  const rejected = await page.evaluate(async binding => {
    try {
      await window.__TAURI_INTERNALS__.invoke('input', new Uint8Array([65]), { headers: { 'x-session-id': binding.sessionId, 'x-view-epoch': crypto.randomUUID(), 'x-protocol-version': '1', 'x-input-sequence': '2' } }); return 'accepted';
    } catch (error) { return error.code; }
  }, initial[0].binding);
  assert.equal(rejected, 'wrong_binding'); report.checks.push('stale view epoch rejected by Rust');
  const denied = await page.evaluate(async () => {
    try { await window.__TAURI_INTERNALS__.invoke('plugin:webview|create_webview_window', { options: { label: 'unauthorized', url: 'https://example.invalid' } }); return false; }
    catch { return true; }
  });
  assert.equal(denied, true); report.checks.push('unauthorized window creation rejected by capability');
  await page.getByRole('tab').first().focus(); await page.keyboard.press('F2');
  await page.getByRole('textbox', { name: '标签名称' }).fill('原生诊断会话'); await page.keyboard.press('Enter');
  await until(async () => { try { const workspace = JSON.parse(await readFile(path.join(output, 'state/workspace.json'), 'utf8')); return workspace.tabs[0]?.title === '原生诊断会话'; } catch { return false; } });
  report.checks.push('validated workspace metadata persisted');
  await page.getByRole('button', { name: '新建终端', exact: true }).click();
  await until(async () => (await page.getByRole('tab').count()) === 2);
  await page.getByText('运行中', { exact: true }).waitFor();
  await page.getByRole('tab', { name: '原生诊断会话' }).click();
  await page.getByRole('listitem').filter({ hasText: 'ECHO:native-contract-中文' }).first().waitFor();
  const retained = await page.evaluate(() => window.__TAURI_INTERNALS__.invoke('list_sessions', {}));
  assert.equal(retained.length, 2); assert(retained.some(s => s.binding.sessionId === initial[0].binding.sessionId)); report.checks.push('tab switching retains session and buffer');
  await page.screenshot({ path: path.join(output, 'native-terminal.png') });
  await page.reload(); await page.getByText('运行中', { exact: true }).waitFor({ timeout: 15000 });
  const restarted = await page.evaluate(() => window.__TAURI_INTERNALS__.invoke('list_sessions', {}));
  assert.equal(restarted.length, 1); assert.notEqual(restarted[0].binding.sessionId, initial[0].binding.sessionId); report.checks.push('navigation revokes previous document session ownership');
  const closeStarted = Date.now();
  await exec('powershell.exe', ['-NoProfile', '-Command', `(Get-Process -Id ${child.pid}).CloseMainWindow()`], { timeout: 10000 });
  await until(() => exit, 10000); assert.equal(exit.code, 0); report.closeMs = Date.now() - closeStarted;
  report.checks.push('native window close awaits supervisor cleanup and exits');
  assert.deepEqual(report.errors, []); report.passed = true;
} catch (error) {
  report.passed = false; report.failure = error.stack ?? String(error); report.stderr = stderr;
  if (browser) { for (const page of browser.contexts().flatMap(c => c.pages())) { try { await page.screenshot({ path: path.join(output, 'failure.png') }); } catch { /* The renderer may have already exited. */ } } }
  process.exitCode = 1;
} finally {
  if (!exit) child.kill();
  if (browser) { try { await browser.close(); } catch { /* Connection closes with the owned application. */ } }
  await writeFile(path.join(output, 'report.json'), JSON.stringify(report, null, 2));
  console.log(JSON.stringify(report, null, 2));
}
