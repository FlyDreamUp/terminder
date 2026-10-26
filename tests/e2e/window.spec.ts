import { test, expect } from '../../apps/desktop/tests/harness';
import { installHost } from './mock-host';

test('integrated window buttons use native actions and preserve sessions', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', error => errors.push(error.message));
  await installHost(page);
  await page.goto('/');
  await expect(page.getByRole('tab')).toHaveCount(1);
  await page.getByRole('button', { name: '最大化窗口', exact: true }).click();
  await expect(page.getByRole('button', { name: '还原窗口', exact: true })).toBeVisible();
  await page.getByRole('button', { name: '还原窗口', exact: true }).click();
  await expect(page.getByRole('button', { name: '最大化窗口', exact: true })).toBeVisible();
  await page.getByRole('button', { name: '最小化窗口', exact: true }).click();
  await expect.poll(() => page.evaluate(() => (window as any).__testEvidence.windowState.minimized)).toBe(true);
  await page.getByRole('button', { name: '关闭窗口', exact: true }).click();
  await expect.poll(() => page.evaluate(() => (window as any).__testEvidence.windowState.closeRequests)).toBe(1);
  await expect(page.getByRole('tab')).toHaveCount(1);
  const commands = await page.evaluate(() => (window as any).__testEvidence.events as string[]);
  expect(commands).not.toContain('close_session');
  expect(commands).not.toContain('plugin:window|destroy');
  expect(errors).toEqual([]);
});

test('titlebar remains usable at the minimum window size and in both themes', async ({ page }) => {
  await installHost(page);
  await page.setViewportSize({ width: 640, height: 400 });
  await page.goto('/');
  await expect(page.getByRole('tab')).toHaveCount(1);
  const close = page.getByRole('button', { name: '关闭窗口', exact: true });
  await expect(close).toBeVisible();
  const box = await close.boundingBox();
  expect(box!.x + box!.width).toBeLessThanOrEqual(640);
  expect(box!.y).toBe(0);
  await page.screenshot({ path: 'test-results/window-dark.png' });
  await page.getByRole('button', { name: '设置', exact: true }).click();
  await page.getByRole('combobox', { name: '主题', exact: true }).selectOption('light');
  await page.getByRole('button', { name: '保存设置', exact: true }).click();
  await expect(page.getByRole('status').filter({ hasText: '已保存' })).toBeVisible();
  await page.getByRole('button', { name: '关闭设置', exact: true }).click();
  await expect(close).toBeVisible();
  await page.screenshot({ path: 'test-results/window-light.png' });
  await expect(page.locator('.topbar')).toHaveAttribute('data-tauri-drag-region', 'deep');
  await expect(page.getByRole('group', { name: '窗口控制' })).toHaveAttribute('data-tauri-drag-region', 'false');
});

test('browser preview does not display nonfunctional native window buttons', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByRole('alert')).toContainText('桌面连接未建立');
  await expect(page.getByRole('group', { name: '窗口控制' })).toHaveCount(0);
});
