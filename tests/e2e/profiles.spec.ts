import { test, expect } from '../../apps/desktop/tests/harness';
import { installHost } from './mock-host';

test.beforeEach(async ({ page }) => {
  await installHost(page);
  await page.goto('/');
  await expect(page.getByText('运行中', { exact: true })).toBeVisible();
});

test('plus opens a chooser and only the selected terminal type creates a tab', async ({ page }) => {
  const trigger = page.getByRole('button', { name: '新建终端', exact: true });
  await expect(page.getByRole('button', { name: '选择 Profile', exact: true })).toHaveCount(0);
  await trigger.click();
  await expect(page.getByRole('menu', { name: '选择终端类型' })).toBeVisible();
  await expect(trigger).toHaveAttribute('aria-expanded', 'true');
  expect(await page.evaluate(() => (window as any).__testEvidence.preparations)).toEqual(['default']);
  await expect(page.getByRole('tab')).toHaveCount(1);
  await page.getByRole('menuitem', { name: '命令提示符', exact: true }).click();
  await expect(page.getByRole('tab', { name: '命令提示符', exact: true })).toHaveAttribute('aria-selected', 'true');
  await expect(page.getByRole('tab')).toHaveCount(2);
  await expect(page.getByRole('menu')).toHaveCount(0);
  expect(await page.evaluate(() => (window as any).__testEvidence.preparations)).toEqual(['default', 'cmd']);
  await page.keyboard.press('Control+Shift+T');
  await expect(page.getByRole('tab')).toHaveCount(3);
  await expect.poll(() => page.evaluate(() => (window as any).__testEvidence.starts)).toBe(3);
  expect(await page.evaluate(() => (window as any).__testEvidence.preparations)).toEqual(['default', 'cmd', 'default']);
});

test('chooser supports keyboard navigation, dismissal and configuration access', async ({ page }) => {
  const trigger = page.getByRole('button', { name: '新建终端', exact: true });
  const menu = page.getByRole('menu', { name: '选择终端类型' });
  await trigger.focus();
  await trigger.press('ArrowDown');
  await expect(page.getByRole('menuitem', { name: 'PowerShell 7', exact: true })).toBeFocused();
  await page.keyboard.press('ArrowDown');
  await expect(page.getByRole('menuitem', { name: '命令提示符', exact: true })).toBeFocused();
  await page.keyboard.press('Escape');
  await expect(menu).toHaveCount(0);
  await expect(trigger).toBeFocused();
  await trigger.click();
  await page.getByRole('tab').click();
  await expect(menu).toHaveCount(0);
  await trigger.click();
  await trigger.click();
  await expect(menu).toHaveCount(0);
  await trigger.press('ArrowDown');
  await page.keyboard.press('Tab');
  await expect(menu).toHaveCount(0);
  await trigger.press('ArrowUp');
  await expect(page.getByRole('menuitem', { name: '管理终端配置', exact: true })).toBeFocused();
  await page.keyboard.press('Enter');
  await expect(menu).toHaveCount(0);
  await expect(page.getByRole('dialog')).toBeVisible();
  expect(await page.evaluate(() => (window as any).__testEvidence.preparations)).toEqual(['default']);
});

test('chooser stays within a narrow window beside overflowing tabs in both themes', async ({ page }) => {
  await page.setViewportSize({ width: 640, height: 400 });
  for (let count = 2; count <= 6; count++) {
    await page.keyboard.press('Control+Shift+T');
    await expect(page.getByRole('tab')).toHaveCount(count);
  }
  await expect(page.getByText('运行中', { exact: true })).toBeVisible();
  const trigger = page.getByRole('button', { name: '新建终端', exact: true });
  const menu = page.getByRole('menu', { name: '选择终端类型' });
  await trigger.click();
  const box = await menu.boundingBox();
  expect(box!.x).toBeGreaterThanOrEqual(12);
  expect(box!.x + box!.width).toBeLessThanOrEqual(628);
  expect(box!.y + box!.height).toBeLessThanOrEqual(388);
  await page.screenshot({ path: 'test-results/profile-menu-dark.png' });
  await page.getByRole('button', { name: '设置', exact: true }).click();
  await expect(menu).toHaveCount(0);
  await page.getByRole('combobox', { name: '主题', exact: true }).selectOption('light');
  await page.getByRole('button', { name: '保存设置', exact: true }).click();
  await expect(page.getByRole('status').filter({ hasText: '已保存' })).toBeVisible();
  await page.getByRole('button', { name: '关闭设置', exact: true }).click();
  await trigger.click();
  await expect(menu).toBeVisible();
  await page.screenshot({ path: 'test-results/profile-menu-light.png' });
});
