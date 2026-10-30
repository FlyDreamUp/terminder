import { test, expect } from '../../apps/desktop/tests/harness';
import { installHost } from './mock-host';

test('duplicate finished state removes only the requested tab once', async ({ page }) => {
  await installHost(page, { repeatFinishedState: true });
  await page.goto('/');
  await expect(page.getByRole('tab')).toHaveCount(1);
  await page.keyboard.press('Control+Shift+T');
  await expect.poll(() => page.evaluate(() => (window as any).__testEvidence.acknowledgments.length)).toBe(2);
  await page.getByRole('button', { name: '关闭 PowerShell 7', exact: true }).first().click();
  await page.getByRole('button', { name: '关闭终端', exact: true }).click();
  await expect(page.getByRole('tab')).toHaveCount(1);
  expect(await page.evaluate(() => (window as any).__testEvidence.events.filter((event: string) => event === 'forget_session'))).toHaveLength(1);
});

test('a rejected forget keeps the finished tab and permits an explicit retry', async ({ page }) => {
  await installHost(page, { forgetFailures: 1 });
  await page.goto('/');
  await expect.poll(() => page.evaluate(() => (window as any).__testEvidence.acknowledgments.length)).toBe(1);
  await page.getByRole('button', { name: '关闭 PowerShell 7', exact: true }).click();
  await page.getByRole('button', { name: '关闭终端', exact: true }).click();
  await expect(page.getByRole('alert')).toContainText('宿主繁忙');
  await expect(page.getByRole('button', { name: '关闭 PowerShell 7', exact: true })).toBeEnabled();
  await page.getByRole('button', { name: '关闭 PowerShell 7', exact: true }).click();
  await expect(page.getByRole('tab')).toHaveCount(0);
});

test('a delayed clipboard read cannot paste into a newly selected terminal', async ({ page }) => {
  await installHost(page, { clipboardDelayMs: 500 });
  await page.goto('/');
  await expect.poll(() => page.evaluate(() => (window as any).__testEvidence.acknowledgments.length)).toBe(1);
  await page.evaluate(() => { (window as any).__testEvidence.clipboard = 'do-not-send-to-another-shell'; });
  await page.locator('.xterm-helper-textarea').focus();
  await page.keyboard.press('Control+Shift+V');
  await expect.poll(() => page.evaluate(() => (window as any).__testEvidence.events.includes('read_clipboard'))).toBe(true);
  await page.keyboard.press('Control+Shift+T');
  await expect(page.getByRole('tab')).toHaveCount(2);
  await expect.poll(() => page.evaluate(() => (window as any).__testEvidence.clipboardReads)).toBe(1);
  const pasted = await page.evaluate(() => (window as any).__testEvidence.input.some((bytes: number[]) => new TextDecoder().decode(Uint8Array.from(bytes)).includes('do-not-send-to-another-shell')));
  expect(pasted).toBe(false);
});

test('tail output is rendered when creation closes before a running state arrives', async ({ page }) => {
  await installHost(page, { finishBeforeRunning: true });
  await page.goto('/');
  await expect(page.locator('.finished-bar')).toContainText('会话已结束');
  await expect(page.getByRole('listitem').filter({ hasText: 'Browser component fixture' })).toBeVisible();
  await expect.poll(() => page.evaluate(() => (window as any).__testEvidence.acknowledgments.length)).toBe(1);
  await expect(page.getByRole('alert')).toHaveCount(0);
});

test('settings preserve a draft when discarding on reload or category change is declined', async ({ page }) => {
  await installHost(page);
  await page.goto('/');
  await expect(page.getByRole('tab')).toHaveCount(1);
  await page.getByRole('button', { name: '设置', exact: true }).click();
  await page.getByRole('button', { name: 'Profiles 与配置', exact: true }).click();
  const source = page.getByRole('textbox', { name: 'TOML 配置' });
  const draft = `${await source.inputValue()}# unsaved draft\n`;
  await source.fill(draft);
  const confirmations: string[] = [];
  page.on('dialog', async dialog => { confirmations.push(dialog.message()); await dialog.dismiss(); });
  await page.getByRole('button', { name: '重新加载', exact: true }).click();
  await expect(source).toHaveValue(draft);
  await page.getByRole('button', { name: '外观', exact: true }).click();
  await expect(source).toBeVisible();
  await expect(source).toHaveValue(draft);
  expect(confirmations).toEqual(['放弃尚未保存的设置？', '放弃尚未保存的设置？']);
});
