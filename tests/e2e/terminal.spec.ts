import { test, expect } from '../../apps/desktop/tests/harness';

test('browser preview is honest about the missing desktop connection', async ({ page }) => {
  const errors: string[] = []; page.on('pageerror', e => errors.push(e.message));
  await page.goto('/');
  await expect(page.getByRole('alert')).toContainText('桌面连接未建立');
  await expect(page.getByRole('button', { name: '新建终端', exact: true }).first()).toBeDisabled();
  await page.screenshot({ path: 'test-results/welcome.png', fullPage: true });
  expect(errors).toEqual([]);
});

test('real xterm parses split UTF-8, alternate screen and denies OSC 52', async ({ page }) => {
  await page.goto('/');
  const result = await page.evaluate(async () => {
    const { Terminal } = await import('/node_modules/@xterm/xterm/lib/xterm.mjs');
    const container = document.createElement('div'); container.style.cssText = 'position:fixed;inset:0;background:#11151b'; document.body.append(container);
    const terminal = new Terminal({ cols: 80, rows: 24, allowProposedApi: false }); terminal.open(container);
    const inputs: string[] = []; terminal.onData((data: string) => inputs.push(data));
    let blocked = 0; terminal.parser.registerOscHandler(52, () => { blocked++; return true; });
    const write = (bytes: string | Uint8Array) => new Promise<void>(resolve => terminal.write(bytes, resolve));
    await write(Uint8Array.of(0xe4, 0xb8)); await write(Uint8Array.of(0xad, 0xf0, 0x9f)); await write(Uint8Array.of(0x98, 0x80));
    const normal = terminal.buffer.active.getLine(0)?.translateToString(true);
    await write('\x1b[?1049h\x1b[Halternate'); const alternate = terminal.buffer.active.type;
    await write('\x1b[?1049l'); const restored = terminal.buffer.active.getLine(0)?.translateToString(true);
    await write('\x1b]52;c;YXR0YWNr\x07\x1b]52;c;?\x07');
    await write('\x1b[6n');
    terminal.dispose(); return { normal, alternate, restored, blocked, inputs };
  });
  expect(result.normal).toBe('中😀'); expect(result.alternate).toBe('alternate'); expect(result.restored).toBe('中😀');
  expect(result.blocked).toBe(2); expect(result.inputs.some((s: string) => s.startsWith('\x1b[') && s.endsWith('R'))).toBe(true);
  expect(result.inputs.some((s: string) => s.includes('52;'))).toBe(false);
});
