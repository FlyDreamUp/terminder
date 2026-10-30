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
  throw new Error(`Native smoke deadline exceeded during ${report.stage}`);
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
// WebView2 ignores WEBVIEW2_* environment settings in elevated runner processes.
// The diagnostic host passes these fixed-purpose settings through the WebView2 API.
const environment = { ...process.env, TERMINDER_TEST_DATA_ROOT: output, TERMINDER_TEST_CDP_PORT: String(port) };
for (const key of Object.keys(environment)) {
  if (key.toUpperCase() === 'WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS') delete environment[key];
}
const child = spawn(executable, [], { cwd: root, env: environment, stdio: ['ignore', 'pipe', 'pipe'] });
child.stdout.resume();
let stderr = ''; child.stderr.on('data', bytes => { if (stderr.length < 32768) stderr += bytes.toString(); });
let exit = null; child.on('error', error => { stderr += error.message; exit = { code: null, signal: 'spawn-error' }; }); child.on('exit', (code, signal) => { exit = { code, signal }; });
let browser;
const report = { diagnosticBuild: true, environment: { os: os.release(), architecture: process.arch, node: process.version }, executable, started: new Date().toISOString(), checks: [], errors: [], output, stage: 'webview2_endpoint', cdpEndpoint: `http://127.0.0.1:${port}` };
let windowHandle;
async function nativeWindow(action = 'inspect') {
  const { stdout } = await exec('powershell.exe', [
    '-NoProfile', '-NonInteractive', '-File', path.join(import.meta.dirname, 'native-window.ps1'),
    '-ProcessId', String(child.pid), '-Action', action,
    ...(windowHandle ? ['-WindowHandle', windowHandle] : []),
  ], { timeout: 10000, windowsHide: true });
  const state = JSON.parse(stdout);
  windowHandle ??= state.handle;
  return state;
}
try {
  await until(async () => { if (exit) throw new Error(`App exited early: ${JSON.stringify(exit)} ${stderr}`); return endpoint(); }, 30000);
  report.environment.webview = (await endpoint()).Browser;
  report.stage = 'webview2_connect';
  browser = await chromium.connectOverCDP(report.cdpEndpoint, { timeout: 15000 });
  report.stage = 'application_page';
  const page = await until(() => browser.contexts().flatMap(c => c.pages()).find(p => p.url().includes('tauri.localhost')));
  page.on('pageerror', error => report.errors.push(error.message));
  report.stage = 'session_running';
  await page.getByText('运行中', { exact: true }).waitFor({ timeout: 20000 });
  const initial = await page.evaluate(() => window.__TAURI_INTERNALS__.invoke('list_sessions', {}));
  assert.equal(initial.length, 1); assert.equal(initial[0].phase, 'running'); report.checks.push('real profile -> ConPTY -> raw Channel -> xterm');
  report.stage = 'window_chrome';
  await page.evaluate(() => {
    window.__windowTrace = [];
    for (const type of ['pointerdown', 'pointerup', 'click', 'resize', 'focus', 'blur']) {
      window.addEventListener(type, event => {
        const button = event.target instanceof Element ? event.target.closest('button') : null;
        window.__windowTrace.push({ type, label: button?.getAttribute('aria-label'), disabled: button?.disabled, target: event.target instanceof Element ? event.target.tagName : 'window', time: performance.now(), width: innerWidth, height: innerHeight });
      }, true);
    }
  });
  const frame = await nativeWindow();
  assert.equal(frame.origin.X, frame.outer.Left);
  assert.equal(frame.origin.Y, frame.outer.Top);
  assert.equal(frame.client.Right, frame.outer.Right - frame.outer.Left);
  assert.equal(frame.client.Bottom, frame.outer.Bottom - frame.outer.Top);
  assert.deepEqual([frame.leftHit, frame.rightHit, frame.topHit, frame.bottomHit], [10, 11, 12, 15]);
  report.windowFrame = frame;
  await page.getByRole('button', { name: '最大化窗口', exact: true }).click();
  await page.getByRole('button', { name: '还原窗口', exact: true }).waitFor();
  assert.equal((await nativeWindow()).maximized, true);
  await page.getByRole('button', { name: '还原窗口', exact: true }).click();
  await page.getByRole('button', { name: '最大化窗口', exact: true }).waitFor();
  assert.equal((await nativeWindow()).maximized, false);
  await page.locator('.topbar-center').dblclick();
  await page.getByRole('button', { name: '还原窗口', exact: true }).waitFor();
  assert.equal((await nativeWindow()).maximized, true);
  await page.getByRole('button', { name: '还原窗口', exact: true }).click();
  await page.getByRole('button', { name: '最大化窗口', exact: true }).waitFor();
  // A changed button label can precede completion of the native restore/resize.
  report.windowRestored = await until(async () => {
    const state = await nativeWindow();
    return !state.maximized && !state.minimized && state.client.Right === frame.client.Right && state.client.Bottom === frame.client.Bottom ? state : false;
  }, 5000);
  await page.getByRole('button', { name: '最小化窗口', exact: true }).click();
  await until(async () => {
    report.windowMinimize = await nativeWindow();
    return report.windowMinimize.minimized;
  }, 5000);
  await nativeWindow('restore');
  await page.getByText('运行中', { exact: true }).waitFor();
  report.checks.push('borderless client fills window; resize edges, maximize, restore, titlebar double-click and minimize work');
  report.windowTrace = await page.evaluate(() => window.__windowTrace);
  report.stage = 'terminal_interaction';
  await page.locator('.xterm-helper-textarea').focus();
  await page.keyboard.type('native-contract-');
  const cdp = await page.context().newCDPSession(page);
  // Exercise the composition path; insertText alone has no keyboard/composition events and
  // xterm intentionally ignores that path when its accessibility support is enabled.
  await cdp.send('Input.imeSetComposition', { text: '中文', selectionStart: 2, selectionEnd: 2 });
  await cdp.send('Input.insertText', { text: '中文' });
  // xterm commits composition asynchronously. Wait for the exact echoed input before
  // Enter so it cannot finalize a stale composition range or clear the pending text.
  await page.getByRole('listitem').filter({ hasText: /^\s*native-contract-中文\s*$/u }).first().waitFor();
  await page.keyboard.press('Enter');
  await page.getByRole('listitem').filter({ hasText: 'ECHO:native-contract-中文' }).first().waitFor(); report.checks.push('raw UTF-8 input and output roundtrip');
  report.stage = 'permissions';
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
  report.stage = 'configuration_limit';
  const configPath = path.join(output, 'config/config.toml');
  const originalConfig = await readFile(configPath, 'utf8');
  await writeFile(configPath, originalConfig.replace('[session]', '[session]\nmax_sessions = 1'));
  const limited = await page.evaluate(async () => {
    const invoke = window.__TAURI_INTERNALS__.invoke;
    const document = await invoke('load_config', {});
    const saved = await invoke('save_terminal', { terminal: document.config.terminal, revision: document.revision });
    try {
      await invoke('prepare_session', { profileId: 'default', cwd: null, inheritSession: null });
      return { code: 'accepted', revision: saved.revision };
    } catch (error) { return { code: error.code, revision: saved.revision }; }
  });
  assert.equal(limited.code, 'session_limit');
  await page.evaluate(({ text, revision }) => window.__TAURI_INTERNALS__.invoke('save_config', { text, revision }), { text: originalConfig, revision: limited.revision });
  report.checks.push('appearance save applies the complete validated configuration and session limit');
  report.stage = 'workspace_and_tabs';
  await page.getByRole('tab').first().focus(); await page.keyboard.press('F2');
  await page.getByRole('textbox', { name: '标签名称' }).fill('原生诊断会话'); await page.keyboard.press('Enter');
  await until(async () => { try { const workspace = JSON.parse(await readFile(path.join(output, 'state/workspace.json'), 'utf8')); return workspace.tabs[0]?.title === '原生诊断会话'; } catch { return false; } });
  report.checks.push('validated workspace metadata persisted');
  await page.getByRole('button', { name: '新建终端', exact: true }).click();
  await page.getByRole('menu', { name: '选择终端类型' }).waitFor();
  assert.equal((await page.evaluate(() => window.__TAURI_INTERNALS__.invoke('list_sessions', {}))).length, 1);
  await page.screenshot({ path: path.join(output, 'profile-menu.png') });
  await page.getByRole('menuitem', { name: 'Native contract fixture', exact: true }).click();
  await until(async () => (await page.getByRole('tab').count()) === 2);
  await page.getByText('运行中', { exact: true }).waitFor();
  await page.getByRole('tab', { name: '原生诊断会话' }).click();
  await page.getByRole('listitem').filter({ hasText: 'ECHO:native-contract-中文' }).first().waitFor();
  const retained = await page.evaluate(() => window.__TAURI_INTERNALS__.invoke('list_sessions', {}));
  assert.equal(retained.length, 2); assert(retained.some(s => s.binding.sessionId === initial[0].binding.sessionId)); assert(retained.every(s => s.profileId === 'test-shell'));
  report.checks.push('plus opens the profile chooser without spawning; selection creates the requested profile; tab switching retains session and buffer');
  await page.screenshot({ path: path.join(output, 'native-terminal.png') });
  report.stage = 'split_panes';
  const activePanes = page.locator('.tab-content:not(.inactive) .session-pane');
  for (const [direction, orientation] of [['向右分屏', 'vertical'], ['向下分屏', 'horizontal']]) {
    await page.getByRole('button', { name: '分屏', exact: true }).click();
    await page.getByRole('menuitem', { name: direction, exact: true }).click();
    const splitSessions = await until(async () => {
      const sessions = await page.evaluate(() => window.__TAURI_INTERNALS__.invoke('list_sessions', {}));
      return sessions.length === 3 && sessions.every(s => s.phase === 'running') ? sessions : false;
    });
    assert.equal(await activePanes.count(), 2);
    const added = splitSessions.find(s => !retained.some(old => old.binding.sessionId === s.binding.sessionId));
    assert.equal(added.profileId, initial[0].profileId);
    assert.equal(added.startupCwd, initial[0].startupCwd);
    await activePanes.nth(0).locator('textarea').focus(); await page.keyboard.type('split-first'); await page.keyboard.press('Enter');
    await activePanes.nth(0).getByRole('listitem').filter({ hasText: 'ECHO:split-first' }).first().waitFor();
    await activePanes.nth(1).locator('textarea').focus(); await page.keyboard.type('split-second'); await page.keyboard.press('Enter');
    await activePanes.nth(1).getByRole('listitem').filter({ hasText: 'ECHO:split-second' }).first().waitFor();
    assert.equal(await activePanes.nth(0).getByRole('listitem').filter({ hasText: 'ECHO:split-second' }).count(), 0);
    assert.equal(await activePanes.nth(1).getByRole('listitem').filter({ hasText: 'ECHO:split-first' }).count(), 0);
    const divider = page.getByRole('separator'); assert.equal(await divider.getAttribute('aria-orientation'), orientation);
    const beforeResize = await page.evaluate(() => window.__TAURI_INTERNALS__.invoke('list_sessions', {}));
    const box = await divider.boundingBox();
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2); await page.mouse.down();
    await page.mouse.move(box.x + box.width / 2 + (orientation === 'vertical' ? 90 : 0), box.y + box.height / 2 + (orientation === 'horizontal' ? 60 : 0)); await page.mouse.up();
    await until(async () => {
      const sessions = await page.evaluate(() => window.__TAURI_INTERNALS__.invoke('list_sessions', {}));
      return [initial[0].binding.sessionId, added.binding.sessionId].every(id => {
        const before = beforeResize.find(s => s.binding.sessionId === id).size;
        const after = sessions.find(s => s.binding.sessionId === id).size;
        return orientation === 'vertical' ? before.cols !== after.cols : before.rows !== after.rows;
      });
    });
    await page.getByRole('button', { name: '最大化窗格 1', exact: true }).click();
    assert.equal(await activePanes.nth(1).isVisible(), false);
    await page.getByRole('button', { name: '还原分屏', exact: true }).click();
    await page.screenshot({ path: path.join(output, orientation === 'vertical' ? 'native-split-columns.png' : 'native-split-rows.png') });
    await page.getByRole('button', { name: '关闭窗格 2', exact: true }).click();
    await until(async () => {
      const sessions = await page.evaluate(() => window.__TAURI_INTERNALS__.invoke('list_sessions', {}));
      return sessions.length === 2 && sessions.every(s => retained.some(old => old.binding.sessionId === s.binding.sessionId));
    });
    await until(async () => (await activePanes.count()) === 1);
    report.checks.push(`${direction}: independent real ConPTY input, both sizes synchronized, zoom/restore and sibling-preserving cleanup`);
  }
  await page.getByRole('button', { name: '分屏', exact: true }).click();
  await page.getByRole('menuitem', { name: '向右分屏', exact: true }).click();
  await until(async () => (await page.evaluate(() => window.__TAURI_INTERNALS__.invoke('list_sessions', {}))).filter(s => s.phase === 'running').length === 3);
  report.stage = 'nested_panes';
  await page.evaluate(() => { window.__retainedTerminals = [...document.querySelectorAll('.tab-content:not(.inactive) .xterm')]; });
  const leftBeforeNested = await activePanes.nth(0).boundingBox();
  await page.getByRole('button', { name: '分屏窗格 2', exact: true }).click();
  await page.getByRole('menuitem', { name: '向下分屏', exact: true }).click();
  await until(async () => (await activePanes.count()) === 3 && (await page.evaluate(() => window.__TAURI_INTERNALS__.invoke('list_sessions', {}))).filter(s => s.phase === 'running').length === 4);
  assert.deepEqual(await activePanes.nth(0).boundingBox(), leftBeforeNested);
  const rightTop = await activePanes.nth(1).boundingBox(); const rightBottom = await activePanes.nth(2).boundingBox();
  await page.getByRole('button', { name: '分屏窗格 1', exact: true }).click();
  await page.getByRole('menuitem', { name: '向下分屏', exact: true }).click();
  const nestedSessions = await until(async () => {
    const sessions = await page.evaluate(() => window.__TAURI_INTERNALS__.invoke('list_sessions', {}));
    return sessions.length === 5 && sessions.every(s => s.phase === 'running' && s.size.cols >= 40 && s.size.rows >= 10) ? sessions : false;
  });
  assert.equal(await activePanes.count(), 4);
  assert.deepEqual(await activePanes.nth(1).boundingBox(), rightTop); assert.deepEqual(await activePanes.nth(2).boundingBox(), rightBottom);
  assert(await page.evaluate(() => window.__retainedTerminals.every(el => el.isConnected)));
  for (let index = 0; index < 4; index++) {
    await activePanes.nth(index).locator('textarea').focus();
    await page.keyboard.type(`nested-pane-${index + 1}`); await page.keyboard.press('Enter');
    await activePanes.nth(index).getByRole('listitem').filter({ hasText: `ECHO:nested-pane-${index + 1}` }).first().waitFor();
    for (let other = 0; other < 4; other++) if (other !== index) assert.equal(await activePanes.nth(other).getByRole('listitem').filter({ hasText: `ECHO:nested-pane-${index + 1}` }).count(), 0);
  }
  await page.keyboard.press('Alt+ArrowRight');
  assert(await activePanes.nth(2).locator('textarea').evaluate(el => el === document.activeElement));
  await page.keyboard.press('Alt+ArrowUp');
  assert(await activePanes.nth(1).locator('textarea').evaluate(el => el === document.activeElement));
  const rootDivider = page.getByRole('separator', { name: '调整窗格 1、4 大小', exact: true });
  await rootDivider.focus(); await page.keyboard.press('ArrowRight');
  const activeSessionIds = nestedSessions.filter(s => s.binding.sessionId !== retained.find(s => s.binding.sessionId !== initial[0].binding.sessionId).binding.sessionId).map(s => s.binding.sessionId);
  await until(async () => {
    const sessions = await page.evaluate(() => window.__TAURI_INTERNALS__.invoke('list_sessions', {}));
    return activeSessionIds.every(id => sessions.find(s => s.binding.sessionId === id)?.size.cols !== nestedSessions.find(s => s.binding.sessionId === id).size.cols);
  });
  const rootRatio = await rootDivider.getAttribute('aria-valuenow');
  await activePanes.nth(3).locator('textarea').focus(); await page.keyboard.press('Control+Alt+Z');
  assert.equal(await activePanes.nth(0).isVisible(), false);
  await page.keyboard.press('Alt+ArrowRight'); assert(await activePanes.nth(2).locator('textarea').evaluate(el => el === document.activeElement));
  await page.keyboard.press('Control+Alt+Z');
  assert.equal(await rootDivider.getAttribute('aria-valuenow'), rootRatio);
  await page.screenshot({ path: path.join(output, 'native-nested-four.png') });
  const leftBeforeClose = [await activePanes.nth(0).boundingBox(), await activePanes.nth(3).boundingBox()];
  await page.getByRole('button', { name: '关闭窗格 2', exact: true }).click();
  await until(async () => (await activePanes.count()) === 3 && (await page.evaluate(() => window.__TAURI_INTERNALS__.invoke('list_sessions', {}))).length === 4);
  assert.deepEqual(await activePanes.nth(0).boundingBox(), leftBeforeClose[0]); assert.deepEqual(await activePanes.nth(2).boundingBox(), leftBeforeClose[1]);
  assert(await activePanes.nth(1).locator('textarea').evaluate(el => el === document.activeElement));
  await activePanes.nth(1).getByRole('listitem').filter({ hasText: 'ECHO:nested-pane-3' }).first().waitFor();
  await page.getByRole('button', { name: '关闭窗格 2', exact: true }).click();
  await until(async () => (await activePanes.count()) === 2 && (await page.evaluate(() => window.__TAURI_INTERNALS__.invoke('list_sessions', {}))).length === 3);
  assert.equal(await page.getByRole('separator').getAttribute('aria-orientation'), 'horizontal');
  for (const [index, marker] of [[0, 'ECHO:nested-pane-1'], [1, 'ECHO:nested-pane-4']]) await activePanes.nth(index).getByRole('listitem').filter({ hasText: marker }).first().waitFor();
  await page.screenshot({ path: path.join(output, 'native-nested-collapse.png') });
  report.checks.push('nested four panes: existing xterm nodes and real ConPTY sessions retained; independent input, spatial focus, all descendant PTY sizes, zoom ratios and local subtree collapse');
  report.stage = 'page_reload';
  await page.reload(); await page.getByText('运行中', { exact: true }).waitFor({ timeout: 15000 });
  const restarted = await page.evaluate(() => window.__TAURI_INTERNALS__.invoke('list_sessions', {}));
  assert.equal(restarted.length, 1); assert.notEqual(restarted[0].binding.sessionId, initial[0].binding.sessionId); report.checks.push('navigation revokes previous document session ownership');
  report.stage = 'window_close';
  const closeStarted = Date.now();
  await page.getByRole('button', { name: '关闭窗口', exact: true }).click();
  await until(() => exit, 10000); assert.equal(exit.code, 0); report.closeMs = Date.now() - closeStarted;
  report.checks.push('custom close button awaits native supervisor cleanup and exits');
  assert.deepEqual(report.errors, []); report.passed = true; report.stage = 'complete';
} catch (error) {
  report.passed = false; report.failure = error.stack ?? String(error); report.stderr = stderr; report.processExit = exit;
  report.appLog = await readFile(path.join(output, 'logs/terminder.log'), 'utf8')
    .then(text => text.slice(-16384)).catch(error => `Application log unavailable: ${error.code ?? error.message}`);
  if (browser) { for (const page of browser.contexts().flatMap(c => c.pages())) { try { report.windowTrace = await page.evaluate(() => window.__windowTrace); await page.screenshot({ path: path.join(output, 'failure.png') }); } catch { /* The renderer may have already exited. */ } } }
  process.exitCode = 1;
} finally {
  if (!exit) child.kill();
  if (browser) { try { await browser.close(); } catch { /* Connection closes with the owned application. */ } }
  await writeFile(path.join(output, 'report.json'), JSON.stringify(report, null, 2));
  console.log(JSON.stringify(report, null, 2));
}
