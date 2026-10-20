import { test, expect } from '../../apps/desktop/tests/harness';
import { installHost } from './mock-host';
test.beforeEach(async ({ page }) => { await installHost(page); await page.goto('/'); await expect(page.getByRole('tab')).toHaveCount(1); });

test('switching tabs and settings preserves terminal sessions', async ({ page }) => {
  const errors: string[] = []; page.on('pageerror', e => errors.push(e.message));
  await expect.poll(() => page.evaluate(() => (window as any).__testEvidence.starts)).toBe(1);
  await page.getByRole('button', { name: '新建终端', exact: true }).click(); await expect(page.getByRole('tab')).toHaveCount(2);
  await expect.poll(() => page.evaluate(() => (window as any).__testEvidence.starts)).toBe(2);
  await page.getByRole('tab').first().click(); await page.getByRole('button', { name: '设置', exact: true }).click();
  await expect(page.getByRole('dialog')).toBeVisible(); await page.getByRole('combobox', { name: '主题', exact: true }).selectOption('light'); await page.getByRole('button', { name: '保存设置' }).click();
  await expect(page.getByRole('status').filter({ hasText: '已保存' })).toBeVisible();
  await page.screenshot({ path: 'test-results/settings.png', fullPage: true });
  await page.getByRole('button', { name: '关闭设置' }).click();
  expect(await page.evaluate(() => (window as any).__testEvidence.starts)).toBe(2);
  await page.screenshot({ path: 'test-results/terminal.png', fullPage: true }); expect(errors).toEqual([]);
});

test('application shortcuts leave Ctrl+C and paste confirmation under user control', async ({ page }) => {
  await expect.poll(() => page.evaluate(() => (window as any).__testEvidence.acknowledgments.length)).toBeGreaterThan(0);
  await expect.poll(() => page.evaluate(() => (window as any).__testEvidence.input.some((bytes: number[]) => bytes[0] === 27 && bytes.at(-1) === 82))).toBe(true);
  await page.locator('.xterm-helper-textarea').focus(); await page.keyboard.press('Control+c');
  await expect.poll(() => page.evaluate(() => (window as any).__testEvidence.input.some((bytes: number[]) => bytes.length === 1 && bytes[0] === 3))).toBe(true);
  await page.evaluate(() => { (window as any).__testEvidence.clipboard = 'echo one\necho two'; });
  const before = await page.evaluate(() => (window as any).__testEvidence.input.length);
  await page.keyboard.press('Control+Shift+V'); await expect(page.getByRole('dialog')).toContainText('确认粘贴');
  await page.getByRole('button', { name: '取消', exact: true }).click();
  expect(await page.evaluate(() => (window as any).__testEvidence.input.length)).toBe(before);
  await page.keyboard.press('Control+Shift+F'); await expect(page.getByRole('textbox', { name: '搜索当前终端' })).toBeFocused();
  await page.getByRole('textbox', { name: '搜索当前终端' }).fill('PowerShell'); await expect(page.locator('.search-bar')).toContainText('已定位');
});

test('closing the final session waits for cleanup and returns to the welcome view', async ({ page }) => {
  await expect.poll(() => page.evaluate(() => (window as any).__testEvidence.starts)).toBe(1);
  await page.getByRole('button', { name: '关闭 PowerShell', exact: true }).click();
  await expect(page.getByRole('dialog')).toContainText('关闭这个终端'); await page.getByRole('button', { name: '关闭终端', exact: true }).click();
  await expect(page.getByRole('tab')).toHaveCount(0); await expect(page.getByRole('heading', { level: 1 })).toBeVisible();
  const calls = await page.evaluate(() => (window as any).__testEvidence.events as string[]);
  expect(calls.indexOf('forget_session')).toBeGreaterThan(calls.indexOf('close_session'));
});
